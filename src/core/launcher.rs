use std::{
    env,
    path::{Path, PathBuf},
    process::{Command, ExitStatus},
};

use super::{
    compiler,
    error::CoreError,
    model::{
        ChecklistItem, CliTool, ContinuationProtocol, LaunchExecution, LaunchExecutionStatus,
        LaunchPlan, OriginalSessionExecution, OriginalSessionPlan, SessionSummary,
        SourceProvenance, TargetLaunchCommand, WorkCapsule,
    },
    redaction, verifier,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetInputPreview {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<String>,
    pub prompt: String,
}

#[cfg(test)]
pub fn target_command(
    target: CliTool,
    session: &SessionSummary,
    capsule: &WorkCapsule,
) -> Result<TargetLaunchCommand, CoreError> {
    target_command_with_continuation(target, session, capsule, &ContinuationProtocol::default())
}

pub fn target_command_with_continuation(
    target: CliTool,
    session: &SessionSummary,
    capsule: &WorkCapsule,
    continuation: &ContinuationProtocol,
) -> Result<TargetLaunchCommand, CoreError> {
    let program = configured_target_binary(target);
    let prompt = handoff_prompt(session, capsule, continuation);
    let cwd = usable_cwd(&session.cwd);
    let args = target_args(target, cwd.as_deref(), prompt);
    let display = shell_command(&program, &args);

    Ok(TargetLaunchCommand {
        program,
        args,
        cwd,
        display,
    })
}

pub fn target_prompt_preview_with_continuation(
    session: &SessionSummary,
    capsule: &WorkCapsule,
    continuation: &ContinuationProtocol,
) -> String {
    handoff_prompt(session, capsule, continuation)
}

pub fn execute_plan(mut plan: LaunchPlan, allow_draft: bool) -> Result<LaunchExecution, CoreError> {
    if !plan.verification.ready {
        return Err(CoreError::LaunchBlocked {
            reason: format!("verification status {}", plan.verification.status),
        });
    }
    if let Some(reason) = draft_compiler_execute_blocker(&plan, allow_draft) {
        return Err(CoreError::LaunchBlocked { reason });
    }
    if let Some(reason) = verifier::execution_command_blocker(&plan.target_command) {
        return Err(CoreError::LaunchBlocked { reason });
    }

    plan.dry_run = false;
    let status = run_target_command(&plan.target_command)?;
    Ok(LaunchExecution {
        version: 1,
        status: if status.success() {
            LaunchExecutionStatus::Success
        } else {
            LaunchExecutionStatus::Failed
        },
        exit_code: status.code(),
        plan,
        launch_ledger: None,
        launch_ledger_warning: None,
    })
}

fn draft_compiler_execute_blocker(plan: &LaunchPlan, allow_draft: bool) -> Option<String> {
    if allow_draft
        || !compiler::compiler_is_builtin(&plan.compiler)
        || plan.source_session.source_provenance == SourceProvenance::Fixture
    {
        return None;
    }

    Some(format!(
        "{} is a built-in draft compiler for a non-fixture session; pass --allow-draft to execute or configure an external compiler skill",
        plan.compiler
    ))
}

pub fn original_command(session: &SessionSummary) -> TargetLaunchCommand {
    let program = configured_target_binary(session.cli);
    let cwd = usable_cwd(&session.cwd);
    let args = original_args(session, cwd.as_deref());
    let display = shell_command(&program, &args);

    TargetLaunchCommand {
        program,
        args,
        cwd,
        display,
    }
}

pub fn execute_original_plan(
    mut plan: OriginalSessionPlan,
) -> Result<OriginalSessionExecution, CoreError> {
    plan.dry_run = false;
    let status = run_target_command(&plan.command)?;
    Ok(OriginalSessionExecution {
        version: 1,
        status: if status.success() {
            LaunchExecutionStatus::Success
        } else {
            LaunchExecutionStatus::Failed
        },
        exit_code: status.code(),
        plan,
        launch_ledger: None,
        launch_ledger_warning: None,
    })
}

pub(crate) fn handoff_original_plan(mut plan: OriginalSessionPlan) -> Result<(), CoreError> {
    plan.dry_run = false;
    exec_interactive_command(&plan.command)
}

pub(crate) fn run_original_interactive(
    mut plan: OriginalSessionPlan,
) -> Result<ExitStatus, CoreError> {
    plan.dry_run = false;
    run_target_command(&plan.command)
}

pub(crate) fn original_handoff_notice(plan: &OriginalSessionPlan) -> String {
    let cwd = plan.command.cwd.as_deref().unwrap_or("terminal default");
    format!(
        "Opening original session: {} {}\nCwd: {}\nCommand: {}\n",
        plan.source_session.cli, plan.source_session.id, cwd, plan.command.display
    )
}

pub(crate) fn target_handoff_notice(plan: &LaunchPlan) -> String {
    format!(
        "Starting local target: {}\nSource: {} {}\nRewind: {}\nCommand: {}\nExit the target CLI to return to Moonbox.\n",
        plan.target_cli,
        plan.source_session.cli,
        plan.source_session.id,
        plan.rewind_point,
        concise_command_display(&plan.target_command)
    )
}

pub(crate) fn concise_command_display(command: &TargetLaunchCommand) -> String {
    let mut parts = Vec::with_capacity(command.args.len() + 1);
    parts.push(command.program.clone());
    for (index, arg) in command.args.iter().enumerate() {
        if index + 1 == command.args.len() && arg.len() > 160 {
            parts.push("<handoff-prompt>".into());
        } else {
            parts.push(shellish_quote(arg));
        }
    }
    parts.join(" ")
}

fn shellish_quote(value: &str) -> String {
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '/' | ':' | '='))
    {
        value.into()
    } else {
        format!("{value:?}")
    }
}

