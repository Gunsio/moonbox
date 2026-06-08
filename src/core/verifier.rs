use std::{env, path::Path};

use super::{
    compiler,
    model::{
        CliTool, LaunchValidation, SessionStatus, SessionSummary, SourceProvenance,
        TargetLaunchCommand, TimelineEvent, VerificationCheck, VerificationReport,
        VerificationStatus, WorkCapsule,
    },
};

const SUPPORTED_CAPSULE_VERSION: u16 = 1;
const TOKEN_BUDGET_WARN_THRESHOLD: usize = 100_000;
const CAPSULE_SIZE_WARN_BYTES: usize = 64 * 1024;
const CAPSULE_SIZE_FAIL_BYTES: usize = 128 * 1024;

pub fn verify_capsule(
    capsule: &WorkCapsule,
    session: &SessionSummary,
    timeline: &[TimelineEvent],
    requested_target: CliTool,
) -> VerificationReport {
    let rewind_id = rewind_event_id(capsule);
    let mut checks = Vec::new();

    checks.push(check(
        "capsule_version",
        if capsule.version == SUPPORTED_CAPSULE_VERSION {
            VerificationStatus::Pass
        } else {
            VerificationStatus::Fail
        },
        format!(
            "capsule version {} vs supported {}",
            capsule.version, SUPPORTED_CAPSULE_VERSION
        ),
    ));

    let missing_fields = missing_required_fields(capsule);
    checks.push(check(
        "capsule_required_fields",
        if missing_fields.is_empty() {
            VerificationStatus::Pass
        } else {
            VerificationStatus::Fail
        },
        if missing_fields.is_empty() {
            "required capsule fields are populated".into()
        } else {
            format!("missing required field(s): {}", missing_fields.join(", "))
        },
    ));

    checks.push(check(
        "compiler_mode",
        compiler_mode_status(capsule, session),
        compiler_mode_detail(capsule, session),
    ));

    checks.push(check(
        "capsule_source",
        if capsule.source_session == session.id && capsule.source_cli == session.cli {
            VerificationStatus::Pass
        } else {
            VerificationStatus::Fail
        },
        format!(
            "capsule source {} / {} vs selected {} / {}",
            capsule.source_cli, capsule.source_session, session.cli, session.id
        ),
    ));

    checks.push(check(
        "target_cli",
        if capsule.target_cli == requested_target {
            VerificationStatus::Pass
        } else {
            VerificationStatus::Fail
        },
        format!(
            "capsule target {} vs requested {}",
            capsule.target_cli, requested_target
        ),
    ));

    checks.push(check(
        "rewind_exists",
        if timeline.iter().any(|event| event.id == rewind_id) {
            VerificationStatus::Pass
        } else {
            VerificationStatus::Fail
        },
        format!("rewind {rewind_id} in selected timeline"),
    ));

    checks.push(check(
        "target_branch",
        if capsule.target_branch.contains(capsule.target_cli.id())
            && capsule.target_branch.contains(rewind_id)
        {
            VerificationStatus::Pass
        } else {
            VerificationStatus::Fail
        },
        format!("target branch {}", capsule.target_branch),
    ));

    checks.push(check(
        "target_branch_namespace",
        if capsule.target_branch.starts_with("moonbox/") {
            VerificationStatus::Pass
        } else {
            VerificationStatus::Warn
        },
        format!("target branch namespace {}", capsule.target_branch),
    ));

    checks.push(check(
        "handoff_context",
        handoff_context_status(capsule),
        handoff_context_detail(capsule),
    ));

    checks.push(check(
        "risk_context",
        if capsule.risks.is_empty() {
            VerificationStatus::Warn
        } else {
            VerificationStatus::Pass
        },
        if capsule.risks.is_empty() {
            "risk list is empty; target may miss known hazards".into()
        } else {
            format!("{} risk(s) captured", capsule.risks.len())
        },
    ));

    checks.push(check(
        "capsule_size",
        capsule_size_status(capsule),
        capsule_size_detail(capsule),
    ));

    checks.push(check(
        "token_budget",
        match session.token_count {
            Some(count) if count > TOKEN_BUDGET_WARN_THRESHOLD => VerificationStatus::Warn,
            _ => VerificationStatus::Pass,
        },
        format!(
            "{} / {} tokens",
            session
                .token_count
                .map(|count| count.to_string())
                .unwrap_or_else(|| "unknown".into()),
            TOKEN_BUDGET_WARN_THRESHOLD
        ),
    ));

    checks.push(check(
        "source_health",
        match session.status {
            SessionStatus::Healthy => VerificationStatus::Pass,
            SessionStatus::Warning | SessionStatus::Failed => VerificationStatus::Warn,
        },
        session
            .health_reason
            .clone()
            .unwrap_or_else(|| "no health reason".into()),
    ));

    checks.push(check(
        "target_support",
        target_support_status(session, requested_target),
        if session.status == SessionStatus::Failed && session.cli == requested_target {
            format!(
                "{} raw resume is known failed for this session",
                requested_target
            )
        } else if session.cli == requested_target {
            format!("Same-CLI handoff to {requested_target}; prefer original resume for no handoff")
        } else {
            format!("{requested_target} handoff dry-run supported")
        },
    ));

    let status = overall_status(&checks);
    VerificationReport {
        version: 1,
        status,
        ready: status != VerificationStatus::Fail,
        checks,
    }
}

