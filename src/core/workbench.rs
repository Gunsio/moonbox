use std::{fs, path::Path};

use super::{
    actions::{self, SessionActionContext, SessionActionSet},
    capsule_store::{
        self, CapsuleExportEnvelope, CapsuleImportResult, CapsuleRecord, CapsuleSummary,
    },
    compiler, continuation, data, dataspace,
    error::CoreError,
    launch_ledger, launcher,
    model::{
        AppOpenPlan, CapsuleCompileOutput, CapsuleCompileRequest, CliTool, ContinuationOptions,
        LaunchExecution, LaunchPlan, OriginalSessionExecution, OriginalSessionPlan, SessionAction,
        SessionSummary, VerificationReport, WorkCapsule, WorkbenchData,
    },
    redaction, sources, verifier,
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

pub fn load_workbench_for_data_space(
    space: &dataspace::DataSpaceEntry,
    source: super::model::CliTool,
    target: super::model::CliTool,
) -> Result<WorkbenchData, CoreError> {
    dataspace::load_workbench_for_space(space, source, target)
}

pub fn load_remote_workbench_from_session_snapshot(
    space: &dataspace::DataSpaceEntry,
    source_session: SessionSummary,
    sessions: Vec<SessionSummary>,
    target: super::model::CliTool,
) -> Result<WorkbenchData, CoreError> {
    dataspace::load_remote_workbench_for_session(space, source_session, sessions, target)
}

pub fn list_sessions() -> Result<Vec<SessionSummary>, CoreError> {
    data::sessions()
}

pub fn search_hermes_sessions(
    query: &str,
    point_limit: usize,
) -> Result<Vec<SessionSummary>, CoreError> {
    sources::search_hermes_sessions(query, point_limit)
}

pub fn find_session(session_id: &str) -> Result<Option<SessionSummary>, CoreError> {
    data::find_session(session_id)
}

pub fn default_session() -> Result<Option<SessionSummary>, CoreError> {
    Ok(list_sessions()?.into_iter().next())
}

pub fn session_actions(session_id: Option<&str>) -> Result<Option<SessionActionSet>, CoreError> {
    let Some(source_session) = selected_session(session_id)? else {
        return Ok(None);
    };
    Ok(Some(actions::session_action_set(
        &source_session,
        &SessionActionContext::local_without_live(),
    )))
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

pub fn open_app_plan(session_id: Option<&str>) -> Result<Option<AppOpenPlan>, CoreError> {
    let Some(source_session) = selected_session(session_id)? else {
        return Ok(None);
    };
    let (supported, deep_link, reason) = if source_session.cli == CliTool::Codex {
        (
            true,
            Some(super::codex_app_server::CodexAppServerSource::deep_link(
                &source_session.id,
            )),
            "Codex desktop app deep link preview; Moonbox does not launch the app".into(),
        )
    } else {
        (
            false,
            None,
            format!(
                "{} does not have a verified provider app deep-link contract",
                source_session.cli
            ),
        )
    };
    Ok(Some(AppOpenPlan {
        version: 1,
        action: SessionAction::AppDeepLink,
        dry_run: true,
        source_session,
        supported,
        deep_link,
        reason,
    }))
}

pub fn execute_open(
    session_id: Option<&str>,
) -> Result<Option<OriginalSessionExecution>, CoreError> {
    require_explicit_session(session_id, "original resume")?;
    let Some(plan) = open_plan(session_id)? else {
        return Ok(None);
    };
    match launcher::execute_original_plan(plan.clone()) {
        Ok(mut execution) => {
            attach_original_launch_ledger(&mut execution);
            Ok(Some(execution))
        }
        Err(error) => {
            warn_if_launch_ledger_fails(launch_ledger::record_original_failed(&plan, &error));
            Err(error)
        }
    }
}

pub fn capsule_for_selection(
    session_id: Option<&str>,
    target: CliTool,
    rewind_event_id: Option<&str>,
    compiler: Option<&str>,
) -> Result<Option<WorkCapsule>, CoreError> {
    let Some(source_session) = selected_session(session_id)? else {
        return Ok(None);
    };
    let compiler = selected_compiler(compiler);
    data::compile_output_for_session_id(&source_session.id, target, rewind_event_id, &compiler)
        .map(|output| output.map(|output| output.capsule))
}

pub fn compile_request_for_selection(
    session_id: Option<&str>,
    target: CliTool,
    rewind_event_id: Option<&str>,
    compiler: Option<&str>,
) -> Result<Option<CapsuleCompileRequest>, CoreError> {
    let Some(source_session) = selected_session(session_id)? else {
        return Ok(None);
    };
    let compiler = selected_compiler(compiler);
    data::compile_request_for_session_id(&source_session.id, target, rewind_event_id, &compiler)
}

pub fn compile_output_for_selection(
    session_id: Option<&str>,
    target: CliTool,
    rewind_event_id: Option<&str>,
    compiler: Option<&str>,
) -> Result<Option<CapsuleCompileOutput>, CoreError> {
    let Some(source_session) = selected_session(session_id)? else {
        return Ok(None);
    };
    let compiler = selected_compiler(compiler);
    data::compile_output_for_session_id(&source_session.id, target, rewind_event_id, &compiler)
}

pub fn compile_capsule(
    session_id: &str,
    target: CliTool,
    rewind_event_id: &str,
    compiler: &str,
) -> Result<Option<WorkCapsule>, CoreError> {
    data::compile_capsule_for_session_id(session_id, target, rewind_event_id, compiler)
}

pub fn compile_capsule_from_workbench_snapshot(
    workbench: &WorkbenchData,
    session_id: &str,
    target: CliTool,
    rewind_event_id: &str,
    compiler: &str,
) -> Result<Option<WorkCapsule>, CoreError> {
    let Some(source_session) = workbench
        .sessions
        .iter()
        .find(|session| session.id == session_id)
        .cloned()
    else {
        return Ok(None);
    };
    data::compile_capsule_from_timeline_snapshot(
        source_session,
        target,
        super::model::CanonicalTimeline {
            version: 1,
            source_cli: workbench.source,
            source_session: session_id.into(),
            events: workbench.timeline.clone(),
        },
        rewind_event_id,
        compiler,
    )
    .map(Some)
}

pub fn save_capsule_for_selection(
    name: &str,
    session_id: Option<&str>,
    target: CliTool,
    rewind_event_id: Option<&str>,
    compiler: Option<&str>,
) -> Result<Option<CapsuleRecord>, CoreError> {
    let Some(capsule) = capsule_for_selection(session_id, target, rewind_event_id, compiler)?
    else {
        return Ok(None);
    };
    capsule_store::save_capsule(name, &capsule).map(Some)
}

pub fn list_saved_capsules() -> Result<Vec<CapsuleSummary>, CoreError> {
    capsule_store::list_capsules()
}

pub fn show_saved_capsule(name: &str) -> Result<Option<CapsuleRecord>, CoreError> {
    capsule_store::show_capsule(name)
}

pub fn delete_saved_capsule(name: &str) -> Result<bool, CoreError> {
    capsule_store::delete_capsule(name)
}

pub fn export_saved_capsule(name: &str) -> Result<CapsuleExportEnvelope, CoreError> {
    capsule_store::export_capsule(name)
}

pub fn write_saved_capsule_export(
    name: &str,
    path: &Path,
) -> Result<CapsuleExportEnvelope, CoreError> {
    let envelope = export_saved_capsule(name)?;
    capsule_store::write_export_file(path, &envelope)?;
    Ok(envelope)
}

pub fn import_saved_capsule(
    path: &Path,
    name_override: Option<&str>,
) -> Result<CapsuleImportResult, CoreError> {
    let envelope = capsule_store::read_export_file(path)?;
    capsule_store::import_capsule(envelope, name_override)
}

pub fn saved_capsule_launch_plan(
    name: &str,
    target: Option<CliTool>,
    continuation_options: ContinuationOptions,
) -> Result<LaunchPlan, CoreError> {
    let record = capsule_store::show_capsule(name)?.ok_or_else(|| CoreError::CapsuleStore {
        reason: format!("capsule {name} was not found"),
    })?;
    let target = target.unwrap_or(record.capsule.target_cli);
    let Some((source_session, timeline)) =
        data::launch_context_for_session_id(&record.capsule.source_session)?
    else {
        return Err(CoreError::CapsuleStore {
            reason: format!(
                "source session {} for capsule {name} was not found",
                record.capsule.source_session
            ),
        });
    };
    let capsule_path = Some(format!("store:{name}"));
    let continuation = continuation::build_continuation_protocol(
        &source_session,
        target,
        &record.capsule,
        capsule_path.as_deref(),
        continuation_options,
    );
    let target_command = launcher::target_command_with_continuation(
        target,
        &source_session,
        &record.capsule,
        &continuation,
    )?;
    let command = target_command.display.clone();
    let verification = verifier::verify_capsule_with_continuation(
        &record.capsule,
        &source_session,
        &timeline.events,
        target,
        &continuation,
    );

    Ok(LaunchPlan {
        version: 1,
        action: SessionAction::TargetHandoff,
        dry_run: true,
        source_session,
        target_cli: target,
        compiler: record.capsule.compiler,
        handoff_label: record.capsule.handoff_label,
        rewind_point: record.capsule.rewind_point,
        capsule_path,
        command,
        target_command,
        verification,
        continuation,
    })
}

pub fn execute_saved_capsule_launch(
    name: &str,
    target: Option<CliTool>,
    allow_draft: bool,
    continuation_options: ContinuationOptions,
) -> Result<LaunchExecution, CoreError> {
    let plan = saved_capsule_launch_plan(name, target, continuation_options)?;
    match launcher::execute_plan(plan.clone(), allow_draft) {
        Ok(mut execution) => {
            attach_target_launch_ledger(&mut execution, Some(name));
            Ok(execution)
        }
        Err(error) => {
            warn_if_launch_ledger_fails(launch_ledger::record_target_blocked(
                &plan,
                Some(name),
                &error,
            ));
            Err(error)
        }
    }
}

pub fn launch_plan(
    session_id: Option<&str>,
    target: CliTool,
    capsule_path: Option<&str>,
) -> Result<Option<LaunchPlan>, CoreError> {
    launch_plan_with_options(
        session_id,
        target,
        capsule_path,
        ContinuationOptions::default(),
    )
}

pub fn launch_plan_with_options(
    session_id: Option<&str>,
    target: CliTool,
    capsule_path: Option<&str>,
    continuation_options: ContinuationOptions,
) -> Result<Option<LaunchPlan>, CoreError> {
    let Some(source_session) = selected_session(session_id)? else {
        return Ok(None);
    };
    let (source_session, timeline, capsule, capsule_path) = if let Some(path) = capsule_path {
        let Some((source_session, timeline)) =
            data::launch_context_for_session_id(&source_session.id)?
        else {
            return Ok(None);
        };
        (
            source_session,
            timeline,
            read_capsule_for_plan(path)?,
            Some(path.into()),
        )
    } else {
        let Some((source_session, timeline, capsule)) =
            data::launch_artifacts_for_session_id(&source_session.id, target)?
        else {
            return Ok(None);
        };
        (source_session, timeline, capsule, None)
    };
    let continuation = continuation::build_continuation_protocol(
        &source_session,
        target,
        &capsule,
        capsule_path.as_deref(),
        continuation_options,
    );
    let target_command = launcher::target_command_with_continuation(
        target,
        &source_session,
        &capsule,
        &continuation,
    )?;
    let command = target_command.display.clone();
    let verification = verifier::verify_capsule_with_continuation(
        &capsule,
        &source_session,
        &timeline.events,
        target,
        &continuation,
    );

    Ok(Some(LaunchPlan {
        version: 1,
        action: SessionAction::TargetHandoff,
        dry_run: true,
        source_session,
        target_cli: target,
        compiler: capsule.compiler.clone(),
        handoff_label: capsule.handoff_label,
        rewind_point: capsule.rewind_point.clone(),
        capsule_path,
        command,
        target_command,
        verification,
        continuation,
    }))
}

pub fn verify_launch(
    session_id: Option<&str>,
    target: CliTool,
    capsule_path: Option<&str>,
) -> Result<Option<VerificationReport>, CoreError> {
    Ok(launch_plan(session_id, target, capsule_path)?.map(|plan| plan.verification))
}

pub fn verify_launch_with_options(
    session_id: Option<&str>,
    target: CliTool,
    capsule_path: Option<&str>,
    continuation_options: ContinuationOptions,
) -> Result<Option<VerificationReport>, CoreError> {
    Ok(
        launch_plan_with_options(session_id, target, capsule_path, continuation_options)?
            .map(|plan| plan.verification),
    )
}

#[cfg(test)]
pub fn execute_launch(
    session_id: Option<&str>,
    target: CliTool,
    capsule_path: Option<&str>,
    allow_draft: bool,
) -> Result<Option<LaunchExecution>, CoreError> {
    execute_launch_with_options(
        session_id,
        target,
        capsule_path,
        allow_draft,
        ContinuationOptions::default(),
    )
}

pub fn execute_launch_with_options(
    session_id: Option<&str>,
    target: CliTool,
    capsule_path: Option<&str>,
    allow_draft: bool,
    continuation_options: ContinuationOptions,
) -> Result<Option<LaunchExecution>, CoreError> {
    require_explicit_session(session_id, "target handoff")?;
    let Some(plan) =
        launch_plan_with_options(session_id, target, capsule_path, continuation_options)?
    else {
        return Ok(None);
    };
    match launcher::execute_plan(plan.clone(), allow_draft) {
        Ok(mut execution) => {
            attach_target_launch_ledger(&mut execution, None);
            Ok(Some(execution))
        }
        Err(error) => {
            warn_if_launch_ledger_fails(launch_ledger::record_target_blocked(&plan, None, &error));
            Err(error)
        }
    }
}

pub fn execute_tui_launch_plan(plan: LaunchPlan) -> Result<LaunchExecution, CoreError> {
    match launcher::execute_plan(plan.clone(), false) {
        Ok(mut execution) => {
            attach_target_launch_ledger(&mut execution, None);
            Ok(execution)
        }
        Err(error) => {
            warn_if_launch_ledger_fails(launch_ledger::record_target_blocked(&plan, None, &error));
            Err(error)
        }
    }
}

pub fn list_launches(limit: usize) -> Result<Vec<launch_ledger::LaunchRecord>, CoreError> {
    launch_ledger::list_launches(limit)
}

pub fn show_launch(id: i64) -> Result<Option<launch_ledger::LaunchRecord>, CoreError> {
    launch_ledger::show_launch(id)
}

pub fn link_launch_to_capsule(
    id: i64,
    capsule_name: &str,
) -> Result<launch_ledger::LaunchRecord, CoreError> {
    launch_ledger::link_launch_to_capsule(id, capsule_name)
}

pub fn list_capsule_launches(
    capsule_name: &str,
    limit: usize,
) -> Result<Vec<launch_ledger::LaunchRecord>, CoreError> {
    launch_ledger::list_capsule_launches(capsule_name, limit)
}

fn attach_target_launch_ledger(execution: &mut LaunchExecution, capsule_name: Option<&str>) {
    match launch_ledger::record_target_execution(execution, capsule_name) {
        Ok(record) => execution.launch_ledger = Some(record.link()),
        Err(error) => {
            execution.launch_ledger_warning = Some(format!("launch ledger write failed: {error}"));
        }
    }
}

fn attach_original_launch_ledger(execution: &mut OriginalSessionExecution) {
    match launch_ledger::record_original_execution(execution) {
        Ok(record) => execution.launch_ledger = Some(record.link()),
        Err(error) => {
            execution.launch_ledger_warning = Some(format!("launch ledger write failed: {error}"));
        }
    }
}

fn warn_if_launch_ledger_fails(result: Result<launch_ledger::LaunchRecord, CoreError>) {
    if let Err(error) = result {
        eprintln!("WARN: launch ledger write failed: {error}");
    }
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

fn selected_compiler(compiler_id: Option<&str>) -> String {
    compiler_id
        .filter(|compiler_id| !compiler_id.trim().is_empty())
        .map(str::to_owned)
        .unwrap_or_else(compiler::default_compiler_id)
}

fn read_capsule_for_plan(path: &str) -> Result<WorkCapsule, CoreError> {
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
    Ok(redaction::redact_work_capsule_for_export(capsule))
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
        assert_eq!(plan.verification.status, VerificationStatus::Warn);
        assert!(plan.verification.ready);
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
    fn open_app_plan_previews_codex_deep_link_without_execute_path() {
        let plan = open_app_plan(Some("codex-cxcp-design"))
            .expect("app open plan result")
            .expect("app open plan");

        assert!(plan.dry_run);
        assert_eq!(plan.action, SessionAction::AppDeepLink);
        assert!(plan.supported);
        assert_eq!(
            plan.deep_link.as_deref(),
            Some("codex://threads/codex-cxcp-design")
        );
        assert!(plan.reason.contains("does not launch"));
    }

    #[test]
    fn open_app_plan_returns_unsupported_provider_plan() {
        let plan = open_app_plan(Some("claude-qc-platform"))
            .expect("app open plan result")
            .expect("app open plan");

        assert!(plan.dry_run);
        assert_eq!(plan.action, SessionAction::AppDeepLink);
        assert!(!plan.supported);
        assert_eq!(plan.deep_link, None);
        assert!(plan.reason.contains("deep-link contract"));
    }

    #[test]
    fn compile_surfaces_accept_explicit_session_target_rewind_and_compiler() {
        let request = compile_request_for_selection(
            Some("claude-qc-platform"),
            CliTool::Codex,
            Some("evt-074"),
            Some("engineering-handoff"),
        )
        .expect("request result")
        .expect("request");

        assert_eq!(request.source_cli, CliTool::Claude);
        assert_eq!(request.target_cli, CliTool::Codex);
        assert_eq!(request.source_session.id, "claude-qc-platform");
        assert_eq!(request.rewind_event_id, "evt-074");
        assert_eq!(request.compiler, "engineering-handoff");

        let capsule = capsule_for_selection(
            Some("claude-qc-platform"),
            CliTool::Codex,
            Some("evt-074"),
            Some("engineering-handoff"),
        )
        .expect("capsule result")
        .expect("capsule");

        assert_eq!(capsule.source_cli, CliTool::Claude);
        assert_eq!(capsule.target_cli, CliTool::Codex);
        assert_eq!(capsule.source_session, "claude-qc-platform");
        assert!(capsule.rewind_point.contains("evt-074"));
    }

    #[test]
    fn execute_open_requires_explicit_session() {
        let error = execute_open(None).expect_err("implicit execute should be blocked");

        assert!(matches!(error, CoreError::ExecuteRequiresSession { .. }));
        assert!(error.to_string().contains("explicit --session"));
    }

    #[test]
    fn execute_launch_requires_explicit_session() {
        let error = execute_launch(None, CliTool::Hermes, None, false)
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
        let capsule = capsule_for_selection(
            Some("codex-cxcp-design"),
            CliTool::Hermes,
            Some("evt-091"),
            None,
        )
        .expect("capsule result")
        .expect("capsule");
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
        let capsule = capsule_for_selection(
            Some("codex-cxcp-design"),
            CliTool::Hermes,
            Some("evt-091"),
            None,
        )
        .expect("capsule result")
        .expect("capsule");
        fs::write(&path, serde_json::to_string_pretty(&capsule).expect("json"))
            .expect("write capsule");

        let error = execute_launch(
            Some("codex-cxcp-design"),
            CliTool::Codex,
            Some(path.to_str().expect("utf-8 path")),
            false,
        )
        .expect_err("blocked launch");

        assert!(matches!(error, CoreError::LaunchBlocked { .. }));
    }
}