fn run_target_command(command: &TargetLaunchCommand) -> Result<ExitStatus, CoreError> {
    command_process(command)
        .status()
        .map_err(|error| launch_start_error(command, error))
}

#[cfg(unix)]
fn exec_interactive_command(command: &TargetLaunchCommand) -> Result<(), CoreError> {
    use std::os::unix::process::CommandExt;

    let error = command_process(command).exec();
    Err(launch_start_error(command, error))
}

#[cfg(not(unix))]
fn exec_interactive_command(command: &TargetLaunchCommand) -> Result<(), CoreError> {
    let _ = run_target_command(command)?;
    Ok(())
}

fn command_process(command: &TargetLaunchCommand) -> Command {
    let mut process = Command::new(&command.program);
    process.args(&command.args);
    if let Some(cwd) = command.cwd.as_deref().filter(|cwd| Path::new(cwd).is_dir()) {
        process.current_dir(cwd);
    }
    process
}

fn launch_start_error(command: &TargetLaunchCommand, error: std::io::Error) -> CoreError {
    CoreError::LaunchStart {
        command: command.display.clone(),
        reason: error.to_string(),
    }
}

fn original_args(session: &SessionSummary, cwd: Option<&str>) -> Vec<String> {
    match session.cli {
        CliTool::Codex => {
            let mut args = Vec::new();
            if let Some(cwd) = cwd {
                args.push("-C".into());
                args.push(cwd.into());
            }
            args.push("resume".into());
            args.push(session.id.clone());
            args
        }
        CliTool::Claude => vec!["--resume".into(), session.id.clone()],
        CliTool::Hermes => vec!["--resume".into(), session.id.clone()],
    }
}

fn target_args(target: CliTool, cwd: Option<&str>, prompt: String) -> Vec<String> {
    match target {
        CliTool::Codex => {
            let mut args = Vec::new();
            if let Some(cwd) = cwd {
                args.push("-C".into());
                args.push(cwd.into());
            }
            args.push(prompt);
            args
        }
        CliTool::Claude => {
            let mut args = Vec::new();
            if let Some(cwd) = cwd {
                args.push("--add-dir".into());
                args.push(cwd.into());
            }
            args.push("--name".into());
            args.push("moonbox-handoff".into());
            args.push(prompt);
            args
        }
        CliTool::Hermes => {
            let mut args = vec![
                "chat".into(),
                "--source".into(),
                "moonbox".into(),
                "--query".into(),
                prompt,
            ];
            if env::var_os("MOONBOX_HERMES_TUI").is_some() {
                args.insert(1, "--tui".into());
            }
            args
        }
    }
}

