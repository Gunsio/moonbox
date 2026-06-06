use std::fs;

use super::{
    data,
    error::CoreError,
    launcher,
    model::{
        CapsuleCompileOutput, CapsuleCompileRequest, CliTool, LaunchExecution, LaunchPlan,
        OriginalSessionExecution, OriginalSessionPlan, SessionAction, SessionSummary,
        VerificationReport, WorkCapsule, WorkbenchData,
    },
    verifier,
};

pub fn load_workbench(source: CliTool, target: CliTool) -> Result<WorkbenchData, CoreError> {
    data::workbench_data(source, target)
}

pub fn load_fixture_workbench(
    source: CliTool,
    target: CliTool,
) -> Result<WorkbenchData, CoreError> {
    data::fixture_workbench_data(source, target)
}

pub fn load_workbench_for_session(
    session_id: &str,
    target: CliTool,
) -> Result<Option<WorkbenchData>, CoreError> {
    data::workbench_data_for_session(session_id, target)
}

pub fn load_workbench_from_session_snapshot(
    source_session: SessionSummary,
    sessions: Vec<SessionSummary>,
    source_adapters: Vec<super::model::SourceAdapterReport>,
    target: CliTool,
) -> Result<WorkbenchData, CoreError> {
    data::workbench_data_from_session_snapshot(source_session, sessions, source_adapters, target)
}

pub fn list_sessions() -> Result<Vec<SessionSummary>, CoreError> {
    data::sessions()
}

pub fn find_session(session_id: &str) -> Result<Option<SessionSummary>, CoreError> {
    data::find_session(session_id)
}

pub fn default_session() -> Result<Option<SessionSummary>, CoreError> {
    Ok(list_sessions()?.into_iter().next())
}

pub fn open_command(session_id: Option<&str>) -> Result<Option<String>, CoreError> {
    Ok(open_plan(session_id)?.map(|plan| plan.command.display))
}

pub fn open_plan(session_id: Option<&str>) -> Result<Option<OriginalSessionPlan>, CoreError> {
    let Some(source_session) = selected_session(session_id)? else {
        return Ok(None);
    };
    let command = launcher::original_command(&source_session);
    Ok(Some(OriginalSessionPlan {
        version: 1,
        action: SessionAction::OriginalResume,
        dry_run: true,
        source_session,
        command,
    }))
}

pub fn execute_open(
    session_id: Option<&str>,
) -> Result<Option<OriginalSessionExecution>, CoreError> {
    require_explicit_session(session_id, "original resume")?;
    let Some(plan) = open_plan(session_id)? else {
        return Ok(None);
    };
    launcher::execute_original_plan(plan).map(Some)
}

pub fn capsule(source: CliTool, target: CliTool) -> Result<WorkCapsule, CoreError> {
    Ok(load_workbench(source, target)?.capsule)
}

pub fn compile_request(
    source: CliTool,
    target: CliTool,
    rewind_event_id: &str,
    compiler: Option<&str>,
) -> Result<CapsuleCompileRequest, CoreError> {
    if let Some(compiler) = compiler {
        data::compile_request_with_compiler(source, target, rewind_event_id, compiler)
    } else {
        data::compile_request(source, target, rewind_event_id)
    }
}

pub fn compile_output(
    source: CliTool,
    target: CliTool,
    compiler: Option<&str>,
) -> Result<CapsuleCompileOutput, CoreError> {
    if let Some(compiler) = compiler {
        data::compile_output_with_compiler(source, target, compiler)
    } else {
        data::compile_output(source, target)
    }
}

pub fn compile_capsule(
    session_id: &str,
    target: CliTool,
    rewind_event_id: &str,
    compiler: &str,
) -> Result<Option<WorkCapsule>, CoreError> {
    data::compile_capsule_for_session_id(session_id, target, rewind_event_id, compiler)
}

pub fn launch_plan(
    session_id: Option<&str>,
    target: CliTool,
    capsule_path: Option<&str>,
) -> Result<Option<LaunchPlan>, CoreError> {
    let Some(source_session) = selected_session(session_id)? else {
        return Ok(None);
    };
    let Some((source_session, timeline, generated_capsule)) =
        data::launch_artifacts_for_session_id(&source_session.id, target)?
    else {
        return Ok(None);
    };
    let (capsule, capsule_path) = capsule_for_plan(&generated_capsule, capsule_path)?;
    let target_command = launcher::target_command(target, &source_session, &capsule)?;
    let command = target_command.display.clone();
    let verification =
        verifier::verify_capsule(&capsule, &source_session, &timeline.events, target);

    Ok(Some(LaunchPlan {
        version: 1,
        action: SessionAction::TargetHandoff,
        dry_run: true,
        source_session,
        target_cli: target,
        target_branch: capsule.target_branch,
        capsule_path,
        command,
        target_command,
        verification,
    }))
}

pub fn verify_launch(
    session_id: Option<&str>,
    target: CliTool,
    capsule_path: Option<&str>,
) -> Result<Option<VerificationReport>, CoreError> {
    Ok(launch_plan(session_id, target, capsule_path)?.map(|plan| plan.verification))
}

pub fn execute_launch(
    session_id: Option<&str>,
    target: CliTool,
    capsule_path: Option<&str>,
) -> Result<Option<LaunchExecution>, CoreError> {
    require_explicit_session(session_id, "target handoff")?;
    let Some(plan) = launch_plan(session_id, target, capsule_path)? else {
        return Ok(None);
    };
    launcher::execute_plan(plan).map(Some)
}

