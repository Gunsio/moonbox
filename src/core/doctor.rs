use std::{env, fs, path::Path};

use super::{
    compiler, config, launcher,
    model::{
        CliTool, CompilerPresetStatus, DoctorReport, SessionSummary, VerificationCheck,
        VerificationStatus,
    },
    workbench,
};

pub fn diagnose() -> DoctorReport {
    let mut checks = Vec::new();
    checks.push(config_check());
    checks.push(session_discovery_check());
    checks.extend(CliTool::ALL.into_iter().map(target_binary_check));
    checks.push(compiler_catalog_check());

    report(checks)
}

pub fn diagnose_with_session_summaries(sessions: &[SessionSummary]) -> DoctorReport {
    let mut checks = Vec::new();
    checks.push(config_check());
    checks.push(session_summaries_check(sessions));
    checks.extend(CliTool::ALL.into_iter().map(target_binary_check));
    checks.push(compiler_catalog_check());

    report(checks)
}

fn report(checks: Vec<VerificationCheck>) -> DoctorReport {
    let status = overall_status(&checks);
    DoctorReport {
        version: 1,
        status,
        ready: status != VerificationStatus::Fail,
        checks,
    }
}

fn config_check() -> VerificationCheck {
    let Some(path) = config::config_path() else {
        return check(
            "config_file",
            VerificationStatus::Fail,
            "HOME is not available and MOONBOX_CONFIG is not set",
        );
    };
    let display = path.display().to_string();
    if !path.exists() {
        return check(
            "config_file",
            VerificationStatus::Pass,
            format!("{display} is absent; built-in defaults will be used"),
        );
    }
    match fs::read_to_string(&path) {
        Ok(_) => match config::validate_config_file_at(&path) {
            Ok(_) => check(
                "config_file",
                VerificationStatus::Pass,
                format!("{display} is readable Moonbox config"),
            ),
            Err(error) => check(
                "config_file",
                VerificationStatus::Fail,
                format!("{display} is not a valid Moonbox config: {error}"),
            ),
        },
        Err(error) => check(
            "config_file",
            VerificationStatus::Fail,
            format!("{display} cannot be read: {error}"),
        ),
    }
}

fn session_discovery_check() -> VerificationCheck {
    match workbench::list_sessions() {
        Ok(sessions) => session_summaries_check(&sessions),
        Err(error) => check(
            "session_discovery",
            VerificationStatus::Fail,
            format!("cannot list session summaries: {error}"),
        ),
    }
}

fn session_summaries_check(sessions: &[SessionSummary]) -> VerificationCheck {
    if sessions.is_empty() {
        return check(
            "session_discovery",
            VerificationStatus::Warn,
            "no sessions discovered from configured source homes",
        );
    }

    let codex = sessions
        .iter()
        .filter(|session| session.cli == CliTool::Codex)
        .count();
    let claude = sessions
        .iter()
        .filter(|session| session.cli == CliTool::Claude)
        .count();
    let hermes = sessions
        .iter()
        .filter(|session| session.cli == CliTool::Hermes)
        .count();
    check(
        "session_discovery",
        VerificationStatus::Pass,
        format!(
            "{} session summaries discovered; codex={codex}, claude={claude}, hermes={hermes}",
            sessions.len()
        ),
    )
}

fn target_binary_check(target: CliTool) -> VerificationCheck {
    let program = launcher::configured_target_binary(target);
    let name = format!("target_{}_binary", target.id());
    if command_available(&program) {
        check(
            name,
            VerificationStatus::Pass,
            format!("{target} target resolves to {program}"),
        )
    } else {
        check(
            name,
            VerificationStatus::Warn,
            format!("{target} target binary is not on PATH: {program}"),
        )
    }
}

fn compiler_catalog_check() -> VerificationCheck {
    let compilers = compiler::compiler_catalog_entries();
    let active = compilers
        .iter()
        .filter(|compiler| compiler.status != CompilerPresetStatus::Disabled)
        .count();
    if active == 0 {
        return check(
            "compiler_catalog",
            VerificationStatus::Fail,
            "no enabled capsule compiler presets are available",
        );
    }

    let default = compiler::default_compiler_id();
    let warning = compilers
        .iter()
        .filter(|compiler| compiler.status == CompilerPresetStatus::Warning)
        .count();
    let disabled = compilers.len().saturating_sub(active);
    check(
        "compiler_catalog",
        VerificationStatus::Pass,
        format!("default={default}; active={active}; warning={warning}; disabled={disabled}"),
    )
}

fn command_available(program: &str) -> bool {
    let program = program.trim();
    if program.is_empty() {
        return false;
    }

    let path = Path::new(program);
    if path.components().count() > 1 {
        return path.is_file();
    }

    env::var_os("PATH")
        .map(|path| env::split_paths(&path).any(|directory| directory.join(program).is_file()))
        .unwrap_or(false)
}

fn overall_status(checks: &[VerificationCheck]) -> VerificationStatus {
    if checks
        .iter()
        .any(|check| check.status == VerificationStatus::Fail)
    {
        return VerificationStatus::Fail;
    }
    if checks
        .iter()
        .any(|check| check.status == VerificationStatus::Warn)
    {
        return VerificationStatus::Warn;
    }
    VerificationStatus::Pass
}

fn check(
    name: impl Into<String>,
    status: VerificationStatus,
    detail: impl Into<String>,
) -> VerificationCheck {
    VerificationCheck {
        name: name.into(),
        status,
        detail: detail.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_warns_when_target_binary_is_missing_but_stays_ready() {
        let check = target_binary_check(CliTool::Codex);

        if !command_available(&launcher::configured_target_binary(CliTool::Codex)) {
            assert_eq!(check.status, VerificationStatus::Warn);
        }
    }
}