fn handoff_prompt(
    session: &SessionSummary,
    capsule: &WorkCapsule,
    continuation: &ContinuationProtocol,
) -> String {
    if let Some(artifact) = capsule.handoff_artifact.as_deref()
        && !compiler::compiler_is_builtin(&capsule.compiler)
    {
        return skill_handoff_prompt(session, capsule, artifact);
    }

    let prompt_session = redaction::redact_session_for_prompt(session, &capsule.redaction);
    let generated_handoff = capsule
        .handoff_artifact
        .as_ref()
        .map(|artifact| {
            format!(
                "\nGenerated Handoff Artifact\n\n{}\n",
                prompt_value(artifact)
            )
        })
        .unwrap_or_default();
    format!(
        "\
You are receiving a Moonbox cross-CLI handoff.

Source
- CLI: {}
- Session: {}
- Title: {}
- Cwd: {}
- Source health: {}

Target
- CLI: {}
- Handoff label: {}
- Rewind point: {}

Continuation Protocol
{}

Work Capsule Summary

Goal:
{}

State:
{}

Decisions:
{}

Todo:
{}

Evidence:
{}

Risks:
{}

Privacy / Redaction:
{}
{}

Instructions
- Continue from the selected rewind point using this handoff.
- Treat the source session as read-only.
- Do not raw-resume the source session unless Moonbox explicitly asks for original-session resume.
- Start by briefly restating the goal, current state, next step, and risks before making changes.
",
        prompt_session.cli,
        prompt_session.id,
        prompt_value(&prompt_session.title),
        prompt_value(&prompt_session.cwd),
        session_health_prompt(&prompt_session),
        capsule.target_cli,
        prompt_value(&capsule.handoff_label),
        capsule.rewind_point,
        continuation_prompt(continuation),
        prompt_value(&capsule.goal),
        prompt_value(&capsule.state),
        bullet_lines(&capsule.decisions),
        todo_lines(&capsule.todo),
        bullet_lines(&capsule.evidence),
        bullet_lines(&capsule.risks),
        redaction::prompt_summary(&capsule.redaction),
        generated_handoff
    )
}

fn skill_handoff_prompt(session: &SessionSummary, capsule: &WorkCapsule, artifact: &str) -> String {
    let prompt_session = redaction::redact_session_for_prompt(session, &capsule.redaction);
    let artifact = artifact.trim();
    let path = capsule
        .handoff_artifact_path
        .as_deref()
        .filter(|path| !path.trim().is_empty());
    if prefers_chinese_handoff_prompt(&prompt_session, artifact) {
        return skill_handoff_prompt_zh(&prompt_session, capsule, artifact, path);
    }
    skill_handoff_prompt_en(&prompt_session, capsule, artifact, path)
}

fn skill_handoff_prompt_zh(
    session: &SessionSummary,
    capsule: &WorkCapsule,
    artifact: &str,
    artifact_path: Option<&str>,
) -> String {
    let document = artifact_path
        .map(|path| {
            format!(
                "\
交接文档：
- {}

请先读取这份 Markdown，再继续执行。不要把这条启动说明当成交接正文。
",
                prompt_value(path)
            )
        })
        .unwrap_or_else(|| {
            format!(
                "\
交接正文：

{}
",
                if artifact.is_empty() { "-" } else { artifact }
            )
        });
    format!(
        "\
这是一份交接任务。具体交接内容请阅读下面的 handoff 文档。

{}

原 session 摘要：
- Source CLI: {}
- Session ID: {}
- Title: {}
- Cwd: {}
- Branch: {}
- Updated: {}
- Tokens: {}
- Source size: {}
- Rewind: {}
- Target CLI: {}
- Handoff skill: {}

执行要求：
- 只从交接文档继续工作；不要恢复或修改原 source session。
- 如果无法读取交接文档，请先明确说明文件不可访问，不要凭这条启动说明继续。
- 开始动手前，先简短复述目标、当前状态、下一步和风险。
",
        document.trim(),
        session.cli,
        session.id,
        prompt_value(&session.title),
        prompt_value(&session.cwd),
        session.branch.as_deref().unwrap_or("-"),
        prompt_value(&session.updated),
        format_token_count_opt(session.token_count),
        format_source_size_opt(session.source_size_bytes),
        prompt_value(&capsule.rewind_point),
        capsule.target_cli,
        capsule.handoff_skill.as_deref().unwrap_or("handoff"),
    )
}

