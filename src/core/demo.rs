use super::{
    compiler::{CapsuleCompiler, DemoCapsuleCompiler, default_rewind_event_id},
    error::CoreError,
    model::{
        BranchNode, CanonicalTimeline, CapsuleCompileOutput, CapsuleCompileRequest, CliTool,
        DemoData, SessionStatus, SessionSummary, TimelineEvent, WorkCapsule,
    },
    sources,
};

#[cfg(test)]
use super::fixture::FixtureSourceAdapter;

pub fn demo_data(source: CliTool, target: CliTool) -> Result<DemoData, CoreError> {
    let sessions = demo_sessions()?;
    let source_session = sessions
        .iter()
        .find(|session| session.cli == source)
        .cloned()
        .unwrap_or_else(|| fallback_session(source));
    let source_session_id = source_session.id.clone();
    if let Some(data) = demo_data_for_session(&source_session_id, target)? {
        return Ok(data);
    }

    let timeline = CanonicalTimeline {
        version: 1,
        source_cli: source,
        source_session: source_session_id.clone(),
        events: Vec::new(),
    };
    let rewind_event_id = rewind_event_id_for_timeline(&source_session_id, &timeline);
    let capsule =
        compile_capsule_for_session(&source_session, target, &timeline, &rewind_event_id)?;
    Ok(build_demo_data(
        source,
        target,
        sessions,
        timeline.events,
        capsule,
        &source_session_id,
    ))
}

pub fn demo_data_for_session(
    session_id: &str,
    target: CliTool,
) -> Result<Option<DemoData>, CoreError> {
    let sessions = demo_sessions()?;
    let source_session = sessions
        .iter()
        .find(|session| session.id == session_id)
        .cloned();
    let Some(source_session) = source_session else {
        return Ok(None);
    };
    let source_session_id = source_session.id.clone();
    let timeline = demo_canonical_timeline_for_session(&source_session)?;
    let rewind_event_id = rewind_event_id_for_timeline(&source_session_id, &timeline);
    let capsule =
        compile_capsule_for_session(&source_session, target, &timeline, &rewind_event_id)?;
    Ok(Some(build_demo_data(
        source_session.cli,
        target,
        sessions,
        timeline.events,
        capsule,
        &source_session_id,
    )))
}

fn build_demo_data(
    source: CliTool,
    target: CliTool,
    sessions: Vec<SessionSummary>,
    timeline: Vec<TimelineEvent>,
    capsule: WorkCapsule,
    source_session_id: &str,
) -> DemoData {
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

    DemoData {
        source,
        target,
        sessions,
        timeline,
        capsule,
        branches,
        compilers: demo_compilers(),
    }
}

#[cfg(test)]
pub type DemoSourceAdapter = FixtureSourceAdapter;

pub fn demo_sessions() -> Result<Vec<SessionSummary>, CoreError> {
    sources::list_sessions()
}

pub fn demo_canonical_timeline_for_session(
    session: &SessionSummary,
) -> Result<CanonicalTimeline, CoreError> {
    sources::load_timeline(session)
}

pub fn demo_compile_request(
    source: CliTool,
    target: CliTool,
    rewind_event_id: &str,
) -> Result<CapsuleCompileRequest, CoreError> {
    let data = demo_data(source, target)?;
    let source_session = data
        .sessions
        .iter()
        .find(|session| session.id == data.capsule.source_session)
        .cloned()
        .unwrap_or_else(|| fallback_session(source));
    Ok(CapsuleCompileRequest {
        version: 1,
        source_cli: source,
        target_cli: target,
        source_session,
        rewind_event_id: rewind_event_id.into(),
        token_budget: 100_000,
        compiler: data.capsule.compiler.clone(),
        timeline: CanonicalTimeline {
            version: 1,
            source_cli: source,
            source_session: data.capsule.source_session.clone(),
            events: data.timeline,
        },
    })
}

pub fn demo_compile_output(
    source: CliTool,
    target: CliTool,
) -> Result<CapsuleCompileOutput, CoreError> {
    let data = demo_data(source, target)?;
    Ok(CapsuleCompileOutput {
        version: 1,
        capsule: data.capsule,
    })
}

fn compile_capsule_for_session(
    session: &SessionSummary,
    target: CliTool,
    timeline: &CanonicalTimeline,
    rewind_event_id: &str,
) -> Result<WorkCapsule, CoreError> {
    let request = CapsuleCompileRequest {
        version: 1,
        source_cli: session.cli,
        target_cli: target,
        source_session: session.clone(),
        rewind_event_id: rewind_event_id.into(),
        token_budget: 100_000,
        compiler: "engineering-handoff".into(),
        timeline: timeline.clone(),
    };
    Ok(DemoCapsuleCompiler.compile(&request)?.capsule)
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

fn demo_compilers() -> Vec<String> {
    vec![
        "engineering-handoff".into(),
        "bugfix-continuation".into(),
        "design-review".into(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::adapter::SourceAdapter;

    #[test]
    fn demo_sessions_are_adapter_collected_and_time_sorted() {
        let sessions = demo_sessions().expect("sessions");

        assert_eq!(sessions.len(), 3);
        assert_eq!(sessions[0].id, "codex-cxcp-design");
        assert_eq!(sessions[1].id, "claude-qc-platform");
        assert_eq!(sessions[2].id, "hermes-cxcp-502");
    }

    #[test]
    fn demo_adapter_returns_canonical_timeline() {
        let adapter = DemoSourceAdapter::new(CliTool::Codex);
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
        let request =
            demo_compile_request(CliTool::Codex, CliTool::Hermes, "evt-091").expect("request");

        assert_eq!(request.version, 1);
        assert_eq!(request.source_session.id, "codex-cxcp-design");
        assert_eq!(request.rewind_event_id, "evt-091");
        assert_eq!(request.timeline.events.len(), 7);
        assert_eq!(request.token_budget, 100_000);
    }
}
