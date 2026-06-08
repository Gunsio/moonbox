use std::{
    env,
    path::{Path, PathBuf},
    process::{Command, ExitStatus},
};

use super::{
    compiler,
    error::CoreError,
    model::{
        ChecklistItem, CliTool, LaunchExecution, LaunchExecutionStatus, LaunchPlan,
        OriginalSessionExecution, OriginalSessionPlan, SessionSummary, SourceProvenance,
        TargetLaunchCommand, WorkCapsule,
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

pub fn target_command(
    target: CliTool,
    session: &SessionSummary,
    capsule: &WorkCapsule,
) -> Result<TargetLaunchCommand, CoreError> {
    let program = configured_target_binary(target);
    let prompt = handoff_prompt(session, capsule);
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

pub fn target_prompt_preview(session: &SessionSummary, capsule: &WorkCapsule) -> String {
    handoff_prompt(session, capsule)
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
    let args = original_args(session);
    let cwd = usable_cwd(&session.cwd);
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
    })
}

pub(crate) fn handoff_original_plan(mut plan: OriginalSessionPlan) -> Result<(), CoreError> {
    plan.dry_run = false;
    exec_interactive_command(&plan.command)
}

pub(crate) fn original_handoff_notice(plan: &OriginalSessionPlan) -> String {
    format!(
        "Opening original session: {} {}\nCommand: {}\n",
        plan.source_session.cli, plan.source_session.id, plan.command.display
    )
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

fn original_args(session: &SessionSummary) -> Vec<String> {
    match session.cli {
        CliTool::Codex => vec!["resume".into(), session.id.clone()],
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

fn handoff_prompt(session: &SessionSummary, capsule: &WorkCapsule) -> String {
    let prompt_session = redaction::redact_session_for_prompt(session, &capsule.redaction);
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

Instructions
- Continue from the selected rewind point using this capsule.
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
        prompt_value(&capsule.goal),
        prompt_value(&capsule.state),
        bullet_lines(&capsule.decisions),
        todo_lines(&capsule.todo),
        bullet_lines(&capsule.evidence),
        bullet_lines(&capsule.risks),
        redaction::prompt_summary(&capsule.redaction)
    )
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
    use crate::core::{data, workbench};

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
        assert!(notice.contains("Command: codex resume codex-cxcp-design"));
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