pub fn validation_from_report(report: &VerificationReport) -> LaunchValidation {
    let blockers = report
        .checks
        .iter()
        .filter(|check| check.status == VerificationStatus::Fail)
        .map(|check| check.detail.clone())
        .collect::<Vec<_>>();
    if !blockers.is_empty() {
        return LaunchValidation::blocked(blockers);
    }

    let warnings = report
        .checks
        .iter()
        .filter(|check| check.status == VerificationStatus::Warn)
        .map(|check| check.detail.clone())
        .collect::<Vec<_>>();
    if !warnings.is_empty() {
        LaunchValidation::warning(warnings)
    } else {
        LaunchValidation::ready()
    }
}

pub fn execution_command_check(command: &TargetLaunchCommand) -> VerificationCheck {
    let available = command_available(&command.program);
    check(
        "target_command",
        if available {
            VerificationStatus::Pass
        } else {
            VerificationStatus::Fail
        },
        if available {
            format!("target command {} is executable", command.program)
        } else {
            format!(
                "target command {} was not found on disk or PATH",
                command.program
            )
        },
    )
}

pub fn execution_command_blocker(command: &TargetLaunchCommand) -> Option<String> {
    let check = execution_command_check(command);
    (check.status == VerificationStatus::Fail).then_some(check.detail)
}

fn rewind_event_id(capsule: &WorkCapsule) -> &str {
    capsule
        .rewind_point
        .split_whitespace()
        .next()
        .unwrap_or_default()
}

fn target_support_status(session: &SessionSummary, target: CliTool) -> VerificationStatus {
    if session.status == SessionStatus::Failed && session.cli == target {
        VerificationStatus::Fail
    } else if session.cli == target {
        VerificationStatus::Warn
    } else {
        VerificationStatus::Pass
    }
}

fn missing_required_fields(capsule: &WorkCapsule) -> Vec<&'static str> {
    [
        (capsule.source_session.trim().is_empty(), "source_session"),
        (capsule.rewind_point.trim().is_empty(), "rewind_point"),
        (capsule.compiler.trim().is_empty(), "compiler"),
        (capsule.target_branch.trim().is_empty(), "target_branch"),
        (capsule.goal.trim().is_empty(), "goal"),
        (capsule.state.trim().is_empty(), "state"),
    ]
    .into_iter()
    .filter_map(|(missing, field)| missing.then_some(field))
    .collect()
}

fn compiler_mode_status(capsule: &WorkCapsule, session: &SessionSummary) -> VerificationStatus {
    if compiler::compiler_is_builtin(&capsule.compiler)
        && session.source_provenance != SourceProvenance::Fixture
    {
        VerificationStatus::Warn
    } else {
        VerificationStatus::Pass
    }
}

fn compiler_mode_detail(capsule: &WorkCapsule, session: &SessionSummary) -> String {
    if compiler::compiler_is_builtin(&capsule.compiler) {
        if session.source_provenance == SourceProvenance::Fixture {
            format!(
                "{} built-in draft compiler for fixture replay",
                capsule.compiler
            )
        } else {
            format!(
                "{} is a built-in draft compiler; configure an external skill for production handoff",
                capsule.compiler
            )
        }
    } else {
        format!("external compiler {} selected", capsule.compiler)
    }
}

