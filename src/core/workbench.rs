use super::{
    demo,
    model::{
        CapsuleCompileOutput, CapsuleCompileRequest, CliTool, DemoData, LaunchPlan, SessionSummary,
        VerificationReport, WorkCapsule,
    },
    verifier,
};

pub fn load_demo_workbench(source: CliTool, target: CliTool) -> DemoData {
    demo::demo_data(source, target)
}

pub fn load_demo_workbench_for_session(session_id: &str, target: CliTool) -> Option<DemoData> {
    demo::demo_data_for_session(session_id, target)
}

pub fn list_sessions() -> Vec<SessionSummary> {
    demo::demo_sessions()
}

pub fn find_session(session_id: &str) -> Option<SessionSummary> {
    list_sessions()
        .into_iter()
        .find(|session| session.id == session_id)
}

pub fn default_session() -> Option<SessionSummary> {
    list_sessions().into_iter().next()
}

pub fn open_command(session_id: Option<&str>) -> Option<String> {
    let session = session_id.and_then(find_session).or_else(default_session)?;
    Some(session.resume_command)
}

pub fn capsule(source: CliTool, target: CliTool) -> WorkCapsule {
    load_demo_workbench(source, target).capsule
}

pub fn compile_request(
    source: CliTool,
    target: CliTool,
    rewind_event_id: &str,
) -> CapsuleCompileRequest {
    demo::demo_compile_request(source, target, rewind_event_id)
}

pub fn compile_output(source: CliTool, target: CliTool) -> CapsuleCompileOutput {
    demo::demo_compile_output(source, target)
}

pub fn launch_plan(
    session_id: Option<&str>,
    target: CliTool,
    capsule_path: Option<&str>,
) -> Option<LaunchPlan> {
    let source_session = session_id.and_then(find_session).or_else(default_session)?;
    let data = load_demo_workbench_for_session(&source_session.id, target)?;
    let capsule_path = capsule_path
        .map(str::to_string)
        .unwrap_or_else(|| capsule_path_for_rewind(&data.capsule.rewind_point));
    let command = format!(
        "moonbox launch --target {} --session {} --capsule {}",
        target.id(),
        source_session.id,
        capsule_path
    );
    let verification = verifier::verify_capsule(&data.capsule, &source_session, &data.timeline);

    Some(LaunchPlan {
        version: 1,
        dry_run: true,
        source_session,
        target_cli: target,
        target_branch: data.capsule.target_branch,
        capsule_path,
        command,
        verification,
    })
}

pub fn verify_launch(session_id: Option<&str>, target: CliTool) -> Option<VerificationReport> {
    launch_plan(session_id, target, None).map(|plan| plan.verification)
}

fn capsule_path_for_rewind(rewind_point: &str) -> String {
    let rewind_id = rewind_point.split_whitespace().next().unwrap_or("evt-000");
    format!("~/.moonbox/capsules/{rewind_id}.json")
}
