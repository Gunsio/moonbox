use super::{
    compiler::{
        compile_with_configured_runner, compiler_catalog, default_compiler_id,
        default_rewind_event_id,
    },
    error::CoreError,
    model::{
        BranchNode, CanonicalTimeline, CapsuleCompileOutput, CapsuleCompileRequest, CliTool,
        SessionStatus, SessionSummary, TimelineEvent, WorkCapsule, WorkbenchData,
    },
    sources,
};

#[cfg(test)]
use super::fixture::FixtureSourceAdapter;

pub fn workbench_data(source: CliTool, target: CliTool) -> Result<WorkbenchData, CoreError> {
    let sessions = sessions()?;
    let source_session = sessions
        .iter()
        .find(|session| session.cli == source)
        .cloned()
        .unwrap_or_else(|| fallback_session(source));
    let source_session_id = source_session.id.clone();
    if let Some(data) = workbench_data_for_session(&source_session_id, target)? {
        return Ok(data);
    }

    let timeline = CanonicalTimeline {
        version: 1,
        source_cli: source,
        source_session: source_session_id.clone(),
        events: Vec::new(),
    };
    let rewind_event_id = rewind_event_id_for_timeline(&source_session_id, &timeline);
    let compiler = default_compiler_id();
    let capsule = compile_capsule_for_session(
        &source_session,
        target,
        &timeline,
        &rewind_event_id,
        &compiler,
    )?;
    Ok(build_workbench_data(
        source,
        target,
        sessions,
        timeline.events,
        capsule,
        &source_session_id,
    ))
}

pub fn workbench_data_for_session(
    session_id: &str,
    target: CliTool,
) -> Result<Option<WorkbenchData>, CoreError> {
    let sessions = sessions()?;
    let source_session = if let Some(session) = sessions
        .iter()
        .find(|session| session.id == session_id)
        .cloned()
    {
        Some(session)
    } else {
        find_session(session_id)?
    };
    let Some(source_session) = source_session else {
        return Ok(None);
    };
    let sessions = include_source_session(sessions, &source_session);
    let source_session_id = source_session.id.clone();
    let timeline = canonical_timeline_for_session(&source_session)?;
    let rewind_event_id = rewind_event_id_for_timeline(&source_session_id, &timeline);
    let compiler = default_compiler_id();
    let capsule = compile_capsule_for_session(
        &source_session,
        target,
        &timeline,
        &rewind_event_id,
        &compiler,
    )?;
    Ok(Some(build_workbench_data(
        source_session.cli,
        target,
        sessions,
        timeline.events,
        capsule,
        &source_session_id,
    )))
}

fn build_workbench_data(
    source: CliTool,
    target: CliTool,
    sessions: Vec<SessionSummary>,
    timeline: Vec<TimelineEvent>,
    capsule: WorkCapsule,
    source_session_id: &str,
) -> WorkbenchData {
    let rewind_id = capsule
        .rewind_point
        .split_whitespace()
        .next()
        .unwrap_or("evt-000")
        .to_string();
    let branches = vec![
        BranchNode {
            id: "root".into(),
            label: format!("original/{source_session_id}"),
            detail: "original session, read-only".into(),
            active: false,
        },
        BranchNode {
            id: rewind_id.clone(),
            label: format!("rewind/{rewind_id}"),
            detail: capsule.rewind_point.clone(),
            active: false,
        },
        BranchNode {
            id: "target".into(),
            label: capsule.target_branch.clone(),
            detail: "compiled by engineering-handoff".into(),
            active: true,
        },
    ];

    WorkbenchData {
        source,
        target,
        sessions,
        timeline,
        capsule,
        branches,
        compilers: compiler_catalog(),
    }
}

fn include_source_session(
    mut sessions: Vec<SessionSummary>,
    source_session: &SessionSummary,
) -> Vec<SessionSummary> {
    if !sessions
        .iter()
        .any(|session| session.id == source_session.id)
    {
        sessions.insert(0, source_session.clone());
    }
    sessions
}

#[cfg(test)]
pub type FixtureTestSourceAdapter = FixtureSourceAdapter;

pub fn sessions() -> Result<Vec<SessionSummary>, CoreError> {
    sources::list_sessions()
}

pub fn find_session(session_id: &str) -> Result<Option<SessionSummary>, CoreError> {
    sources::find_session(session_id)
}

pub fn canonical_timeline_for_session(
    session: &SessionSummary,
) -> Result<CanonicalTimeline, CoreError> {
    sources::load_timeline(session)
}

pub fn compile_request(
    source: CliTool,
    target: CliTool,
    rewind_event_id: &str,
) -> Result<CapsuleCompileRequest, CoreError> {
    compile_request_with_compiler(source, target, rewind_event_id, &default_compiler_id())
}

pub fn compile_request_with_compiler(
    source: CliTool,
    target: CliTool,
    rewind_event_id: &str,
    compiler: &str,
) -> Result<CapsuleCompileRequest, CoreError> {
    let source_session = default_source_session(source)?;
    let timeline = timeline_for_compile_request(&source_session)?;
    Ok(CapsuleCompileRequest {
        version: 1,
        source_cli: source,
        target_cli: target,
        source_session,
        rewind_event_id: rewind_event_id.into(),
        token_budget: 100_000,
        compiler: compiler.into(),
        timeline: CanonicalTimeline {
            version: 1,
            source_cli: source,
            source_session: timeline.source_session,
            events: timeline.events,
        },
    })
}

