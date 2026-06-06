use std::{
    env,
    path::{Path, PathBuf},
    process::Command,
};

use super::{
    error::CoreError,
    model::{
        CliTool, LaunchExecution, LaunchExecutionStatus, LaunchPlan, SessionSummary,
        TargetLaunchCommand, WorkCapsule,
    },
};

pub fn target_command(
    target: CliTool,
    session: &SessionSummary,
    capsule: &WorkCapsule,
) -> Result<TargetLaunchCommand, CoreError> {
    let program = configured_target_binary(target);
    let prompt = handoff_prompt(session, capsule)?;
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

pub fn execute_plan(mut plan: LaunchPlan) -> Result<LaunchExecution, CoreError> {
    if !plan.verification.ready {
        return Err(CoreError::LaunchBlocked {
            reason: format!("verification status {}", plan.verification.status),
        });
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

fn run_target_command(
    command: &TargetLaunchCommand,
) -> Result<std::process::ExitStatus, CoreError> {
    let mut process = Command::new(&command.program);
    process.args(&command.args);
    if let Some(cwd) = command.cwd.as_deref().filter(|cwd| Path::new(cwd).is_dir()) {
        process.current_dir(cwd);
    }
    process.status().map_err(|error| CoreError::LaunchStart {
        command: command.display.clone(),
        reason: error.to_string(),
    })
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

fn handoff_prompt(session: &SessionSummary, capsule: &WorkCapsule) -> Result<String, CoreError> {
    let capsule_json =
        serde_json::to_string(capsule).map_err(|error| CoreError::LaunchPrepare {
            reason: error.to_string(),
        })?;
    Ok(format!(
        "\
You are receiving a Moonbox cross-CLI handoff.

Source session: {} {}
Source title: {}
Source cwd: {}
Target CLI: {}
Rewind point: {}

Continue from the rewind point using the Work Capsule below. Do not try raw
session resume unless Moonbox explicitly asked for original-session resume.

Work Capsule JSON:
{}
",
        session.cli,
        session.id,
        session.title,
        session.cwd,
        capsule.target_cli,
        capsule.rewind_point,
        capsule_json
    ))
}

fn configured_target_binary(target: CliTool) -> String {
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
    use crate::core::data;

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
        assert!(command.display.contains("Work Capsule JSON"));
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
}
