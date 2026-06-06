use std::fs;

use super::{
    data,
    error::CoreError,
    model::{
        CapsuleCompileOutput, CapsuleCompileRequest, CliTool, LaunchPlan, SessionSummary,
        VerificationReport, WorkCapsule, WorkbenchData,
    },
    verifier,
};

pub fn load_workbench(source: CliTool, target: CliTool) -> Result<WorkbenchData, CoreError> {
    data::workbench_data(source, target)
}

pub fn load_workbench_for_session(
    session_id: &str,
    target: CliTool,
) -> Result<Option<WorkbenchData>, CoreError> {
    data::workbench_data_for_session(session_id, target)
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
    let session = selected_session(session_id)?;
    Ok(session.map(|session| session.resume_command))
}

pub fn capsule(source: CliTool, target: CliTool) -> Result<WorkCapsule, CoreError> {
    Ok(load_workbench(source, target)?.capsule)
}

pub fn compile_request(
    source: CliTool,
    target: CliTool,
    rewind_event_id: &str,
) -> Result<CapsuleCompileRequest, CoreError> {
    data::compile_request(source, target, rewind_event_id)
}

pub fn compile_output(source: CliTool, target: CliTool) -> Result<CapsuleCompileOutput, CoreError> {
    data::compile_output(source, target)
}

pub fn launch_plan(
    session_id: Option<&str>,
    target: CliTool,
    capsule_path: Option<&str>,
) -> Result<Option<LaunchPlan>, CoreError> {
    let Some(source_session) = selected_session(session_id)? else {
        return Ok(None);
    };
    let Some(data) = load_workbench_for_session(&source_session.id, target)? else {
        return Ok(None);
    };
    let (capsule, capsule_path) = capsule_for_plan(&data.capsule, capsule_path)?;
    let command = launch_command(target, &source_session.id, capsule_path.as_deref());
    let verification = verifier::verify_capsule(&capsule, &source_session, &data.timeline, target);

    Ok(Some(LaunchPlan {
        version: 1,
        dry_run: true,
        source_session,
        target_cli: target,
        target_branch: capsule.target_branch,
        capsule_path,
        command,
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

fn launch_command(target: CliTool, session_id: &str, capsule_path: Option<&str>) -> String {
    let base = format!(
        "moonbox launch --target {} --session {}",
        target.id(),
        session_id
    );
    if let Some(capsule_path) = capsule_path {
        format!("{base} --capsule {capsule_path}")
    } else {
        base
    }
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
        assert_eq!(plan.verification.status, VerificationStatus::Pass);
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
}
