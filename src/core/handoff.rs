use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs, io,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    sync::{Mutex, OnceLock},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[cfg(unix)]
use std::os::unix::process::CommandExt;

use serde_json::{Value, json};

use super::{
    compiler::{self, CompilerError},
    model::{
        CapsuleCompileOutput, CapsuleCompileRequest, ChecklistItem, CompilerPresetStatus,
        TimelineEvent, TimelineKind, WorkCapsule,
    },
};

const COMPILER_PREFIX: &str = "agent";
const DEFAULT_TIMEOUT_MS: u64 = 180_000;
const MAX_ARTIFACT_CHARS: usize = 40_000;
const MAX_CONTEXT_EVENTS: usize = 80;
const MAX_CONTEXT_COMPACTS: usize = 5;
const MAX_CONTEXT_EVIDENCE: usize = 40;
const MAX_CONTEXT_EXCERPT_CHARS: usize = 480;
const CLAUDE_PLUGIN_NAME: &str = "moonbox-handoff";
const AGENT_CHILD_TERM_GRACE_MS: u64 = 80;
static PYTHON_MODULE_CACHE: OnceLock<Mutex<BTreeMap<(String, String), bool>>> = OnceLock::new();
const CODEX_PYTHON_BRIDGE: &str = r#"
import json
import sys

from openai_codex import Codex, Sandbox

try:
    from openai_codex import CodexConfig
except Exception:
    CodexConfig = None


payload = json.load(sys.stdin)


def codex_client():
    codex_bin = payload.get("codex_bin")
    if codex_bin and CodexConfig is not None:
        config = CodexConfig(codex_bin=codex_bin)
        try:
            return Codex(config)
        except TypeError:
            return Codex(config=config)
    return Codex()


def start_thread(client):
    kwargs = {"sandbox": Sandbox.read_only}
    if payload.get("cwd"):
        kwargs["cwd"] = payload["cwd"]
    if payload.get("model"):
        kwargs["model"] = payload["model"]
    try:
        return client.thread_start(**kwargs)
    except TypeError:
        kwargs.pop("cwd", None)
        return client.thread_start(**kwargs)


def run_turn(thread):
    prompt = payload["prompt"]
    try:
        return thread.run(prompt, sandbox=Sandbox.read_only)
    except TypeError:
        return thread.run(prompt)


with codex_client() as codex:
    thread = start_thread(codex)
    result = run_turn(thread)

artifact = (
    getattr(result, "final_response", None)
    or getattr(result, "text", None)
    or getattr(result, "output_text", None)
    or str(result)
)
print(json.dumps({"artifact": artifact, "warnings": []}, ensure_ascii=False))
"#;
const CLAUDE_PYTHON_BRIDGE: &str = r#"
import asyncio
import json
import sys

from claude_agent_sdk import ClaudeAgentOptions, query


payload = json.load(sys.stdin)


async def main():
    plugin_skill = f"{payload['plugin_name']}:{payload['skill_id']}"
    prompt = payload["prompt"]
    options = ClaudeAgentOptions(
        cwd=payload.get("cwd"),
        setting_sources=[],
        plugins=[{"type": "local", "path": payload["plugin_path"]}],
        skills=[plugin_skill],
        allowed_tools=["Skill"],
        disallowed_tools=[
            "Bash",
            "Edit",
            "Write",
            "Read",
            "Glob",
            "Grep",
            "WebFetch",
            "WebSearch",
            "NotebookEdit",
        ],
        permission_mode="dontAsk",
        max_turns=payload.get("max_turns", 3),
        output_format={
            "type": "json_schema",
            "schema": {
                "type": "object",
                "properties": {
                    "artifact": {"type": "string"},
                    "warnings": {"type": "array", "items": {"type": "string"}},
                },
                "required": ["artifact"],
            },
        },
    )
    final = {"artifact": "", "warnings": []}
    async for message in query(prompt=prompt, options=options):
        structured = getattr(message, "structured_output", None)
        if isinstance(structured, dict) and structured.get("artifact"):
            final["artifact"] = structured["artifact"]
            final["warnings"] = structured.get("warnings") or []
        result = getattr(message, "result", None)
        if result and not final["artifact"]:
            final["artifact"] = result
        subtype = getattr(message, "subtype", None)
        if subtype and subtype != "success":
            final.setdefault("warnings", []).append(f"claude_result:{subtype}")
    print(json.dumps(final, ensure_ascii=False))


asyncio.run(main())
"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentRunner {
    Codex,
    Claude,
}

impl AgentRunner {
    pub fn id(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Codex => "Codex",
            Self::Claude => "Claude",
        }
    }

    pub fn bin_env(self) -> &'static str {
        match self {
            Self::Codex => "MOONBOX_CODEX_SDK_PYTHON",
            Self::Claude => "MOONBOX_CLAUDE_AGENT_SDK_PYTHON",
        }
    }

    pub fn sdk_module(self) -> &'static str {
        match self {
            Self::Codex => "openai_codex",
            Self::Claude => "claude_agent_sdk",
        }
    }

    pub fn sdk_package(self) -> &'static str {
        match self {
            Self::Codex => "openai-codex",
            Self::Claude => "claude-agent-sdk",
        }
    }

    pub fn cli_command(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
        }
    }
}