pub fn compile_output(source: CliTool, target: CliTool) -> Result<CapsuleCompileOutput, CoreError> {
    compile_output_with_compiler(source, target, &default_compiler_id())
}

pub fn compile_output_with_compiler(
    source: CliTool,
    target: CliTool,
    compiler: &str,
) -> Result<CapsuleCompileOutput, CoreError> {
    let source_session = default_source_session(source)?;
    let timeline = timeline_for_compile_request(&source_session)?;
    let rewind_event_id = rewind_event_id_for_timeline(&source_session.id, &timeline);
    let request = CapsuleCompileRequest {
        version: 1,
        source_cli: source,
        target_cli: target,
        source_session,
        rewind_event_id,
        token_budget: 100_000,
        compiler: compiler.into(),
        timeline,
    };
    compile_with_configured_runner(&request).map_err(CoreError::from)
}

pub fn compile_capsule_for_session_id(
    session_id: &str,
    target: CliTool,
    rewind_event_id: &str,
    compiler: &str,
) -> Result<Option<WorkCapsule>, CoreError> {
    let Some(source_session) = find_session(session_id)? else {
        return Ok(None);
    };
    let timeline = canonical_timeline_for_session(&source_session)?;
    compile_capsule_for_session(
        &source_session,
        target,
        &timeline,
        rewind_event_id,
        compiler,
    )
    .map(Some)
}

fn compile_capsule_for_session(
    session: &SessionSummary,
    target: CliTool,
    timeline: &CanonicalTimeline,
    rewind_event_id: &str,
    compiler: &str,
) -> Result<WorkCapsule, CoreError> {
    let request = CapsuleCompileRequest {
        version: 1,
        source_cli: session.cli,
        target_cli: target,
        source_session: session.clone(),
        rewind_event_id: rewind_event_id.into(),
        token_budget: 100_000,
        compiler: compiler.into(),
        timeline: timeline.clone(),
    };
    Ok(compile_with_configured_runner(&request)?.capsule)
}

fn rewind_event_id_for_timeline(session_id: &str, timeline: &CanonicalTimeline) -> String {
    let preferred = default_rewind_event_id(session_id);
    if timeline.events.iter().any(|event| event.id == preferred) {
        return preferred.into();
    }
    timeline
        .events
        .last()
        .map(|event| event.id.clone())
        .unwrap_or_else(|| preferred.into())
}

fn default_source_session(source: CliTool) -> Result<SessionSummary, CoreError> {
    Ok(sessions()?
        .into_iter()
        .find(|session| session.cli == source)
        .unwrap_or_else(|| fallback_session(source)))
}

fn timeline_for_compile_request(
    source_session: &SessionSummary,
) -> Result<CanonicalTimeline, CoreError> {
    if source_session.event_count == 0 {
        return Ok(CanonicalTimeline {
            version: 1,
            source_cli: source_session.cli,
            source_session: source_session.id.clone(),
            events: Vec::new(),
        });
    }
    canonical_timeline_for_session(source_session)
}

fn fallback_session(source: CliTool) -> SessionSummary {
    SessionSummary {
        id: format!("{}-session", source.id()),
        cli: source,
        title: "Synthetic session".into(),
        cwd: "~".into(),
        updated_at: "1970-01-01T00:00:00+00:00".into(),
        updated: "unknown".into(),
        status: SessionStatus::Warning,
        branch: None,
        token_count: None,
        health_reason: Some("synthetic fallback".into()),
        event_count: 0,
        resume_command: format!("{} resume {}-session", source.id(), source.id()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::adapter::SourceAdapter;

    #[test]
    fn sessions_are_adapter_collected_and_time_sorted() {
        let sessions = sessions().expect("sessions");

        assert_eq!(sessions.len(), 3);
        assert_eq!(sessions[0].id, "codex-cxcp-design");
        assert_eq!(sessions[1].id, "claude-qc-platform");
        assert_eq!(sessions[2].id, "hermes-cxcp-502");
    }

    #[test]
    fn fixture_adapter_returns_canonical_timeline() {
        let adapter = FixtureTestSourceAdapter::new(CliTool::Codex);
        assert_eq!(adapter.tool(), CliTool::Codex);
        let timeline = adapter
            .load_timeline("codex-cxcp-design")
            .expect("timeline");

        assert_eq!(timeline.version, 1);
        assert_eq!(timeline.source_cli, CliTool::Codex);
        assert_eq!(timeline.source_session, "codex-cxcp-design");
        assert_eq!(timeline.events[0].id, "evt-001");
    }

    #[test]
    fn compile_request_carries_source_session_rewind_and_timeline() {
        let request = compile_request(CliTool::Codex, CliTool::Hermes, "evt-091").expect("request");

        assert_eq!(request.version, 1);
        assert_eq!(request.source_session.id, "codex-cxcp-design");
        assert_eq!(request.rewind_event_id, "evt-091");
        assert_eq!(request.timeline.events.len(), 7);
        assert_eq!(request.token_budget, 100_000);
    }

    #[test]
    fn compile_request_accepts_explicit_compiler() {
        let request = compile_request_with_compiler(
            CliTool::Codex,
            CliTool::Hermes,
            "evt-091",
            "custom-skill",
        )
        .expect("request");

        assert_eq!(request.compiler, "custom-skill");
    }
}
