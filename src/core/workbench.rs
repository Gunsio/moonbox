use super::{
    demo,
    model::{
        CapsuleCompileOutput, CapsuleCompileRequest, CliTool, DemoData, SessionSummary, WorkCapsule,
    },
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