#[derive(Debug, Clone)]
pub struct HandoffSkill {
    pub id: String,
    pub name: String,
    pub description: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct AgentCompilerSpec {
    pub runner: AgentRunner,
    pub skill_id: String,
}

#[derive(Debug, Clone)]
pub struct RunnerPreflight {
    pub status: CompilerPresetStatus,
    pub reason: String,
    pub command: Option<String>,
}

pub fn compiler_id(runner: AgentRunner, skill_id: &str) -> String {
    format!("{COMPILER_PREFIX}:{}:{skill_id}", runner.id())
}

pub fn parse_compiler_id(id: &str) -> Option<AgentCompilerSpec> {
    let mut parts = id.splitn(3, ':');
    if parts.next()? != COMPILER_PREFIX {
        return None;
    }
    let runner = match parts.next()? {
        "codex" => AgentRunner::Codex,
        "claude" => AgentRunner::Claude,
        _ => return None,
    };
    let skill_id = parts.next()?.trim();
    (!skill_id.is_empty()).then(|| AgentCompilerSpec {
        runner,
        skill_id: skill_id.into(),
    })
}

pub fn discover_handoff_skills() -> Vec<HandoffSkill> {
    let mut skills = Vec::new();
    for root in skill_roots() {
        discover_skills_under(&root, &mut skills);
    }
    skills.sort_by(|left, right| left.id.cmp(&right.id).then(left.path.cmp(&right.path)));
    skills.dedup_by(|left, right| left.id == right.id && left.path == right.path);
    skills
}

pub fn find_handoff_skill(skill_id: &str) -> Option<HandoffSkill> {
    discover_handoff_skills()
        .into_iter()
        .find(|skill| skill.id == skill_id)
}

pub fn runner_preflight(runner: AgentRunner) -> RunnerPreflight {
    let candidates = python_candidates_for_runner(runner);
    if candidates.is_empty() {
        return RunnerPreflight {
            status: CompilerPresetStatus::Warning,
            reason: runner_sdk_missing_reason(runner, &[]),
            command: None,
        };
    }

    let configured = configured_runner_command(runner);
    for command in &candidates {
        if !command_available(command) {
            continue;
        }
        if python_module_available(command, runner.sdk_module()) {
            let auth_reason = match runner {
                AgentRunner::Codex => codex_auth_reason(),
                AgentRunner::Claude => claude_auth_reason(),
            };
            if let Some(reason) = auth_reason {
                return RunnerPreflight {
                    status: CompilerPresetStatus::Warning,
                    reason,
                    command: Some(command.clone()),
                };
            }
            return RunnerPreflight {
                status: CompilerPresetStatus::Ready,
                reason: format!(
                    "{} Python SDK is installed in {command}; auth preflight passed",
                    runner.label()
                ),
                command: Some(command.clone()),
            };
        }
    }

    if configured
        .as_deref()
        .is_some_and(|command| !command_available(command))
    {
        return RunnerPreflight {
            status: CompilerPresetStatus::Warning,
            reason: runner_python_missing_reason(runner, configured.as_deref().unwrap_or_default()),
            command: configured,
        };
    }

    RunnerPreflight {
        status: CompilerPresetStatus::Warning,
        reason: runner_sdk_missing_reason(runner, &candidates),
        command: candidates
            .into_iter()
            .find(|command| command_available(command)),
    }
}

pub fn compile_with_agent_runner(
    request: &CapsuleCompileRequest,
    spec: AgentCompilerSpec,
) -> Result<CapsuleCompileOutput, CompilerError> {
    let skill = find_handoff_skill(&spec.skill_id).ok_or_else(|| CompilerError::InvalidConfig {
        name: request.compiler.clone(),
        reason: format!("skill_not_found: {}", spec.skill_id),
    })?;
    let preflight = runner_preflight(spec.runner);
    if preflight.status != CompilerPresetStatus::Ready {
        return Err(CompilerError::InvalidConfig {
            name: request.compiler.clone(),
            reason: preflight.reason,
        });
    }
    let skill_source = fs::read_to_string(&skill.path).map_err(|error| CompilerError::Io {
        compiler: request.compiler.clone(),
        stream: "skill",
        reason: error.to_string(),
    })?;
    let prompt = build_agent_prompt(request, &skill, &skill_source);
    let artifact = if let Ok(fake) = env::var("MOONBOX_AGENT_HANDOFF_FAKE_OUTPUT") {
        fake
    } else {
        run_agent(request, spec.runner, &skill, &skill_source, &prompt)?
    };
    let artifact = validate_agent_artifact(request, artifact)?;
    let capsule = normalize_agent_artifact(request, spec.runner, &skill, &artifact);
    Ok(compiler::enrich_compile_output(
        request,
        CapsuleCompileOutput {
            version: 1,
            capsule,
        },
    ))
}

pub fn build_agent_prompt(
    request: &CapsuleCompileRequest,
    skill: &HandoffSkill,
    skill_source: &str,
) -> String {
    let context = context_pack_markdown(request);
    format!(
        "\
You are running a Moonbox continuation handoff job.

Use the selected community handoff skill exactly as the handoff-writing policy:

<selected_skill path=\"{}\">
{}
</selected_skill>

Moonbox safety constraints:
- Source session stores are read-only.
- Do not open, resume, or launch the source session.
- Do not scan the user's real source store. Use only the bounded context below.
- Write a continuation handoff artifact for the target executor.
- Preserve the source session's language and do not translate quoted session content.
- Redact secrets and sensitive paths already marked by Moonbox.

{}
",
        skill.path.display(),
        skill_source.trim(),
        context
    )
}

fn normalize_agent_artifact(
    request: &CapsuleCompileRequest,
    runner: AgentRunner,
    skill: &HandoffSkill,
    artifact: &str,
) -> WorkCapsule {
    let artifact = bounded_artifact(artifact);
    let session = &request.source_session;
    let rewind = request
        .timeline
        .events
        .iter()
        .find(|event| event.id == request.rewind_event_id)
        .map(|event| format!("{} / {}", event.id, truncate(&event.detail, 140)))
        .unwrap_or_else(|| request.rewind_event_id.clone());
    WorkCapsule {
        version: 1,
        source_cli: request.source_cli,
        target_cli: request.target_cli,
        source_session: session.id.clone(),
        rewind_point: rewind,
        compiler: request.compiler.clone(),
        handoff_label: format!(
            "moonbox/{}/{}-{}",
            request.target_cli.id(),
            runner.id(),
            request.rewind_event_id
        ),
        goal: format!(
            "Continue {} from the selected rewind point using an AI-generated handoff.",
            truncate(&session.title, 96)
        ),
        state: "handoff_ready".into(),
        decisions: vec![
            format!("Generated by {} runner.", runner.label()),
            format!("Used community handoff skill {}.", skill.name),
            "Moonbox supplied a bounded context pack instead of granting source-store access."
                .into(),
        ],
        todo: vec![
            ChecklistItem {
                done: true,
                text: "Review the generated handoff artifact.".into(),
            },
            ChecklistItem {
                done: false,
                text: format!(
                    "Continue in {} after confirming the handoff.",
                    request.target_cli
                ),
            },
        ],
        evidence: vec![
            format!("source session: {} ({})", session.id, session.cli),
            format!("skill path: {}", skill.path.display()),
            format!("artifact excerpt: {}", single_line_excerpt(&artifact, 220)),
        ],
        risks: vec![
            "AI-generated handoff must be reviewed before launching a target CLI.".into(),
            "Moonbox did not mutate the source session store.".into(),
        ],
        handoff_artifact: Some(artifact),
        handoff_runner: Some(runner.label().into()),
        handoff_skill: Some(skill.name.clone()),
        raw_source_map: None,
        raw_refs: Vec::new(),
        coverage: Default::default(),
        redaction: request.redaction.clone(),
    }
}

fn context_pack_markdown(request: &CapsuleCompileRequest) -> String {
    let events_through_rewind = events_through_rewind(request).collect::<Vec<_>>();
    let selected_start = events_through_rewind
        .len()
        .saturating_sub(MAX_CONTEXT_EVENTS);
    let selected_events = &events_through_rewind[selected_start..];
    let omitted_events = selected_start;
    let mut lines = vec![
        "# Moonbox Handoff Context Pack".into(),
        String::new(),
        "## Selection".into(),
        format!("- Source CLI: {}", request.source_cli),
        format!("- Target CLI: {}", request.target_cli),
        format!("- Source session: {}", request.source_session.id),
        format!("- Title: {}", request.source_session.title),
        format!("- Cwd: {}", request.source_session.cwd),
        format!(
            "- Branch: {}",
            request.source_session.branch.as_deref().unwrap_or("-")
        ),
        format!("- Rewind event: {}", request.rewind_event_id),
        format!("- Token budget: {}", request.token_budget),
        String::new(),
        "## Context Bounds".into(),
        "- Scope: read-only Moonbox inventory already loaded for the selected rewind.".into(),
        "- Source store access: not granted to the runner.".into(),
        format!(
            "- Timeline events through rewind: {}",
            events_through_rewind.len()
        ),
        format!("- Included recent events: {}", selected_events.len()),
        format!("- Omitted older events: {}", omitted_events),
        format!("- Max event excerpt chars: {}", MAX_CONTEXT_EXCERPT_CHARS),
        String::new(),
        "## Session Index".into(),
        format!("- Updated: {}", request.source_session.updated),
        format!("- Status: {:?}", request.source_session.status),
        format!(
            "- Runtime: {:?}{}",
            request.source_session.runtime_status,
            request
                .source_session
                .runtime_reason
                .as_deref()
                .map(|reason| format!(" / {}", single_line_excerpt(reason, 180)))
                .unwrap_or_default()
        ),
        format!(
            "- Tokens: {}",
            request
                .source_session
                .token_count
                .map(|tokens| tokens.to_string())
                .unwrap_or_else(|| "-".into())
        ),
        format!(
            "- Raw size bytes: {}",
            request
                .source_session
                .source_size_bytes
                .map(|bytes| bytes.to_string())
                .unwrap_or_else(|| "-".into())
        ),
        format!(
            "- Source path: {}",
            request.source_session.source_path.as_deref().unwrap_or("-")
        ),
        format!(
            "- Health: {}",
            request
                .source_session
                .health_reason
                .as_deref()
                .map(|reason| single_line_excerpt(reason, 220))
                .unwrap_or_else(|| "-".into())
        ),
    ];
    push_anatomy_index(request, &mut lines);
    lines.extend([
        String::new(),
        "## Redaction".into(),
        format!("- Policy: {}", request.redaction.policy),
        format!("- Secrets redacted: {}", request.redaction.secrets_redacted),
        format!("- Paths redacted: {}", request.redaction.paths_redacted),
        format!(
            "- Prompt-injection warnings: {}",
            request.redaction.prompt_injection_warnings
        ),
    ]);
    if !request.redaction.warnings.is_empty() {
        lines.push(format!(
            "- Warnings: {}",
            request
                .redaction
                .warnings
                .iter()
                .take(5)
                .map(|warning| single_line_excerpt(warning, 160))
                .collect::<Vec<_>>()
                .join(" | ")
        ));
    }
    push_compact_frontier(&events_through_rewind, &mut lines);
    push_tool_evidence(&events_through_rewind, &mut lines);
    push_file_evidence(&events_through_rewind, &mut lines);
    push_attachment_refs(&events_through_rewind, &mut lines);
    lines.extend([String::new(), "## Selected Rewind Window".into()]);
    for event in selected_events {
        lines.push(format!(
            "- [{}] {} {:?}: {} :: {}",
            event.id,
            event.time,
            event.kind,
            event.title,
            single_line_excerpt(&event.detail, MAX_CONTEXT_EXCERPT_CHARS)
        ));
    }
    lines.join("\n")
}

fn push_anatomy_index(request: &CapsuleCompileRequest, lines: &mut Vec<String>) {
    let Some(anatomy) = request.source_session.anatomy.as_ref() else {
        return;
    };
    lines.push(format!("- Anatomy status: {:?}", anatomy.status));
    if !anatomy.scan_scope.is_empty() {
        lines.push(format!("- Anatomy scope: {}", anatomy.scan_scope));
    }
    if let Some(compact) = anatomy.compact.as_ref() {
        lines.push(format!(
            "- Anatomy compact frontier: {}{} / tail {} lines, {} bytes",
            compact.label,
            compact
                .line_number
                .map(|line| format!(" line {line}"))
                .unwrap_or_default(),
            compact.tail_lines,
            compact.tail_bytes
        ));
        if !compact.detail.is_empty() {
            lines.push(format!(
                "- Anatomy compact detail: {}",
                single_line_excerpt(&compact.detail, 260)
            ));
        }
    }
    if let Some(tokens) = anatomy.token_profile.as_ref() {
        lines.push(format!(
            "- Anatomy tokens: input={} output={} cache_read={} cache_write={} reasoning={} total={}",
            tokens.input,
            tokens.output,
            tokens.cache_read,
            tokens.cache_write,
            tokens.reasoning,
            tokens.total
        ));
    }
    if !anatomy.value_signals.is_empty() {
        let signals = anatomy
            .value_signals
            .iter()
            .take(5)
            .map(|signal| {
                format!(
                    "{}={} ({})",
                    signal.label,
                    signal.value,
                    single_line_excerpt(&signal.detail, 100)
                )
            })
            .collect::<Vec<_>>()
            .join(" | ");
        lines.push(format!("- Anatomy signals: {signals}"));
    }
}

fn push_compact_frontier(events: &[&TimelineEvent], lines: &mut Vec<String>) {
    lines.extend([String::new(), "## Compact Frontier".into()]);
    let mut compact_events = events
        .iter()
        .rev()
        .filter(|event| event.kind == TimelineKind::Compact)
        .take(MAX_CONTEXT_COMPACTS)
        .copied()
        .collect::<Vec<_>>();
    compact_events.reverse();
    if compact_events.is_empty() {
        lines.push("- No compact event observed before the rewind point.".into());
        return;
    }
    for event in compact_events {
        lines.push(format!(
            "- [{}] {} :: {}",
            event.id,
            event.title,
            single_line_excerpt(&event.detail, MAX_CONTEXT_EXCERPT_CHARS)
        ));
    }
}

fn push_tool_evidence(events: &[&TimelineEvent], lines: &mut Vec<String>) {
    lines.extend([String::new(), "## Tool And Approval Evidence".into()]);
    let mut count = 0;
    for event in events {
        for call in &event.metadata.tool_calls {
            if count >= MAX_CONTEXT_EVIDENCE {
                lines.push(format!(
                    "- ... truncated after {MAX_CONTEXT_EVIDENCE} entries"
                ));
                return;
            }
            count += 1;
            lines.push(format!(
                "- [{}] call {}{}{}",
                event.id,
                call.name.as_deref().unwrap_or("unknown"),
                call.id
                    .as_deref()
                    .map(|id| format!(" id={id}"))
                    .unwrap_or_default(),
                call.arguments
                    .as_ref()
                    .map(|args| format!(" args={}", json_excerpt(args, 220)))
                    .unwrap_or_default()
            ));
        }
        for result in &event.metadata.tool_results {
            if count >= MAX_CONTEXT_EVIDENCE {
                lines.push(format!(
                    "- ... truncated after {MAX_CONTEXT_EVIDENCE} entries"
                ));
                return;
            }
            count += 1;
            lines.push(format!(
                "- [{}] result {} error={} :: {}",
                event.id,
                result.name.as_deref().unwrap_or("unknown"),
                result.is_error.unwrap_or(false),
                result
                    .content
                    .as_deref()
                    .map(|content| single_line_excerpt(content, 260))
                    .or_else(|| result.raw.as_ref().map(|raw| json_excerpt(raw, 260)))
                    .unwrap_or_else(|| "-".into())
            ));
        }
        for approval in &event.metadata.approvals {
            if count >= MAX_CONTEXT_EVIDENCE {
                lines.push(format!(
                    "- ... truncated after {MAX_CONTEXT_EVIDENCE} entries"
                ));
                return;
            }
            count += 1;
            lines.push(format!(
                "- [{}] approval action={} decision={} reason={}",
                event.id,
                approval.action.as_deref().unwrap_or("-"),
                approval.decision.as_deref().unwrap_or("-"),
                approval
                    .reason
                    .as_deref()
                    .map(|reason| single_line_excerpt(reason, 220))
                    .unwrap_or_else(|| "-".into())
            ));
        }
    }
    if count == 0 {
        lines.push("- No tool calls, tool results, or approvals were indexed.".into());
    }
}

fn push_file_evidence(events: &[&TimelineEvent], lines: &mut Vec<String>) {
    lines.extend([String::new(), "## File Change Evidence".into()]);
    let mut count = 0;
    for event in events {
        for change in &event.metadata.file_changes {
            if count >= MAX_CONTEXT_EVIDENCE {
                lines.push(format!(
                    "- ... truncated after {MAX_CONTEXT_EVIDENCE} entries"
                ));
                return;
            }
            count += 1;
            lines.push(format!(
                "- [{}] {} {} :: {}{}",
                event.id,
                change.operation.as_deref().unwrap_or("change"),
                change.path.as_deref().unwrap_or("-"),
                change
                    .summary
                    .as_deref()
                    .map(|summary| single_line_excerpt(summary, 240))
                    .unwrap_or_else(|| "-".into()),
                change
                    .diff
                    .as_deref()
                    .map(|diff| format!(" diff={}", single_line_excerpt(diff, 240)))
                    .unwrap_or_default()
            ));
        }
    }
    if count == 0 {
        lines.push("- No file changes were indexed.".into());
    }
}

fn push_attachment_refs(events: &[&TimelineEvent], lines: &mut Vec<String>) {
    lines.extend([String::new(), "## Attachments And Raw References".into()]);
    let mut count = 0;
    for event in events {
        for attachment in &event.metadata.attachments {
            if count >= MAX_CONTEXT_EVIDENCE {
                lines.push(format!(
                    "- ... truncated after {MAX_CONTEXT_EVIDENCE} entries"
                ));
                return;
            }
            count += 1;
            lines.push(format!(
                "- [{}] attachment id={} name={} path={} mime={} bytes={}",
                event.id,
                attachment.id.as_deref().unwrap_or("-"),
                attachment.name.as_deref().unwrap_or("-"),
                attachment.path.as_deref().unwrap_or("-"),
                attachment.mime_type.as_deref().unwrap_or("-"),
                attachment
                    .size_bytes
                    .map(|bytes| bytes.to_string())
                    .unwrap_or_else(|| "-".into())
            ));
        }
        for raw_ref in &event.metadata.raw_refs {
            if count >= MAX_CONTEXT_EVIDENCE {
                lines.push(format!(
                    "- ... truncated after {MAX_CONTEXT_EVIDENCE} entries"
                ));
                return;
            }
            count += 1;
            lines.push(format!(
                "- [{}] raw source={} line={} digest={} role={} type={}",
                event.id,
                raw_ref.source_path.as_deref().unwrap_or("-"),
                raw_ref
                    .line_number
                    .map(|line| line.to_string())
                    .unwrap_or_else(|| "-".into()),
                raw_ref.digest.as_deref().unwrap_or("-"),
                raw_ref.role.as_deref().unwrap_or("-"),
                raw_ref.record_type.as_deref().unwrap_or("-")
            ));
        }
    }
    if count == 0 {
        lines.push("- No attachments or raw references were indexed.".into());
    }
}

fn json_excerpt(value: &Value, max_chars: usize) -> String {
    serde_json::to_string(value)
        .map(|json| single_line_excerpt(&json, max_chars))
        .unwrap_or_else(|_| "<invalid-json>".into())
}

fn events_through_rewind(
    request: &CapsuleCompileRequest,
) -> impl Iterator<Item = &super::model::TimelineEvent> {
    let mut found = false;
    request.timeline.events.iter().take_while(move |event| {
        if found {
            return false;
        }
        if event.id == request.rewind_event_id {
            found = true;
        }
        true
    })
}

fn validate_agent_artifact(
    request: &CapsuleCompileRequest,
    artifact: String,
) -> Result<String, CompilerError> {
    let artifact = artifact.trim();
    if artifact.is_empty() {
        return Err(CompilerError::InvalidOutput {
            compiler: request.compiler.clone(),
            reason: "agent runner returned an empty handoff artifact".into(),
        });
    }
    Ok(artifact.into())
}

fn run_agent(
    request: &CapsuleCompileRequest,
    runner: AgentRunner,
    skill: &HandoffSkill,
    skill_source: &str,
    prompt: &str,
) -> Result<String, CompilerError> {
    match runner {
        AgentRunner::Codex => run_codex_agent_sdk(request, skill, prompt),
        AgentRunner::Claude => run_claude_agent_sdk(request, skill, skill_source, prompt),
    }
}

fn run_codex_agent_sdk(
    request: &CapsuleCompileRequest,
    skill: &HandoffSkill,
    prompt: &str,
) -> Result<String, CompilerError> {
    let timeout = agent_timeout()?;
    let python =
        runner_command(AgentRunner::Codex).ok_or_else(|| CompilerError::InvalidConfig {
            name: request.compiler.clone(),
            reason: "Codex SDK Python command is not configured".into(),
        })?;
    let payload = json!({
        "prompt": format!("${}\n\n{}", skill.name, prompt),
        "cwd": sdk_cwd(request),
        "skill_id": skill.id,
        "skill_path": skill.path.to_string_lossy(),
        "codex_bin": env::var("MOONBOX_CODEX_BIN").ok(),
        "model": env::var("MOONBOX_CODEX_MODEL").ok(),
    });
    let (stdout, stderr) = run_child_with_input(
        &request.compiler,
        &python,
        &["-c".into(), CODEX_PYTHON_BRIDGE.into()],
        serde_json::to_vec(&payload).map_err(|error| CompilerError::InvalidOutput {
            compiler: request.compiler.clone(),
            reason: error.to_string(),
        })?,
        timeout,
    )?;
    if !stderr.trim().is_empty() && stdout.trim().is_empty() {
        return Err(CompilerError::InvalidOutput {
            compiler: request.compiler.clone(),
            reason: truncate(stderr.trim(), 500),
        });
    }
    Ok(agent_artifact_from_output(&stdout))
}

fn run_claude_agent_sdk(
    request: &CapsuleCompileRequest,
    skill: &HandoffSkill,
    skill_source: &str,
    prompt: &str,
) -> Result<String, CompilerError> {
    let timeout = agent_timeout()?;
    let python =
        runner_command(AgentRunner::Claude).ok_or_else(|| CompilerError::InvalidConfig {
            name: request.compiler.clone(),
            reason: "Claude Agent SDK Python command is not configured".into(),
        })?;
    let plugin = TempClaudePlugin::new(skill, skill_source).map_err(|error| CompilerError::Io {
        compiler: request.compiler.clone(),
        stream: "plugin",
        reason: error.to_string(),
    })?;
    let payload = json!({
        "prompt": prompt,
        "cwd": sdk_cwd(request),
        "plugin_name": CLAUDE_PLUGIN_NAME,
        "plugin_path": plugin.root.to_string_lossy(),
        "skill_id": skill.id,
        "max_turns": 3,
    });
    let (stdout, stderr) = run_child_with_input(
        &request.compiler,
        &python,
        &["-c".into(), CLAUDE_PYTHON_BRIDGE.into()],
        serde_json::to_vec(&payload).map_err(|error| CompilerError::InvalidOutput {
            compiler: request.compiler.clone(),
            reason: error.to_string(),
        })?,
        timeout,
    )?;
    if !stderr.trim().is_empty() && stdout.trim().is_empty() {
        return Err(CompilerError::InvalidOutput {
            compiler: request.compiler.clone(),
            reason: truncate(stderr.trim(), 500),
        });
    }
    Ok(agent_artifact_from_output(&stdout))
}

fn agent_artifact_from_output(output: &str) -> String {
    let trimmed = output.trim();
    if let Ok(value) = serde_json::from_str::<Value>(trimmed)
        && let Some(artifact) = value.get("artifact").and_then(Value::as_str)
    {
        return artifact.to_string();
    }
    trimmed.into()
}

struct TempClaudePlugin {
    root: PathBuf,
}

impl TempClaudePlugin {
    fn new(skill: &HandoffSkill, skill_source: &str) -> io::Result<Self> {
        let root = env::temp_dir().join(format!(
            "moonbox-claude-skill-{}-{}",
            std::process::id(),
            unique_suffix()
        ));
        let skill_dir = root.join("skills").join(&skill.id);
        fs::create_dir_all(&skill_dir)?;
        fs::create_dir_all(root.join(".claude-plugin"))?;
        fs::write(skill_dir.join("SKILL.md"), skill_source)?;
        fs::write(
            root.join(".claude-plugin").join("plugin.json"),
            format!(
                "{{\"name\":\"{}\",\"version\":\"0.0.0\",\"description\":\"Moonbox temporary handoff skill bridge\"}}\n",
                CLAUDE_PLUGIN_NAME
            ),
        )?;
        Ok(Self { root })
    }
}

impl Drop for TempClaudePlugin {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn unique_suffix() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default()
}

fn sdk_cwd(request: &CapsuleCompileRequest) -> String {
    env::current_dir()
        .ok()
        .unwrap_or_else(|| PathBuf::from(&request.source_session.cwd))
        .to_string_lossy()
        .into_owned()
}

fn run_child_with_input(
    compiler: &str,
    program: &str,
    args: &[String],
    input: Vec<u8>,
    timeout: Duration,
) -> Result<(String, String), CompilerError> {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    configure_agent_child_process_group(&mut command);
    let mut child = command.spawn().map_err(|error| CompilerError::Spawn {
        compiler: compiler.into(),
        program: program.into(),
        reason: error.to_string(),
    })?;
    let stdin = child.stdin.take().ok_or_else(|| CompilerError::Io {
        compiler: compiler.into(),
        stream: "stdin",
        reason: "stdin was not piped".into(),
    })?;
    let stdout = child.stdout.take().ok_or_else(|| CompilerError::Io {
        compiler: compiler.into(),
        stream: "stdout",
        reason: "stdout was not piped".into(),
    })?;
    let stderr = child.stderr.take().ok_or_else(|| CompilerError::Io {
        compiler: compiler.into(),
        stream: "stderr",
        reason: "stderr was not piped".into(),
    })?;
    let writer = write_child_stdin(stdin, input);
    let stdout_reader = read_child_pipe(stdout);
    let stderr_reader = read_child_pipe(stderr);
    let outcome = wait_for_child(&mut child, timeout).map_err(|error| CompilerError::Io {
        compiler: compiler.into(),
        stream: "process",
        reason: error.to_string(),
    })?;
    let stdin_result = join_io_thread(writer, compiler, "stdin")?;
    let stdout = join_io_thread_bytes(stdout_reader, compiler, "stdout")?.map_err(|error| {
        CompilerError::Io {
            compiler: compiler.into(),
            stream: "stdout",
            reason: error.to_string(),
        }
    })?;
    let stderr = join_io_thread_bytes(stderr_reader, compiler, "stderr")?.map_err(|error| {
        CompilerError::Io {
            compiler: compiler.into(),
            stream: "stderr",
            reason: error.to_string(),
        }
    })?;
    let stderr_text = String::from_utf8_lossy(&stderr).trim().to_string();
    match outcome {
        WaitOutcome::TimedOut => Err(CompilerError::Timeout {
            compiler: compiler.into(),
            timeout_ms: duration_millis(timeout),
        }),
        WaitOutcome::Exited(status) if !status.success() => Err(CompilerError::ProcessFailed {
            compiler: compiler.into(),
            status: status_label(status),
            stderr: truncate(&stderr_text, 500),
        }),
        WaitOutcome::Exited(_) => {
            if let Err(error) = stdin_result
                && error.kind() != io::ErrorKind::BrokenPipe
            {
                return Err(CompilerError::Io {
                    compiler: compiler.into(),
                    stream: "stdin",
                    reason: error.to_string(),
                });
            }
            String::from_utf8(stdout)
                .map(|stdout| (stdout, stderr_text))
                .map_err(|error| CompilerError::InvalidOutput {
                    compiler: compiler.into(),
                    reason: error.to_string(),
                })
        }
    }
}

fn runner_command(runner: AgentRunner) -> Option<String> {
    configured_runner_command(runner)
        .or_else(|| {
            python_candidates_for_runner(runner)
                .into_iter()
                .find(|command| {
                    command_available(command)
                        && python_module_available(command, runner.sdk_module())
                })
        })
        .or_else(|| Some("python3".into()))
}

fn configured_runner_command(runner: AgentRunner) -> Option<String> {
    env::var(runner.bin_env())
        .ok()
        .filter(|value| !value.trim().is_empty())
}

fn python_candidates_for_runner(runner: AgentRunner) -> Vec<String> {
    if let Some(configured) = configured_runner_command(runner) {
        return vec![configured];
    }
    python_candidates(runner)
}

fn python_candidates(runner: AgentRunner) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut candidates = Vec::new();
    if let Some(path) = managed_sdk_python_path_from_env(runner) {
        push_python_candidate(&mut candidates, &mut seen, path);
    }
    for command in common_python_candidates() {
        push_python_candidate(&mut candidates, &mut seen, PathBuf::from(command));
    }
    candidates
}

