use std::{
    env, fs, io,
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use serde_json::{Value, json};

use super::{
    compiler::{self, CompilerError},
    model::{
        CapsuleCompileOutput, CapsuleCompileRequest, ChecklistItem, CompilerPresetStatus,
        WorkCapsule,
    },
};

const COMPILER_PREFIX: &str = "agent";
const DEFAULT_TIMEOUT_MS: u64 = 180_000;
const MAX_ARTIFACT_CHARS: usize = 40_000;
const CLAUDE_PLUGIN_NAME: &str = "moonbox-handoff";
const CLAUDE_PYTHON_BRIDGE: &str = r#"
import asyncio
import json
import sys

from claude_agent_sdk import ClaudeAgentOptions, query


payload = json.load(sys.stdin)


async def main():
    prompt = f"/{payload['plugin_name']}:{payload['skill_id']}\n\n{payload['prompt']}"
    options = ClaudeAgentOptions(
        cwd=payload.get("cwd"),
        setting_sources=[],
        plugins=[{"type": "local", "path": payload["plugin_path"]}],
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
            Self::Codex => "MOONBOX_CODEX_BIN",
            Self::Claude => "MOONBOX_CLAUDE_AGENT_SDK_PYTHON",
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
    let Some(command) = runner_command(runner) else {
        return RunnerPreflight {
            status: CompilerPresetStatus::Warning,
            reason: format!("{} SDK runner not installed or not on PATH", runner.label()),
            command: None,
        };
    };
    if !command_available(&command) {
        return RunnerPreflight {
            status: CompilerPresetStatus::Warning,
            reason: format!(
                "{} SDK runner command was not found: {command}",
                runner.label()
            ),
            command: Some(command),
        };
    }
    if runner == AgentRunner::Claude && !python_module_available(&command, "claude_agent_sdk") {
        return RunnerPreflight {
            status: CompilerPresetStatus::Warning,
            reason:
                "not_installed: install the Claude Agent SDK with `pip install claude-agent-sdk`"
                    .into(),
            command: Some(command),
        };
    }
    let auth_reason = match runner {
        AgentRunner::Codex => codex_auth_reason(),
        AgentRunner::Claude => claude_auth_reason(),
    };
    if let Some(reason) = auth_reason {
        return RunnerPreflight {
            status: CompilerPresetStatus::Warning,
            reason,
            command: Some(command),
        };
    }
    RunnerPreflight {
        status: CompilerPresetStatus::Ready,
        reason: format!(
            "{} SDK runner is installed and auth preflight passed",
            runner.label()
        ),
        command: Some(command),
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
        String::new(),
        "## Redaction".into(),
        format!("- Policy: {}", request.redaction.policy),
        format!("- Secrets redacted: {}", request.redaction.secrets_redacted),
        format!("- Paths redacted: {}", request.redaction.paths_redacted),
        format!(
            "- Prompt-injection warnings: {}",
            request.redaction.prompt_injection_warnings
        ),
        String::new(),
        "## Timeline Through Rewind".into(),
    ];
    for event in events_through_rewind(request) {
        lines.push(format!(
            "- [{}] {:?}: {}",
            event.id,
            event.kind,
            single_line_excerpt(&event.detail, 480)
        ));
    }
    lines.join("\n")
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

fn run_agent(
    request: &CapsuleCompileRequest,
    runner: AgentRunner,
    skill: &HandoffSkill,
    skill_source: &str,
    prompt: &str,
) -> Result<String, CompilerError> {
    match runner {
        AgentRunner::Codex => run_codex_app_server(request, skill, prompt),
        AgentRunner::Claude => run_claude_agent_sdk(request, skill, skill_source, prompt),
    }
}

fn run_codex_app_server(
    request: &CapsuleCompileRequest,
    skill: &HandoffSkill,
    prompt: &str,
) -> Result<String, CompilerError> {
    let timeout = agent_timeout()?;
    let program =
        runner_command(AgentRunner::Codex).ok_or_else(|| CompilerError::InvalidConfig {
            name: request.compiler.clone(),
            reason: "Codex app-server command is not configured".into(),
        })?;
    let mut child = Command::new(&program)
        .arg("app-server")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| CompilerError::Spawn {
            compiler: request.compiler.clone(),
            program: program.clone(),
            reason: error.to_string(),
        })?;
    let mut stdin = child.stdin.take().ok_or_else(|| CompilerError::Io {
        compiler: request.compiler.clone(),
        stream: "stdin",
        reason: "stdin was not piped".into(),
    })?;
    let stdout = child.stdout.take().ok_or_else(|| CompilerError::Io {
        compiler: request.compiler.clone(),
        stream: "stdout",
        reason: "stdout was not piped".into(),
    })?;
    let stderr = child.stderr.take().ok_or_else(|| CompilerError::Io {
        compiler: request.compiler.clone(),
        stream: "stderr",
        reason: "stderr was not piped".into(),
    })?;
    let (line_sender, line_receiver) = mpsc::channel::<io::Result<String>>();
    let stdout_reader = thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            if line_sender.send(line).is_err() {
                break;
            }
        }
    });
    let stderr_reader = read_child_pipe(stderr);
    send_rpc(
        &mut stdin,
        json!({
            "id": 1,
            "method": "initialize",
            "params": {
                "clientInfo": {
                    "name": "moonbox",
                    "title": "Moonbox",
                    "version": env!("CARGO_PKG_VERSION")
                }
            }
        }),
    )
    .map_err(|error| CompilerError::Io {
        compiler: request.compiler.clone(),
        stream: "stdin",
        reason: error.to_string(),
    })?;
    send_rpc(
        &mut stdin,
        json!({
            "method": "initialized",
            "params": {}
        }),
    )
    .map_err(|error| CompilerError::Io {
        compiler: request.compiler.clone(),
        stream: "stdin",
        reason: error.to_string(),
    })?;
    send_rpc(
        &mut stdin,
        json!({
            "id": 2,
            "method": "thread/start",
            "params": {
                "cwd": sdk_cwd(request),
                "approvalPolicy": "never",
                "sandbox": "readOnly",
                "serviceName": "moonbox_handoff"
            }
        }),
    )
    .map_err(|error| CompilerError::Io {
        compiler: request.compiler.clone(),
        stream: "stdin",
        reason: error.to_string(),
    })?;

    let started = Instant::now();
    let mut thread_id = None;
    let mut turn_started = false;
    let mut artifact = String::new();
    let mut completed = false;

    while started.elapsed() < timeout {
        let remaining = timeout.saturating_sub(started.elapsed());
        let line = match line_receiver.recv_timeout(remaining.min(Duration::from_millis(250))) {
            Ok(Ok(line)) => line,
            Ok(Err(error)) => {
                return Err(CompilerError::Io {
                    compiler: request.compiler.clone(),
                    stream: "stdout",
                    reason: error.to_string(),
                });
            }
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };
        if line.trim().is_empty() {
            continue;
        }
        let message =
            serde_json::from_str::<Value>(&line).map_err(|error| CompilerError::InvalidOutput {
                compiler: request.compiler.clone(),
                reason: format!("invalid Codex app-server JSON: {error}; line={line}"),
            })?;
        if message.get("id").and_then(Value::as_i64) == Some(2) {
            thread_id = message
                .pointer("/result/thread/id")
                .and_then(Value::as_str)
                .map(str::to_owned);
            if let Some(id) = &thread_id {
                send_rpc(
                    &mut stdin,
                    json!({
                        "id": 3,
                        "method": "turn/start",
                        "params": {
                            "threadId": id,
                            "input": [
                                {
                                    "type": "text",
                                    "text": format!("${}\n\n{}", skill.name, prompt)
                                },
                                {
                                    "type": "skill",
                                    "name": skill.name,
                                    "path": skill.path.to_string_lossy()
                                }
                            ]
                        }
                    }),
                )
                .map_err(|error| CompilerError::Io {
                    compiler: request.compiler.clone(),
                    stream: "stdin",
                    reason: error.to_string(),
                })?;
                turn_started = true;
            }
            continue;
        }
        if let Some(error) = message.get("error") {
            let _ = child.kill();
            let _ = child.wait();
            return Err(CompilerError::InvalidOutput {
                compiler: request.compiler.clone(),
                reason: format!(
                    "Codex app-server error: {}",
                    single_line_excerpt(&error.to_string(), 500)
                ),
            });
        }
        collect_codex_text(&message, &mut artifact);
        if message.get("method").and_then(Value::as_str) == Some("turn/completed") {
            completed = true;
            break;
        }
    }

    if !completed {
        let _ = child.kill();
        let _ = child.wait();
        return Err(CompilerError::Timeout {
            compiler: request.compiler.clone(),
            timeout_ms: duration_millis(timeout),
        });
    }
    let _ = child.kill();
    let _ = child.wait();
    let _ = stdout_reader.join();
    let stderr =
        join_io_thread_bytes(stderr_reader, &request.compiler, "stderr")?.map_err(|error| {
            CompilerError::Io {
                compiler: request.compiler.clone(),
                stream: "stderr",
                reason: error.to_string(),
            }
        })?;
    if !turn_started || thread_id.is_none() {
        return Err(CompilerError::InvalidOutput {
            compiler: request.compiler.clone(),
            reason: "Codex app-server did not start a thread/turn".into(),
        });
    }
    if artifact.trim().is_empty() {
        let stderr = String::from_utf8_lossy(&stderr);
        return Err(CompilerError::InvalidOutput {
            compiler: request.compiler.clone(),
            reason: format!(
                "Codex app-server completed without agent text{}",
                if stderr.trim().is_empty() {
                    String::new()
                } else {
                    format!("; stderr: {}", truncate(stderr.trim(), 500))
                }
            ),
        });
    }
    Ok(agent_artifact_from_output(&artifact))
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