fn require_explicit_session(
    session_id: Option<&str>,
    action: &'static str,
) -> Result<(), CoreError> {
    if session_id.is_some_and(|session_id| !session_id.trim().is_empty()) {
        return Ok(());
    }
    Err(CoreError::ExecuteRequiresSession { action })
}

fn selected_session(session_id: Option<&str>) -> Result<Option<SessionSummary>, CoreError> {
    if let Some(session_id) = session_id {
        find_session(session_id)
    } else {
        default_session()
    }
}

fn capsule_for_plan(
    generated: &WorkCapsule,
    capsule_path: Option<&str>,
) -> Result<(WorkCapsule, Option<String>), CoreError> {
    let Some(path) = capsule_path else {
        return Ok((generated.clone(), None));
    };
    let contents = fs::read_to_string(path).map_err(|error| CoreError::CapsuleRead {
        path: path.into(),
        reason: error.to_string(),
    })?;
    let capsule = serde_json::from_str::<WorkCapsule>(&contents).map_err(|error| {
        CoreError::CapsuleParse {
            path: path.into(),
            reason: error.to_string(),
        }
    })?;
    Ok((capsule, Some(path.into())))
}

pub fn moonbox_execute_command(
    target: CliTool,
    session_id: &str,
    capsule_path: Option<&str>,
) -> String {
    let base = format!(
        "moonbox launch --execute --target {} --session {}",
        target.id(),
        session_id
    );
    if let Some(capsule_path) = capsule_path {
        format!("{base} --capsule {capsule_path}")
    } else {
        base
    }
}

pub fn moonbox_open_execute_command(session_id: &str) -> String {
    format!("moonbox open --execute --session {session_id}")
}

#[cfg(test)]
mod tests {
    use std::{env, fs};

    use super::*;
    use crate::core::model::VerificationStatus;

    #[test]
    fn default_launch_plan_uses_generated_capsule_without_fake_path() {
        let plan = launch_plan(Some("codex-cxcp-design"), CliTool::Hermes, None)
            .expect("plan result")
            .expect("plan");

        assert_eq!(plan.capsule_path, None);
        assert!(!plan.command.contains("--capsule"));
        assert!(plan.command.starts_with("hermes chat "));
        assert_eq!(plan.verification.status, VerificationStatus::Pass);
    }

    #[test]
    fn open_plan_uses_structured_original_command() {
        let plan = open_plan(Some("codex-cxcp-design"))
            .expect("open plan result")
            .expect("open plan");

        assert!(plan.dry_run);
        assert_eq!(plan.command.program, "codex");
        assert_eq!(plan.command.args, ["resume", "codex-cxcp-design"]);
        assert_eq!(plan.command.display, "codex resume codex-cxcp-design");
    }

    #[test]
    fn execute_open_requires_explicit_session() {
        let error = execute_open(None).expect_err("implicit execute should be blocked");

        assert!(matches!(error, CoreError::ExecuteRequiresSession { .. }));
        assert!(error.to_string().contains("explicit --session"));
    }

    #[test]
    fn execute_launch_requires_explicit_session() {
        let error = execute_launch(None, CliTool::Hermes, None)
            .expect_err("implicit launch execute should be blocked");

        assert!(matches!(error, CoreError::ExecuteRequiresSession { .. }));
        assert!(error.to_string().contains("newest active session"));
    }

    #[test]
    fn launch_plan_errors_for_missing_capsule_file() {
        let error = launch_plan(
            Some("codex-cxcp-design"),
            CliTool::Hermes,
            Some("/tmp/moonbox-missing-capsule-for-test.json"),
        )
        .expect_err("missing capsule should error");

        assert!(matches!(error, CoreError::CapsuleRead { .. }));
    }

    #[test]
    fn verify_launch_fails_when_capsule_target_mismatches_requested_target() {
        let path = env::temp_dir().join(format!(
            "moonbox-target-mismatch-{}.json",
            std::process::id()
        ));
        let capsule = capsule(CliTool::Codex, CliTool::Hermes).expect("capsule");
        fs::write(&path, serde_json::to_string_pretty(&capsule).expect("json"))
            .expect("write capsule");

        let report = verify_launch(
            Some("codex-cxcp-design"),
            CliTool::Codex,
            Some(path.to_str().expect("utf-8 path")),
        )
        .expect("verify result")
        .expect("report");

        assert_eq!(report.status, VerificationStatus::Fail);
        assert!(report
            .checks
            .iter()
            .any(|check| check.name == "target_cli" && check.status == VerificationStatus::Fail));
    }

    #[test]
    fn execute_launch_rejects_failed_verification_before_spawning_target() {
        let path = env::temp_dir().join(format!(
            "moonbox-target-execute-mismatch-{}.json",
            std::process::id()
        ));
        let capsule = capsule(CliTool::Codex, CliTool::Hermes).expect("capsule");
        fs::write(&path, serde_json::to_string_pretty(&capsule).expect("json"))
            .expect("write capsule");

        let error = execute_launch(
            Some("codex-cxcp-design"),
            CliTool::Codex,
            Some(path.to_str().expect("utf-8 path")),
        )
        .expect_err("blocked launch");

        assert!(matches!(error, CoreError::LaunchBlocked { .. }));
    }
}