fn push_python_candidate(candidates: &mut Vec<String>, seen: &mut BTreeSet<String>, path: PathBuf) {
    let command = path.to_string_lossy().to_string();
    if seen.insert(command.clone()) && command_available(&command) {
        candidates.push(command);
    }
}

fn common_python_candidates() -> [&'static str; 5] {
    [
        "python3",
        "python",
        "/opt/homebrew/bin/python3",
        "/usr/local/bin/python3",
        "/usr/bin/python3",
    ]
}

fn managed_sdk_python_path_from_env(runner: AgentRunner) -> Option<PathBuf> {
    let home = env::var_os("MOONBOX_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".moonbox")))?;
    Some(managed_sdk_python_path(&home, runner))
}

fn managed_sdk_python_path(moonbox_home: &Path, runner: AgentRunner) -> PathBuf {
    moonbox_home
        .join("venvs")
        .join(match runner {
            AgentRunner::Codex => "codex-sdk",
            AgentRunner::Claude => "claude-sdk",
        })
        .join("bin")
        .join("python")
}

fn managed_sdk_setup_hint(runner: AgentRunner) -> Option<String> {
    let python = common_python_candidates()
        .into_iter()
        .find(|command| command.starts_with("/opt/homebrew/") && command_available(command))
        .or_else(|| {
            common_python_candidates()
                .into_iter()
                .find(|command| command.starts_with("/usr/local/") && command_available(command))
        })
        .or_else(|| {
            common_python_candidates()
                .into_iter()
                .find(|command| command_available(command))
        })?;
    let moonbox_home = env::var_os("MOONBOX_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".moonbox")))?;
    let managed_python = managed_sdk_python_path(&moonbox_home, runner);
    let venv_root = managed_python.parent()?.parent()?;
    Some(format!(
        "{python} -m venv {} && {} -m pip install {}",
        venv_root.display(),
        managed_python.display(),
        runner.sdk_package()
    ))
}
fn runner_python_missing_reason(runner: AgentRunner, command: &str) -> String {
    format!(
        "python_command_not_found: runner={}; cli={}; command={}; env={}",
        runner.label(),
        runner_cli_state(runner),
        command,
        runner.bin_env()
    )
}

fn runner_sdk_missing_reason(runner: AgentRunner, candidates: &[String]) -> String {
    format!(
        "sdk_not_found: runner={}; cli={}; module={}; checked={}; install={}; env={}",
        runner.label(),
        runner_cli_state(runner),
        runner.sdk_module(),
        checked_python_label(candidates),
        sdk_install_hint(runner, candidates),
        runner.bin_env()
    )
}

fn runner_cli_state(runner: AgentRunner) -> String {
    executable_path(runner.cli_command())
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "not_found".into())
}

fn checked_python_label(candidates: &[String]) -> String {
    if candidates.is_empty() {
        "none".into()
    } else {
        candidates.join(",")
    }
}

fn sdk_install_hint(runner: AgentRunner, candidates: &[String]) -> String {
    if let Some(hint) = managed_sdk_setup_hint(runner) {
        return hint;
    }
    let python = candidates
        .iter()
        .find(|command| command.starts_with("/opt/homebrew/"))
        .or_else(|| {
            candidates
                .iter()
                .find(|command| command.starts_with("/usr/local/"))
        })
        .or_else(|| {
            candidates
                .iter()
                .find(|command| command.as_str() != "/usr/bin/python3")
        })
        .or_else(|| candidates.first())
        .map(String::as_str)
        .unwrap_or("python3");
    format!("{python} -m pip install {}", runner.sdk_package())
}

fn executable_path(command: &str) -> Option<PathBuf> {
    let path = Path::new(command);
    if path.components().count() > 1 {
        return command_is_executable(path).then(|| path.to_path_buf());
    }
    env::var_os("PATH").and_then(|paths| {
        env::split_paths(&paths)
            .map(|dir| dir.join(command))
            .find(|path| command_is_executable(path))
    })
}

fn skill_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(paths) = env::var_os("MOONBOX_SKILLS_DIRS") {
        roots.extend(env::split_paths(&paths));
    }
    if let Some(path) = env::var_os("CODEX_HOME").or_else(|| env::var_os("MOONBOX_CODEX_HOME")) {
        roots.push(PathBuf::from(path).join("skills"));
    }
    #[cfg(not(test))]
    if let Some(home) = env::var_os("HOME") {
        roots.push(PathBuf::from(&home).join(".codex").join("skills"));
        roots.push(PathBuf::from(home).join(".agents").join("skills"));
    }
    roots
}