fn send_rpc(stdin: &mut impl Write, message: Value) -> io::Result<()> {
    serde_json::to_writer(&mut *stdin, &message)?;
    stdin.write_all(b"\n")?;
    stdin.flush()
}

fn collect_codex_text(message: &Value, output: &mut String) {
    let Some(method) = message.get("method").and_then(Value::as_str) else {
        return;
    };
    if !method.contains("agentMessage") && method != "item/completed" {
        return;
    }
    let mut parts = Vec::new();
    collect_text_fields(message.get("params").unwrap_or(message), &mut parts);
    if !parts.is_empty() {
        if !output.is_empty() {
            output.push('\n');
        }
        output.push_str(&parts.join("\n"));
    }
}

fn collect_text_fields(value: &Value, parts: &mut Vec<String>) {
    match value {
        Value::String(text) => {
            if !text.trim().is_empty() {
                parts.push(text.clone());
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_text_fields(item, parts);
            }
        }
        Value::Object(object) => {
            for key in ["delta", "text", "content", "message", "result", "item"] {
                if let Some(value) = object.get(key) {
                    collect_text_fields(value, parts);
                }
            }
        }
        _ => {}
    }
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
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| CompilerError::Spawn {
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
    join_io_thread(writer, compiler, "stdin")?.map_err(|error| CompilerError::Io {
        compiler: compiler.into(),
        stream: "stdin",
        reason: error.to_string(),
    })?;
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
        WaitOutcome::Exited(_) => String::from_utf8(stdout)
            .map(|stdout| (stdout, stderr_text))
            .map_err(|error| CompilerError::InvalidOutput {
                compiler: compiler.into(),
                reason: error.to_string(),
            }),
    }
}