fn handoff_context_status(capsule: &WorkCapsule) -> VerificationStatus {
    if capsule.decisions.is_empty()
        || capsule.todo.is_empty()
        || capsule.evidence.is_empty()
        || !capsule.todo.iter().any(|item| !item.done)
    {
        VerificationStatus::Fail
    } else {
        VerificationStatus::Pass
    }
}

fn handoff_context_detail(capsule: &WorkCapsule) -> String {
    let mut missing = Vec::new();
    if capsule.decisions.is_empty() {
        missing.push("decisions");
    }
    if capsule.todo.is_empty() {
        missing.push("todo");
    }
    if capsule.evidence.is_empty() {
        missing.push("evidence");
    }
    if !capsule.todo.iter().any(|item| !item.done) {
        missing.push("open_todo");
    }
    if missing.is_empty() {
        format!(
            "{} decision(s), {} todo item(s), {} evidence item(s)",
            capsule.decisions.len(),
            capsule.todo.len(),
            capsule.evidence.len()
        )
    } else {
        format!("missing handoff context: {}", missing.join(", "))
    }
}

fn capsule_size_status(capsule: &WorkCapsule) -> VerificationStatus {
    match capsule_size_bytes(capsule) {
        bytes if bytes > CAPSULE_SIZE_FAIL_BYTES => VerificationStatus::Fail,
        bytes if bytes > CAPSULE_SIZE_WARN_BYTES => VerificationStatus::Warn,
        _ => VerificationStatus::Pass,
    }
}

fn capsule_size_detail(capsule: &WorkCapsule) -> String {
    format!(
        "{} / {} bytes",
        capsule_size_bytes(capsule),
        CAPSULE_SIZE_WARN_BYTES
    )
}