fn discover_skills_under(root: &Path, skills: &mut Vec<HandoffSkill>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.file_name().and_then(|name| name.to_str()) == Some(".system") {
            continue;
        }
        if path.is_dir() {
            let skill_path = path.join("SKILL.md");
            if let Some(skill) = parse_skill_file(&skill_path) {
                skills.push(skill);
            }
        } else if path.file_name().and_then(|name| name.to_str()) == Some("SKILL.md")
            && let Some(skill) = parse_skill_file(&path)
        {
            skills.push(skill);
        }
    }
}

fn parse_skill_file(path: &Path) -> Option<HandoffSkill> {
    let contents = fs::read_to_string(path).ok()?;
    let metadata = parse_frontmatter(&contents);
    let name = metadata
        .name
        .or_else(|| path.parent()?.file_name()?.to_str().map(ToOwned::to_owned))?;
    let description = metadata.description.unwrap_or_default();
    let searchable = format!("{name}\n{description}").to_lowercase();
    if !searchable.contains("handoff") {
        return None;
    }
    Some(HandoffSkill {
        id: sanitize_skill_id(&name),
        name,
        description,
        path: path.to_path_buf(),
    })
}

#[derive(Default)]
struct SkillMetadata {
    name: Option<String>,
    description: Option<String>,
}