fn skill_handoff_prompt_en(
    session: &SessionSummary,
    capsule: &WorkCapsule,
    artifact: &str,
    artifact_path: Option<&str>,
) -> String {
    let document = artifact_path
        .map(|path| {
            format!(
                "\
Handoff document:
- {}

Read this Markdown file first, then continue the task. Do not treat this launch note as the handoff body.
",
                prompt_value(path)
            )
        })
        .unwrap_or_else(|| {
            format!(
                "\
Handoff body:

{}
",
                if artifact.is_empty() {
                    "-"
                } else {
                    artifact
                }
            )
        });
    format!(
        "\
This is a handoff task. Read the handoff document below for the actual continuation content.

{}

Source session:
- Source CLI: {}
- Session ID: {}
- Title: {}
- Cwd: {}
- Branch: {}
- Updated: {}
- Tokens: {}
- Source size: {}
- Rewind: {}
- Target CLI: {}
- Handoff skill: {}

Instructions:
- Continue only from the handoff document; do not resume or mutate the original source session.
- If the handoff document cannot be read, say so before proceeding instead of relying on this launch note.
- Before making changes, briefly restate the goal, current state, next step, and risks.
",
        document.trim(),
        session.cli,
        session.id,
        prompt_value(&session.title),
        prompt_value(&session.cwd),
        session.branch.as_deref().unwrap_or("-"),
        prompt_value(&session.updated),
        format_token_count_opt(session.token_count),
        format_source_size_opt(session.source_size_bytes),
        prompt_value(&capsule.rewind_point),
        capsule.target_cli,
        capsule.handoff_skill.as_deref().unwrap_or("handoff"),
    )
}

fn prefers_chinese_handoff_prompt(session: &SessionSummary, artifact: &str) -> bool {
    contains_cjk(&session.title) || contains_cjk(artifact)
}

fn contains_cjk(value: &str) -> bool {
    value.chars().any(|ch| {
        matches!(
            ch,
            '\u{3400}'..='\u{4dbf}' | '\u{4e00}'..='\u{9fff}' | '\u{f900}'..='\u{faff}'
        )
    })
}

fn format_token_count_opt(tokens: Option<usize>) -> String {
    tokens.map(format_token_count).unwrap_or_else(|| "-".into())
}

fn format_token_count(tokens: usize) -> String {
    match tokens {
        0..=999 => tokens.to_string(),
        1_000..=999_999 => format!("{:.1}K", tokens as f64 / 1_000.0),
        _ => format!("{:.1}M", tokens as f64 / 1_000_000.0),
    }
}

fn format_source_size_opt(bytes: Option<u64>) -> String {
    bytes.map(format_source_size).unwrap_or_else(|| "-".into())
}

fn format_source_size(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;
    let bytes = bytes as f64;
    if bytes >= GIB {
        format!("{:.1}GB", bytes / GIB)
    } else if bytes >= MIB {
        format!("{:.1}MB", bytes / MIB)
    } else if bytes >= KIB {
        format!("{:.1}KB", bytes / KIB)
    } else {
        format!("{}B", bytes as u64)
    }
}