fn capsule_size_bytes(capsule: &WorkCapsule) -> usize {
    serde_json::to_vec(capsule)
        .map(|bytes| bytes.len())
        .unwrap_or(usize::MAX)
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

fn overall_status(checks: &[VerificationCheck]) -> VerificationStatus {
    if checks
        .iter()
        .any(|check| check.status == VerificationStatus::Fail)
    {
        VerificationStatus::Fail
    } else if checks
        .iter()
        .any(|check| check.status == VerificationStatus::Warn)
    {
        VerificationStatus::Warn
    } else {
        VerificationStatus::Pass
    }
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
    use crate::core::{data, model::CliTool};
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicUsize, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    #[cfg(unix)]
    static SCRIPT_COUNTER: AtomicUsize = AtomicUsize::new(0);

    #[test]
    fn healthy_cross_cli_capsule_passes() {
        let data = data::workbench_data(CliTool::Codex, CliTool::Hermes).expect("data");
        let session = data
            .sessions
            .iter()
            .find(|session| session.id == data.capsule.source_session)
            .expect("source session");

        let report = verify_capsule(&data.capsule, session, &data.timeline, CliTool::Hermes);

        assert_eq!(report.status, VerificationStatus::Pass);
        assert!(report.ready);
    }

    #[test]
    fn builtin_compiler_warns_for_real_source_handoff() {
        let data = data::workbench_data(CliTool::Codex, CliTool::Hermes).expect("data");
        let mut session = data
            .sessions
            .iter()
            .find(|session| session.id == data.capsule.source_session)
            .expect("source session")
            .clone();
        session.source_provenance = SourceProvenance::Real;

        let report = verify_capsule(&data.capsule, &session, &data.timeline, CliTool::Hermes);

        assert_eq!(report.status, VerificationStatus::Warn);
        assert!(report.ready);
        assert!(report.checks.iter().any(|check| {
            check.name == "compiler_mode" && check.status == VerificationStatus::Warn
        }));
    }

    #[test]
    fn external_compiler_passes_compiler_mode_for_real_source() {
        let data = data::workbench_data(CliTool::Codex, CliTool::Hermes).expect("data");
        let mut session = data
            .sessions
            .iter()
            .find(|session| session.id == data.capsule.source_session)
            .expect("source session")
            .clone();
        session.source_provenance = SourceProvenance::Real;
        let mut capsule = data.capsule.clone();
        capsule.compiler = "production-skill".into();
        capsule.state = "compiled".into();

        let report = verify_capsule(&capsule, &session, &data.timeline, CliTool::Hermes);

        assert!(report.checks.iter().any(|check| {
            check.name == "compiler_mode" && check.status == VerificationStatus::Pass
        }));
    }

    #[test]
    fn failed_same_cli_capsule_fails_target_support() {
        let data = data::workbench_data(CliTool::Hermes, CliTool::Hermes).expect("data");
        let session = data
            .sessions
            .iter()
            .find(|session| session.id == data.capsule.source_session)
            .expect("source session");

        let report = verify_capsule(&data.capsule, session, &data.timeline, CliTool::Hermes);

        assert_eq!(report.status, VerificationStatus::Fail);
        assert!(!report.ready);
        assert!(report.checks.iter().any(
            |check| check.name == "target_support" && check.status == VerificationStatus::Fail
        ));
    }

    #[test]
    fn healthy_same_cli_capsule_warns_target_support() {
        let data = data::workbench_data(CliTool::Codex, CliTool::Codex).expect("data");
        let session = data
            .sessions
            .iter()
            .find(|session| session.id == data.capsule.source_session)
            .expect("source session");

        let report = verify_capsule(&data.capsule, session, &data.timeline, CliTool::Codex);

        assert_eq!(report.status, VerificationStatus::Warn);
        assert!(report.ready);
        assert!(report.checks.iter().any(
            |check| check.name == "target_support" && check.status == VerificationStatus::Warn
        ));
    }

    #[test]
    fn mismatched_requested_target_fails() {
        let data = data::workbench_data(CliTool::Codex, CliTool::Hermes).expect("data");
        let session = data
            .sessions
            .iter()
            .find(|session| session.id == data.capsule.source_session)
            .expect("source session");

        let report = verify_capsule(&data.capsule, session, &data.timeline, CliTool::Codex);

        assert_eq!(report.status, VerificationStatus::Fail);
        assert!(!report.ready);
        assert!(report
            .checks
            .iter()
            .any(|check| check.name == "target_cli" && check.status == VerificationStatus::Fail));
    }

    #[test]
    fn wrong_capsule_version_fails() {
        let data = data::workbench_data(CliTool::Codex, CliTool::Hermes).expect("data");
        let session = data
            .sessions
            .iter()
            .find(|session| session.id == data.capsule.source_session)
            .expect("source session");
        let mut capsule = data.capsule.clone();
        capsule.version = 99;

        let report = verify_capsule(&capsule, session, &data.timeline, CliTool::Hermes);

        assert_eq!(report.status, VerificationStatus::Fail);
        assert!(report.checks.iter().any(|check| {
            check.name == "capsule_version" && check.status == VerificationStatus::Fail
        }));
    }

    #[test]
    fn missing_required_capsule_fields_fail() {
        let data = data::workbench_data(CliTool::Codex, CliTool::Hermes).expect("data");
        let session = data
            .sessions
            .iter()
            .find(|session| session.id == data.capsule.source_session)
            .expect("source session");
        let mut capsule = data.capsule.clone();
        capsule.goal.clear();
        capsule.compiler.clear();

        let report = verify_capsule(&capsule, session, &data.timeline, CliTool::Hermes);

        assert_eq!(report.status, VerificationStatus::Fail);
        assert!(report.checks.iter().any(|check| {
            check.name == "capsule_required_fields"
                && check.status == VerificationStatus::Fail
                && check.detail.contains("goal")
                && check.detail.contains("compiler")
        }));
    }

    #[test]
    fn missing_handoff_context_fails() {
        let data = data::workbench_data(CliTool::Codex, CliTool::Hermes).expect("data");
        let session = data
            .sessions
            .iter()
            .find(|session| session.id == data.capsule.source_session)
            .expect("source session");
        let mut capsule = data.capsule.clone();
        capsule.decisions.clear();
        capsule.todo.clear();
        capsule.evidence.clear();

        let report = verify_capsule(&capsule, session, &data.timeline, CliTool::Hermes);

        assert_eq!(report.status, VerificationStatus::Fail);
        assert!(report.checks.iter().any(|check| {
            check.name == "handoff_context" && check.status == VerificationStatus::Fail
        }));
    }

    #[test]
    fn all_done_todo_fails_handoff_context() {
        let data = data::workbench_data(CliTool::Codex, CliTool::Hermes).expect("data");
        let session = data
            .sessions
            .iter()
            .find(|session| session.id == data.capsule.source_session)
            .expect("source session");
        let mut capsule = data.capsule.clone();
        for item in &mut capsule.todo {
            item.done = true;
        }

        let report = verify_capsule(&capsule, session, &data.timeline, CliTool::Hermes);

        assert_eq!(report.status, VerificationStatus::Fail);
        assert!(
            report
                .checks
                .iter()
                .any(|check| check.name == "handoff_context" && check.detail.contains("open_todo"))
        );
    }

    #[test]
    fn empty_risk_context_warns_but_stays_ready() {
        let data = data::workbench_data(CliTool::Codex, CliTool::Hermes).expect("data");
        let session = data
            .sessions
            .iter()
            .find(|session| session.id == data.capsule.source_session)
            .expect("source session");
        let mut capsule = data.capsule.clone();
        capsule.risks.clear();

        let report = verify_capsule(&capsule, session, &data.timeline, CliTool::Hermes);

        assert_eq!(report.status, VerificationStatus::Warn);
        assert!(report.ready);
        assert!(
            report
                .checks
                .iter()
                .any(|check| check.name == "risk_context"
                    && check.status == VerificationStatus::Warn)
        );
    }

    #[test]
    fn target_branch_without_rewind_fails() {
        let data = data::workbench_data(CliTool::Codex, CliTool::Hermes).expect("data");
        let session = data
            .sessions
            .iter()
            .find(|session| session.id == data.capsule.source_session)
            .expect("source session");
        let mut capsule = data.capsule.clone();
        capsule.target_branch = "moonbox/hermes-rewind-other".into();

        let report = verify_capsule(&capsule, session, &data.timeline, CliTool::Hermes);

        assert_eq!(report.status, VerificationStatus::Fail);
        assert!(
            report
                .checks
                .iter()
                .any(|check| check.name == "target_branch"
                    && check.status == VerificationStatus::Fail)
        );
    }

    #[test]
    fn oversized_capsule_fails() {
        let data = data::workbench_data(CliTool::Codex, CliTool::Hermes).expect("data");
        let session = data
            .sessions
            .iter()
            .find(|session| session.id == data.capsule.source_session)
            .expect("source session");
        let mut capsule = data.capsule.clone();
        capsule.evidence.push("x".repeat(CAPSULE_SIZE_FAIL_BYTES));

        let report = verify_capsule(&capsule, session, &data.timeline, CliTool::Hermes);

        assert_eq!(report.status, VerificationStatus::Fail);
        assert!(report.checks.iter().any(|check| {
            check.name == "capsule_size" && check.status == VerificationStatus::Fail
        }));
    }

    #[test]
    fn execution_command_check_detects_missing_binary() {
        let command = TargetLaunchCommand {
            program: format!("/tmp/moonbox-missing-target-command-{}", std::process::id()),
            args: Vec::new(),
            cwd: None,
            display: "missing".into(),
        };

        let check = execution_command_check(&command);

        assert_eq!(check.status, VerificationStatus::Fail);
        assert!(check.detail.contains("not found"));
    }

    #[cfg(unix)]
    #[test]
    fn execution_command_check_accepts_executable_path() {
        let script = executable_script(
            "target-command",
            r#"#!/bin/sh
exit 0
"#,
        );
        let command = TargetLaunchCommand {
            program: script.to_string_lossy().into_owned(),
            args: Vec::new(),
            cwd: None,
            display: script.to_string_lossy().into_owned(),
        };

        let check = execution_command_check(&command);

        assert_eq!(check.status, VerificationStatus::Pass);
    }

    #[cfg(unix)]
    fn executable_script(name: &str, contents: &str) -> PathBuf {
        let unique = SCRIPT_COUNTER.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "moonbox-verifier-{name}-{}-{nanos}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("script dir");
        let path = dir.join("target.sh");
        fs::write(&path, contents).expect("script");
        let mut permissions = fs::metadata(&path).expect("metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions).expect("permissions");
        path
    }
}