fn parse_frontmatter(contents: &str) -> SkillMetadata {
    let mut metadata = SkillMetadata::default();
    let Some(rest) = contents.strip_prefix("---") else {
        return metadata;
    };
    let Some(frontmatter) = rest.split("\n---").next() else {
        return metadata;
    };
    for line in frontmatter.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = trim_yaml_scalar(value);
        match key.trim() {
            "name" => metadata.name = Some(value),
            "description" => metadata.description = Some(value),
            _ => {}
        }
    }
    metadata
}

fn trim_yaml_scalar(value: &str) -> String {
    value
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .trim()
        .into()
}

fn sanitize_skill_id(name: &str) -> String {
    let mut id = String::new();
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            id.push(ch.to_ascii_lowercase());
        } else if (ch == '-' || ch == '_' || ch.is_whitespace()) && !id.ends_with('-') {
            id.push('-');
        }
    }
    id.trim_matches('-').to_string()
}

fn codex_auth_reason() -> Option<String> {
    if env_non_empty("OPENAI_API_KEY") || env_non_empty("OPENAI_API_KEY_FILE") {
        return None;
    }
    if let Some(home) = env::var_os("CODEX_HOME").or_else(|| env::var_os("MOONBOX_CODEX_HOME"))
        && PathBuf::from(home).join("auth.json").is_file()
    {
        return None;
    }
    #[cfg(not(test))]
    if let Some(home) = env::var_os("HOME")
        && PathBuf::from(home)
            .join(".codex")
            .join("auth.json")
            .is_file()
    {
        return None;
    }
    Some("auth_required: Codex SDK needs OPENAI_API_KEY or a Codex auth cache".into())
}