fn runner_command(runner: AgentRunner) -> Option<String> {
    env::var(runner.bin_env())
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            Some(match runner {
                AgentRunner::Codex => "codex".into(),
                AgentRunner::Claude => "python3".into(),
            })
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
    Some("auth_required: Codex auth cache was not found".into())
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
    let timeout = env::var("MOONBOX_AGENT_HANDOFF_TIMEOUT_MS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_TIMEOUT_MS);
    if timeout == 0 {
        return Err(CompilerError::InvalidConfig {
            name: "MOONBOX_AGENT_HANDOFF_TIMEOUT_MS".into(),
            reason: "timeout must be greater than zero".into(),
        });
    }
    Ok(Duration::from_millis(timeout))
}

enum WaitOutcome {
    Exited(ExitStatus),
    TimedOut,
}

fn wait_for_child(child: &mut Child, timeout: Duration) -> io::Result<WaitOutcome> {
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(WaitOutcome::Exited(status));
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(WaitOutcome::TimedOut);
        }
        thread::sleep(Duration::from_millis(20));
    }
}

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
    use crate::core::{data, model::CliTool};

    #[test]
    fn parses_agent_compiler_id() {
        let spec = parse_compiler_id("agent:codex:handoff").expect("spec");
        assert_eq!(spec.runner, AgentRunner::Codex);
        assert_eq!(spec.skill_id, "handoff");
        assert!(parse_compiler_id("engineering-handoff").is_none());
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
    fn collects_codex_app_server_agent_text() {
        let mut output = String::new();
        collect_codex_text(
            &json!({
                "method": "item/agentMessage/delta",
                "params": {
                    "delta": "# Handoff",
                }
            }),
            &mut output,
        );
        collect_codex_text(
            &json!({
                "method": "item/completed",
                "params": {
                    "item": {
                        "type": "message",
                        "content": [
                            {"type": "output_text", "text": "Continue safely."}
                        ]
                    }
                }
            }),
            &mut output,
        );

        assert!(output.contains("# Handoff"));
        assert!(output.contains("Continue safely."));
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