fn continuation_prompt(continuation: &ContinuationProtocol) -> String {
    let mut lines = vec![
        format!("- Requested level: {}", continuation.requested_level),
        format!("- Target input level: {}", continuation.target_input_level),
        format!(
            "- Package import: {}",
            if continuation.package_import.requested {
                continuation.package_import.reason.as_str()
            } else {
                "not requested"
            }
        ),
        format!(
            "- Workspace restore: {}",
            if continuation.workspace_restore.requested {
                continuation.workspace_restore.reason.as_str()
            } else {
                "not requested"
            }
        ),
    ];
    lines.extend(
        continuation
            .notes
            .iter()
            .map(|note| format!("- Note: {}", prompt_value(note))),
    );
    lines.join("\n")
}

fn session_health_prompt(session: &SessionSummary) -> String {
    match session.health_reason.as_deref() {
        Some(reason) if !reason.trim().is_empty() => {
            format!(
                "{} - {}",
                session_status_label(session.status),
                prompt_value(reason)
            )
        }
        _ => session_status_label(session.status).into(),
    }
}

fn session_status_label(status: super::model::SessionStatus) -> &'static str {
    match status {
        super::model::SessionStatus::Healthy => "healthy",
        super::model::SessionStatus::Warning => "warning",
        super::model::SessionStatus::Failed => "failed",
    }
}

fn bullet_lines(items: &[String]) -> String {
    if items.is_empty() {
        return "- None recorded.".into();
    }
    items
        .iter()
        .map(|item| format!("- {}", prompt_value(item)))
        .collect::<Vec<_>>()
        .join("\n")
}