fn claude_auth_reason() -> Option<String> {
    if env_non_empty("ANTHROPIC_API_KEY") || env_non_empty("ANTHROPIC_AUTH_TOKEN") {
        return None;
    }
    if env_non_empty("CLAUDE_CODE_USE_BEDROCK")
        && (env_non_empty("AWS_PROFILE")
            || env_non_empty("AWS_ACCESS_KEY_ID")
            || env_non_empty("AWS_WEB_IDENTITY_TOKEN_FILE"))
    {
        return None;
    }
    if env_non_empty("CLAUDE_CODE_USE_VERTEX")
        && (env_non_empty("GOOGLE_APPLICATION_CREDENTIALS")
            || env_non_empty("CLOUDSDK_CONFIG")
            || env_non_empty("GOOGLE_CLOUD_PROJECT"))
    {
        return None;
    }
    Some(
        "auth_required: Claude runner needs ANTHROPIC_API_KEY or supported provider credentials"
            .into(),
    )
}

fn env_non_empty(name: &str) -> bool {
    env::var(name)
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
}

fn command_available(command: &str) -> bool {
    let path = Path::new(command);
    if path.components().count() > 1 {
        return command_is_executable(path);
    }
    env::var_os("PATH")
        .map(|paths| env::split_paths(&paths).any(|dir| command_is_executable(&dir.join(command))))
        .unwrap_or(false)
}

fn command_is_executable(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata()
            .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn python_module_available(python: &str, module: &str) -> bool {
    let key = (python.to_string(), module.to_string());
    let cache = PYTHON_MODULE_CACHE.get_or_init(|| Mutex::new(BTreeMap::new()));
    if let Ok(cache) = cache.lock()
        && cache.get(&key).copied().unwrap_or(false)
    {
        return true;
    }

    let available = python_module_available_uncached(python, module);
    if let Ok(mut cache) = cache.lock() {
        if available {
            cache.insert(key, true);
        } else {
            cache.remove(&key);
        }
    }
    available
}

fn python_module_available_uncached(python: &str, module: &str) -> bool {
    let mut child = match Command::new(python)
        .arg("-c")
        .arg(format!("import {module}"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return false,
    };
    let deadline = Instant::now() + Duration::from_millis(900);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) if Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(20));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
            Err(_) => return false,
        }
    }
}

fn agent_timeout() -> Result<Duration, CompilerError> {
    let timeout = configured_agent_timeout_ms_raw().unwrap_or(DEFAULT_TIMEOUT_MS);
    if timeout == 0 {
        return Err(CompilerError::InvalidConfig {
            name: "MOONBOX_AGENT_HANDOFF_TIMEOUT_MS".into(),
            reason: "timeout must be greater than zero".into(),
        });
    }
    Ok(Duration::from_millis(timeout))
}

pub(crate) fn configured_agent_timeout_ms() -> u64 {
    configured_agent_timeout_ms_raw()
        .filter(|timeout| *timeout > 0)
        .unwrap_or(DEFAULT_TIMEOUT_MS)
}

fn configured_agent_timeout_ms_raw() -> Option<u64> {
    env::var("MOONBOX_AGENT_HANDOFF_TIMEOUT_MS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
}

enum WaitOutcome {
    Exited(ExitStatus),
    TimedOut,
}

fn wait_for_child(child: &mut Child, timeout: Duration) -> io::Result<WaitOutcome> {
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            cleanup_agent_child_process_group(child.id());
            return Ok(WaitOutcome::Exited(status));
        }
        if started.elapsed() >= timeout {
            terminate_agent_child_process_group(child.id());
            thread::sleep(Duration::from_millis(AGENT_CHILD_TERM_GRACE_MS));
            kill_agent_child_process_group(child.id());
            let _ = child.wait();
            return Ok(WaitOutcome::TimedOut);
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn configure_agent_child_process_group(command: &mut Command) {
    #[cfg(unix)]
    {
        command.process_group(0);
    }
}

fn cleanup_agent_child_process_group(pid: u32) {
    terminate_agent_child_process_group(pid);
    thread::sleep(Duration::from_millis(AGENT_CHILD_TERM_GRACE_MS));
    kill_agent_child_process_group(pid);
}

fn terminate_agent_child_process_group(pid: u32) {
    signal_agent_child_process_group(pid, "TERM");
}

fn kill_agent_child_process_group(pid: u32) {
    signal_agent_child_process_group(pid, "KILL");
}

#[cfg(unix)]
fn signal_agent_child_process_group(pid: u32, signal: &str) {
    let group = format!("-{pid}");
    let _ = Command::new("/bin/kill")
        .arg(format!("-{signal}"))
        .arg(group)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[cfg(not(unix))]
fn signal_agent_child_process_group(_pid: u32, _signal: &str) {}

fn write_child_stdin(
    mut stdin: impl Write + Send + 'static,
    input: Vec<u8>,
) -> thread::JoinHandle<io::Result<()>> {
    thread::spawn(move || {
        stdin.write_all(&input)?;
        stdin.write_all(b"\n")?;
        Ok(())
    })
}

fn read_child_pipe(
    mut pipe: impl Read + Send + 'static,
) -> thread::JoinHandle<io::Result<Vec<u8>>> {
    thread::spawn(move || {
        let mut output = Vec::new();
        pipe.read_to_end(&mut output)?;
        Ok(output)
    })
}

fn join_io_thread(
    handle: thread::JoinHandle<io::Result<()>>,
    compiler: &str,
    stream: &'static str,
) -> Result<io::Result<()>, CompilerError> {
    handle.join().map_err(|_| CompilerError::Io {
        compiler: compiler.into(),
        stream,
        reason: "thread panicked".into(),
    })
}

fn join_io_thread_bytes(
    handle: thread::JoinHandle<io::Result<Vec<u8>>>,
    compiler: &str,
    stream: &'static str,
) -> Result<io::Result<Vec<u8>>, CompilerError> {
    handle.join().map_err(|_| CompilerError::Io {
        compiler: compiler.into(),
        stream,
        reason: "thread panicked".into(),
    })
}

fn status_label(status: ExitStatus) -> String {
    status
        .code()
        .map(|code| format!("exit {code}"))
        .unwrap_or_else(|| "terminated by signal".into())
}

fn duration_millis(duration: Duration) -> u64 {
    duration.as_millis().try_into().unwrap_or(u64::MAX)
}

fn bounded_artifact(value: &str) -> String {
    truncate(value.trim(), MAX_ARTIFACT_CHARS)
}

fn single_line_excerpt(value: &str, limit: usize) -> String {
    truncate(
        &value.split_whitespace().collect::<Vec<_>>().join(" "),
        limit,
    )
}

fn truncate(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let truncated = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        format!("{truncated}...")
    } else {
        truncated
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{
        data,
        model::{
            CliTool, TimelineAttachment, TimelineEventMetadata, TimelineFileChange,
            TimelineToolCall, TimelineToolResult,
        },
    };

    #[test]
    fn parses_agent_compiler_id() {
        let spec = parse_compiler_id("agent:codex:handoff").expect("spec");
        assert_eq!(spec.runner, AgentRunner::Codex);
        assert_eq!(spec.skill_id, "handoff");
        assert!(parse_compiler_id("engineering-handoff").is_none());
    }

    #[test]
    fn managed_sdk_python_paths_are_runner_specific() {
        let home = PathBuf::from("/moonbox-home");

        assert_eq!(
            managed_sdk_python_path(&home, AgentRunner::Codex),
            PathBuf::from("/moonbox-home/venvs/codex-sdk/bin/python")
        );
        assert_eq!(
            managed_sdk_python_path(&home, AgentRunner::Claude),
            PathBuf::from("/moonbox-home/venvs/claude-sdk/bin/python")
        );
    }

    #[test]
    #[cfg(unix)]
    fn python_module_negative_result_is_not_sticky() {
        use std::os::unix::fs::PermissionsExt;

        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let root = env::temp_dir().join(format!(
            "moonbox-python-module-cache-{}-{nonce}",
            std::process::id()
        ));
        let marker = root.join("module-ready");
        let python = root.join("python");
        fs::create_dir_all(&root).expect("test dir");
        let marker_literal = marker.to_string_lossy().replace('\'', "'\\''");
        fs::write(
            &python,
            format!("#!/bin/sh\nif [ -f '{marker_literal}' ]; then exit 0; fi\nexit 1\n"),
        )
        .expect("fake python");
        let mut permissions = fs::metadata(&python).expect("metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&python, permissions).expect("permissions");

        let command = python.to_string_lossy().to_string();
        assert!(!python_module_available(&command, "openai_codex"));

        fs::write(&marker, "ready").expect("marker");
        let deadline = Instant::now() + Duration::from_secs(5);
        while !python_module_available(&command, "openai_codex") && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(20));
        }
        assert!(python_module_available(&command, "openai_codex"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    #[cfg(unix)]
    fn agent_child_success_reaps_sdk_helper_process_group() {
        let root = env::temp_dir().join(format!(
            "moonbox-agent-child-group-{}-{}",
            std::process::id(),
            unique_suffix()
        ));
        fs::create_dir_all(&root).expect("test dir");
        let pid_file = root.join("helper.pid");
        let pid_file_literal = pid_file.to_string_lossy().replace('\'', "'\\''");
        let script = executable_script(
            &root,
            "agent-child-group",
            &format!(
                "#!/bin/sh\nsleep 30 &\necho $! > '{pid_file_literal}'\nprintf '{{\"artifact\":\"ok\"}}\\n'\n"
            ),
        );

        let (stdout, _stderr) = run_child_with_input(
            "agent:codex:handoff",
            &script.to_string_lossy(),
            &[],
            b"{}".to_vec(),
            Duration::from_secs(5),
        )
        .expect("success");

        assert!(stdout.contains("\"artifact\":\"ok\""));
        let pid = fs::read_to_string(&pid_file)
            .expect("pid file")
            .trim()
            .parse::<u32>()
            .expect("pid");
        let deadline = Instant::now() + Duration::from_secs(2);
        while process_exists(pid) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(20));
        }
        assert!(
            !process_exists(pid),
            "helper process {pid} survived timeout"
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    #[cfg(unix)]
    fn agent_child_success_ignores_closed_stdin_pipe() {
        let root = env::temp_dir().join(format!(
            "moonbox-agent-closed-stdin-{}-{}",
            std::process::id(),
            unique_suffix()
        ));
        fs::create_dir_all(&root).expect("test dir");
        let script = executable_script(
            &root,
            "agent-closed-stdin",
            "#!/bin/sh\nexec 0<&-\nprintf '{\"artifact\":\"ok\"}\\n'\n",
        );

        let (stdout, _stderr) = run_child_with_input(
            "agent:codex:handoff",
            &script.to_string_lossy(),
            &[],
            vec![b'x'; 1024 * 1024],
            Duration::from_secs(5),
        )
        .expect("success");

        assert!(stdout.contains("\"artifact\":\"ok\""));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn discovers_generic_handoff_skill_from_explicit_root() {
        let root = env::temp_dir().join(format!("moonbox-handoff-skill-{}", std::process::id()));
        let skill_dir = root.join("handoff");
        fs::create_dir_all(&skill_dir).expect("skill dir");
        fs::write(
            skill_dir.join("SKILL.md"),
            r#"---
name: handoff
description: Compact the current conversation into a handoff document.
---
Write the handoff.
"#,
        )
        .expect("skill");
        let mut skills = Vec::new();
        discover_skills_under(&root, &mut skills);
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].id, "handoff");
        assert!(skills[0].description.contains("handoff"));
    }

    #[test]
    fn agent_prompt_uses_bounded_context_pack_and_skill_source() {
        let request =
            data::compile_request(CliTool::Codex, CliTool::Claude, "evt-091").expect("request");
        let skill = HandoffSkill {
            id: "handoff".into(),
            name: "handoff".into(),
            description: "handoff".into(),
            path: PathBuf::from("/skills/handoff/SKILL.md"),
        };
        let prompt = build_agent_prompt(&request, &skill, "Write a handoff document.");
        assert!(prompt.contains("selected community handoff skill"));
        assert!(prompt.contains("Moonbox Handoff Context Pack"));
        assert!(prompt.contains("Source session stores are read-only"));
        assert!(prompt.contains("evt-091"));
    }

    #[test]
    fn context_pack_bounds_large_timeline_and_surfaces_metadata() {
        let mut request =
            data::compile_request(CliTool::Codex, CliTool::Claude, "evt-091").expect("request");
        request.rewind_event_id = "evt-129".into();
        request.timeline.events = (0..130)
            .map(|index| TimelineEvent {
                id: format!("evt-{index:03}"),
                time: format!("2026-06-12T10:{:02}:00Z", index % 60),
                kind: if index == 100 {
                    TimelineKind::Compact
                } else if index == 120 {
                    TimelineKind::Tool
                } else {
                    TimelineKind::Assistant
                },
                title: format!("event title {index:03}"),
                detail: format!("event detail {index:03}"),
                metadata: TimelineEventMetadata::default(),
            })
            .collect();
        let tool_event = request
            .timeline
            .events
            .iter_mut()
            .find(|event| event.id == "evt-120")
            .expect("tool event");
        tool_event.metadata.tool_calls = vec![TimelineToolCall {
            id: Some("call-1".into()),
            name: Some("Edit".into()),
            arguments: Some(json!({"file_path":"src/core/handoff.rs"})),
            raw: None,
        }];
        tool_event.metadata.tool_results = vec![TimelineToolResult {
            call_id: Some("call-1".into()),
            name: Some("Edit".into()),
            content: Some("patched handoff context pack".into()),
            is_error: Some(false),
            raw: None,
        }];
        tool_event.metadata.file_changes = vec![TimelineFileChange {
            path: Some("src/core/handoff.rs".into()),
            operation: Some("modify".into()),
            summary: Some("add context pack evidence sections".into()),
            diff: Some("@@ context pack @@".into()),
            raw: None,
        }];
        tool_event.metadata.attachments = vec![TimelineAttachment {
            id: Some("att-1".into()),
            name: Some("screenshot.png".into()),
            path: Some("/tmp/screenshot.png".into()),
            mime_type: Some("image/png".into()),
            size_bytes: Some(1234),
            raw: None,
        }];

        let context = context_pack_markdown(&request);

        assert!(context.contains("- Timeline events through rewind: 130"));
        assert!(context.contains("- Included recent events: 80"));
        assert!(context.contains("- Omitted older events: 50"));
        assert!(context.contains("## Compact Frontier"));
        assert!(context.contains("[evt-100] event title 100"));
        assert!(context.contains("call Edit id=call-1"));
        assert!(context.contains("patched handoff context pack"));
        assert!(context.contains("modify src/core/handoff.rs"));
        assert!(context.contains("attachment id=att-1"));
        assert!(context.contains("[evt-050]"));
        assert!(!context.contains("[evt-049]"));
    }

    #[test]
    fn normalizes_agent_artifact_into_legacy_compatible_capsule() {
        let request =
            data::compile_request(CliTool::Codex, CliTool::Claude, "evt-091").expect("request");
        let skill = HandoffSkill {
            id: "handoff".into(),
            name: "handoff".into(),
            description: "handoff".into(),
            path: PathBuf::from("/skills/handoff/SKILL.md"),
        };
        let capsule = normalize_agent_artifact(
            &request,
            AgentRunner::Codex,
            &skill,
            "# Handoff\nContinue the work.",
        );
        assert_eq!(capsule.compiler, request.compiler);
        assert_eq!(capsule.state, "handoff_ready");
        assert_eq!(capsule.handoff_runner.as_deref(), Some("Codex"));
        assert_eq!(capsule.handoff_skill.as_deref(), Some("handoff"));
        assert!(
            capsule
                .handoff_artifact
                .as_deref()
                .expect("artifact")
                .contains("Continue the work")
        );
    }

    #[test]
    fn extracts_structured_agent_artifact_json() {
        let artifact = agent_artifact_from_output(
            r##"{"artifact":"# Handoff\nContinue here.","warnings":[]}"##,
        );

        assert_eq!(artifact, "# Handoff\nContinue here.");
    }

    #[test]
    fn validates_agent_artifact_is_not_empty() {
        let request =
            data::compile_request(CliTool::Codex, CliTool::Claude, "evt-091").expect("request");

        let error = validate_agent_artifact(&request, " \n\t ".into()).expect_err("empty artifact");

        assert!(error.to_string().contains("empty handoff artifact"));
    }

    #[cfg(unix)]
    fn executable_script(root: &Path, name: &str, contents: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;

        let path = root.join(name);
        fs::write(&path, contents).expect("script");
        let mut permissions = fs::metadata(&path).expect("metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions).expect("permissions");
        path
    }

    #[cfg(unix)]
    fn process_exists(pid: u32) -> bool {
        Command::new("/bin/kill")
            .arg("-0")
            .arg(pid.to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }

    #[test]
    fn codex_bridge_uses_official_sdk_and_read_only_sandbox() {
        assert!(CODEX_PYTHON_BRIDGE.contains("from openai_codex import Codex, Sandbox"));
        assert!(CODEX_PYTHON_BRIDGE.contains("Sandbox.read_only"));
        assert!(CODEX_PYTHON_BRIDGE.contains("thread.run(prompt"));
        assert!(CODEX_PYTHON_BRIDGE.contains("final_response"));
    }

    #[test]
    fn claude_bridge_filters_to_selected_plugin_skill() {
        assert!(CLAUDE_PYTHON_BRIDGE.contains("skills=[plugin_skill]"));
        assert!(CLAUDE_PYTHON_BRIDGE.contains("allowed_tools=[\"Skill\"]"));
        assert!(CLAUDE_PYTHON_BRIDGE.contains("permission_mode=\"dontAsk\""));
    }

    #[test]
    fn temp_claude_plugin_bridges_selected_skill_without_touching_home() {
        let skill = HandoffSkill {
            id: "handoff".into(),
            name: "handoff".into(),
            description: "handoff".into(),
            path: PathBuf::from("/source/skills/handoff/SKILL.md"),
        };
        let plugin = TempClaudePlugin::new(&skill, "# Handoff\nWrite it.").expect("plugin");
        let root = plugin.root.clone();

        assert!(root.join(".claude-plugin").join("plugin.json").is_file());
        let copied = fs::read_to_string(root.join("skills/handoff/SKILL.md")).expect("skill");
        assert!(copied.contains("Write it."));
        drop(plugin);
        assert!(!root.exists());
    }
}