fn todo_lines(items: &[ChecklistItem]) -> String {
    if items.is_empty() {
        return "- [ ] No todo items recorded.".into();
    }
    items
        .iter()
        .map(|item| {
            let marker = if item.done { "x" } else { " " };
            format!("- [{marker}] {}", prompt_value(&item.text))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn prompt_value(value: &str) -> String {
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        "-".into()
    } else {
        normalized
    }
}

pub(crate) fn configured_target_binary(target: CliTool) -> String {
    let env_key = format!("MOONBOX_{}_BIN", target.id().to_ascii_uppercase());
    env::var(env_key).unwrap_or_else(|_| target.id().into())
}

fn usable_cwd(cwd: &str) -> Option<String> {
    let cwd = cwd.trim();
    if cwd.is_empty() || cwd == "~" {
        return None;
    }
    let expanded = expand_home(cwd);
    let path = Path::new(&expanded);
    if path.is_absolute() && path.is_dir() {
        Some(expanded)
    } else {
        None
    }
}

fn expand_home(path: &str) -> String {
    if path == "~" {
        return env::var("HOME").unwrap_or_else(|_| "~".into());
    }
    if let Some(rest) = path.strip_prefix("~/")
        && let Some(home) = env::var_os("HOME")
    {
        return PathBuf::from(home)
            .join(rest)
            .to_string_lossy()
            .into_owned();
    }
    path.into()
}

fn shell_command(program: &str, args: &[String]) -> String {
    std::iter::once(program)
        .chain(args.iter().map(String::as_str))
        .map(shell_quote)
        .collect::<Vec<_>>()
        .join(" ")
}

fn shell_quote(value: &str) -> String {
    if value.is_empty() {
        return "''".into();
    }
    if value.bytes().all(|byte| {
        byte.is_ascii_alphanumeric()
            || matches!(byte, b'_' | b'-' | b'.' | b'/' | b':' | b'=' | b',')
    }) {
        return value.into();
    }
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{
        data,
        model::{SessionRuntimeStatus, SessionStatus},
        workbench,
    };

    fn codex_session_with_cwd(cwd: String) -> SessionSummary {
        SessionSummary {
            id: "codex-cxcp-design".into(),
            cli: CliTool::Codex,
            title: "Codex fixture".into(),
            cwd,
            updated_at: "2026-06-16T00:00:00Z".into(),
            updated: "now".into(),
            runtime_status: SessionRuntimeStatus::Unknown,
            runtime_reason: None,
            status: SessionStatus::Healthy,
            branch: None,
            token_count: None,
            health_reason: None,
            event_count: 1,
            resume_command: "codex resume codex-cxcp-design".into(),
            source_provenance: SourceProvenance::Fixture,
            source_path: None,
            source_size_bytes: None,
            parse_skip_count: 0,
            provider_metadata: None,
            anatomy: None,
        }
    }

    #[test]
    fn codex_command_uses_prompt_argument_and_workspace() {
        let data = data::workbench_data(CliTool::Codex, CliTool::Hermes).expect("data");
        let session = data
            .sessions
            .iter()
            .find(|session| session.id == data.capsule.source_session)
            .expect("session");

        let command = target_command(CliTool::Codex, session, &data.capsule).expect("command");

        assert_eq!(command.program, "codex");
        assert!(command.display.starts_with("codex "));
        assert!(command.display.contains("Work Capsule Summary"));
        assert!(!command.display.contains("Work Capsule JSON"));
    }

    #[test]
    fn target_handoff_prompt_is_readable_summary_not_raw_json() {
        let data = data::workbench_data(CliTool::Claude, CliTool::Codex).expect("data");
        let session = data
            .sessions
            .iter()
            .find(|session| session.id == data.capsule.source_session)
            .expect("session");

        let command = target_command(CliTool::Codex, session, &data.capsule).expect("command");
        let prompt = command.args.last().expect("prompt");

        assert!(prompt.contains("Source\n- CLI: Claude"));
        assert!(prompt.contains("Work Capsule Summary"));
        assert!(prompt.contains("Decisions:\n- "));
        assert!(prompt.contains("Todo:\n- ["));
        assert!(prompt.contains("Risks:\n- "));
        assert!(prompt.contains("Privacy / Redaction"));
        assert!(prompt.contains("Prompt injection"));
        assert!(prompt.contains("Instructions\n- Continue from the selected rewind point"));
        assert!(!prompt.contains("Work Capsule JSON"));
        assert!(!prompt.contains("\"source_cli\""));
        assert!(!prompt.contains("{\"version\""));
        assert!(!prompt.contains("~/coding/qc-platform"));
    }

    #[test]
    fn agent_skill_handoff_prompt_uses_markdown_artifact_without_capsule_wrapper() {
        let mut data = data::workbench_data(CliTool::Claude, CliTool::Codex).expect("data");
        let session = data
            .sessions
            .iter()
            .find(|session| session.id == data.capsule.source_session)
            .expect("session");
        data.capsule.compiler = "agent:codex:handoff".into();
        data.capsule.handoff_runner = Some("Codex".into());
        data.capsule.handoff_skill = Some("handoff".into());
        data.capsule.handoff_artifact_path =
            Some("/tmp/moonbox-continuation-handoff-demo.md".into());
        data.capsule.handoff_artifact = Some(
            "# Handoff\n\nContinue with the community skill output.\n\n## Next steps\n- Validate UI copy."
                .into(),
        );

        let command = target_command(CliTool::Codex, session, &data.capsule).expect("command");
        let prompt = command.args.last().expect("prompt");

        assert!(prompt.contains("This is a handoff task."));
        assert!(prompt.contains("Read the handoff document below"));
        assert!(prompt.contains("/tmp/moonbox-continuation-handoff-demo.md"));
        assert!(prompt.contains("Source session:"));
        assert!(prompt.contains("Source CLI: Claude"));
        assert!(prompt.contains("Target CLI: Codex"));
        assert!(prompt.contains("Handoff skill: handoff"));
        assert!(prompt.contains("Session ID:"));
        assert!(!prompt.contains("Continue with the community skill output."));
        assert!(!prompt.contains("Work Capsule Summary"));
        assert!(!prompt.contains("Generated Handoff Artifact"));
        assert!(!prompt.contains("Moonbox continuation handoff."));
        assert!(!prompt.contains("Privacy / Redaction"));
        assert!(!prompt.contains("Decisions:\n"));
        assert!(!prompt.contains("Risks:\n"));
    }

    #[test]
    fn agent_skill_handoff_prompt_points_chinese_targets_to_artifact_file() {
        let mut data = data::workbench_data(CliTool::Claude, CliTool::Codex).expect("data");
        let mut session = data
            .sessions
            .iter()
            .find(|session| session.id == data.capsule.source_session)
            .expect("session")
            .clone();
        session.title = "继续 oxlint 版本分析任务".into();
        session.token_count = Some(81_000);
        session.source_size_bytes = Some(12_345_678);
        data.capsule.compiler = "agent:codex:handoff".into();
        data.capsule.handoff_skill = Some("handoff".into());
        data.capsule.handoff_artifact_path =
            Some("/tmp/moonbox-continuation-handoff-6e04f5c0.md".into());
        data.capsule.handoff_artifact =
            Some("# Handoff\n\n未完成的 oxlint 版本分析任务，继续动作见本文档。".into());

        let command = target_command(CliTool::Codex, &session, &data.capsule).expect("command");
        let prompt = command.args.last().expect("prompt");

        assert!(prompt.contains("这是一份交接任务。"));
        assert!(prompt.contains("交接文档："));
        assert!(prompt.contains("/tmp/moonbox-continuation-handoff-6e04f5c0.md"));
        assert!(prompt.contains("原 session 摘要："));
        assert!(prompt.contains("Source CLI: Claude"));
        assert!(prompt.contains("Session ID:"));
        assert!(prompt.contains("Title: 继续 oxlint 版本分析任务"));
        assert!(prompt.contains("Tokens: 81.0K"));
        assert!(prompt.contains("Source size: 11.8MB"));
        assert!(prompt.contains("Handoff skill: handoff"));
        assert!(!prompt.contains("未完成的 oxlint 版本分析任务，继续动作见本文档。"));
        assert!(!prompt.contains("Moonbox cross-CLI"));
    }

    #[test]
    fn claude_command_sets_session_name() {
        let data = data::workbench_data(CliTool::Codex, CliTool::Claude).expect("data");
        let session = data
            .sessions
            .iter()
            .find(|session| session.id == data.capsule.source_session)
            .expect("session");

        let command = target_command(CliTool::Claude, session, &data.capsule).expect("command");

        assert_eq!(command.program, "claude");
        assert!(
            command
                .args
                .windows(2)
                .any(|pair| pair == ["--name", "moonbox-handoff"])
        );
    }

    #[test]
    fn hermes_command_uses_chat_query_source() {
        let data = data::workbench_data(CliTool::Codex, CliTool::Hermes).expect("data");
        let session = data
            .sessions
            .iter()
            .find(|session| session.id == data.capsule.source_session)
            .expect("session");

        let command = target_command(CliTool::Hermes, session, &data.capsule).expect("command");

        assert_eq!(command.program, "hermes");
        assert_eq!(command.args[0], "chat");
        assert!(
            command
                .args
                .windows(2)
                .any(|pair| pair == ["--source", "moonbox"])
        );
        assert!(command.args.windows(2).any(|pair| pair[0] == "--query"));
    }

    #[test]
    fn shell_quote_preserves_copyable_display() {
        assert_eq!(shell_quote("abc-123"), "abc-123");
        assert_eq!(shell_quote("hello world"), "'hello world'");
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
    }

    #[test]
    fn original_commands_use_source_cli_resume_entrypoints() {
        let data = data::workbench_data(CliTool::Codex, CliTool::Hermes).expect("data");
        let codex = data
            .sessions
            .iter()
            .find(|session| session.cli == CliTool::Codex)
            .expect("codex");
        let claude = data
            .sessions
            .iter()
            .find(|session| session.cli == CliTool::Claude)
            .expect("claude");
        let hermes = data
            .sessions
            .iter()
            .find(|session| session.cli == CliTool::Hermes)
            .expect("hermes");

        assert_eq!(
            original_command(codex).args,
            ["resume", "codex-cxcp-design"]
        );
        assert_eq!(
            original_command(claude).args,
            ["--resume", "claude-qc-platform"]
        );
        assert_eq!(
            original_command(hermes).args,
            ["--resume", "hermes-cxcp-502"]
        );
    }

    #[test]
    fn codex_original_resume_passes_source_cwd_to_cli() {
        let root = std::env::temp_dir().join(format!(
            "moonbox-original-command-cwd-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).expect("temp root");
        let session = codex_session_with_cwd(root.display().to_string());

        let command = original_command(&session);

        assert_eq!(
            command.args,
            [
                "-C",
                root.to_string_lossy().as_ref(),
                "resume",
                "codex-cxcp-design"
            ]
        );
        assert_eq!(
            command.cwd.as_deref(),
            Some(root.to_string_lossy().as_ref())
        );
        assert!(command.display.contains(" -C "));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn original_handoff_notice_names_session_and_command() {
        let data = data::workbench_data(CliTool::Codex, CliTool::Hermes).expect("data");
        let session = data
            .sessions
            .iter()
            .find(|session| session.cli == CliTool::Codex)
            .expect("codex")
            .clone();
        let command = original_command(&session);
        let plan = OriginalSessionPlan {
            version: 1,
            action: crate::core::model::SessionAction::OriginalResume,
            dry_run: true,
            source_session: session,
            command,
        };

        let notice = original_handoff_notice(&plan);

        assert!(notice.contains("Opening original session: Codex codex-cxcp-design"));
        assert!(notice.contains("Cwd: terminal default"));
        assert!(notice.contains("Command: codex resume codex-cxcp-design"));
    }

    #[cfg(unix)]
    #[test]
    fn run_original_interactive_waits_for_fake_binary_without_execing_test() {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;

        let root = std::env::temp_dir().join(format!(
            "moonbox-original-interactive-{}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("temp root");
        let marker = root.join("args.txt");
        let pwd_marker = root.join("pwd.txt");
        let workspace = root.join("workspace");
        fs::create_dir_all(&workspace).expect("workspace");
        let script = root.join("fake-original");
        fs::write(
            &script,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\npwd > '{}'\nexit 7\n",
                marker.display(),
                pwd_marker.display()
            ),
        )
        .expect("fake script");
        let mut permissions = fs::metadata(&script)
            .expect("script metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script, permissions).expect("script permissions");

        let data = data::workbench_data(CliTool::Codex, CliTool::Hermes).expect("data");
        let session = data
            .sessions
            .iter()
            .find(|session| session.cli == CliTool::Codex)
            .expect("codex")
            .clone();
        let mut session = session;
        session.cwd = workspace.display().to_string();
        let mut command = original_command(&session);
        command.program = script.display().to_string();
        command.display = format!("{} {}", command.program, command.args.join(" "));
        let plan = OriginalSessionPlan {
            version: 1,
            action: crate::core::model::SessionAction::OriginalResume,
            dry_run: true,
            source_session: session,
            command,
        };

        let status = run_original_interactive(plan).expect("interactive status");

        assert_eq!(status.code(), Some(7));
        assert_eq!(
            fs::read_to_string(marker).expect("marker"),
            format!("-C\n{}\nresume\ncodex-cxcp-design\n", workspace.display())
        );
        let actual_pwd =
            fs::canonicalize(fs::read_to_string(pwd_marker).expect("pwd marker").trim())
                .expect("actual pwd");
        let expected_pwd = fs::canonicalize(&workspace).expect("expected pwd");
        assert_eq!(actual_pwd, expected_pwd);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn execute_plan_blocks_missing_target_binary_before_spawn() {
        let mut plan = workbench::launch_plan(Some("codex-cxcp-design"), CliTool::Hermes, None)
            .expect("launch plan result")
            .expect("launch plan");
        plan.target_command.program = format!("/tmp/moonbox-missing-target-{}", std::process::id());
        plan.target_command.display = plan.target_command.program.clone();

        let error = execute_plan(plan, false).expect_err("missing target should block");

        assert!(matches!(error, CoreError::LaunchBlocked { .. }));
        assert!(error.to_string().contains("not found"));
    }
}
