use super::{
    adapter::{SourceAdapter, collect_sessions},
    fixture::FixtureSourceAdapter,
    model::{
        BranchNode, CanonicalTimeline, CapsuleCompileOutput, CapsuleCompileRequest, ChecklistItem,
        CliTool, DemoData, SessionStatus, SessionSummary, TimelineEvent, WorkCapsule,
    },
};

pub fn demo_data(source: CliTool, target: CliTool) -> DemoData {
    let sessions = demo_sessions();
    let source_session = sessions
        .iter()
        .find(|session| session.cli == source)
        .cloned()
        .unwrap_or_else(|| fallback_session(source));
    let source_session_id = source_session.id.clone();
    demo_data_for_session(&source_session_id, target).unwrap_or_else(|| {
        let timeline = demo_timeline_for_session(&source_session);
        let capsule = demo_capsule_for_session(&source_session, target);
        build_demo_data(
            source,
            target,
            sessions,
            timeline,
            capsule,
            &source_session_id,
        )
    })
}

pub fn demo_data_for_session(session_id: &str, target: CliTool) -> Option<DemoData> {
    let sessions = demo_sessions();
    let source_session = sessions
        .iter()
        .find(|session| session.id == session_id)
        .cloned()?;
    let source_session_id = source_session.id.clone();
    let timeline = demo_timeline_for_session(&source_session);
    let capsule = demo_capsule_for_session(&source_session, target);
    Some(build_demo_data(
        source_session.cli,
        target,
        sessions,
        timeline,
        capsule,
        &source_session_id,
    ))
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

pub type DemoSourceAdapter = FixtureSourceAdapter;

pub fn demo_adapters() -> [DemoSourceAdapter; 3] {
    [
        DemoSourceAdapter::new(CliTool::Codex),
        DemoSourceAdapter::new(CliTool::Claude),
        DemoSourceAdapter::new(CliTool::Hermes),
    ]
}

pub fn demo_sessions() -> Vec<SessionSummary> {
    let adapters = demo_adapters();
    collect_sessions(
        &adapters
            .iter()
            .map(|adapter| adapter as &dyn SourceAdapter)
            .collect::<Vec<_>>(),
    )
}

pub fn demo_timeline_for_session(session: &SessionSummary) -> Vec<TimelineEvent> {
    DemoSourceAdapter::new(session.cli)
        .load_timeline(&session.id)
        .map(|timeline| timeline.events)
        .unwrap_or_default()
}

pub fn demo_compile_request(
    source: CliTool,
    target: CliTool,
    rewind_event_id: &str,
) -> CapsuleCompileRequest {
    let data = demo_data(source, target);
    let source_session = data
        .sessions
        .iter()
        .find(|session| session.id == data.capsule.source_session)
        .cloned()
        .unwrap_or_else(|| fallback_session(source));
    CapsuleCompileRequest {
        version: 1,
        source_cli: source,
        target_cli: target,
        source_session,
        rewind_event_id: rewind_event_id.into(),
        token_budget: 100_000,
        compiler: data.capsule.compiler.clone(),
        timeline: DemoSourceAdapter::new(source)
            .load_timeline(&data.capsule.source_session)
            .unwrap_or(CanonicalTimeline {
                version: 1,
                source_cli: source,
                source_session: data.capsule.source_session.clone(),
                events: data.timeline,
            }),
    }
}

pub fn demo_compile_output(source: CliTool, target: CliTool) -> CapsuleCompileOutput {
    CapsuleCompileOutput {
        version: 1,
        capsule: demo_data(source, target).capsule,
    }
}

fn demo_capsule_for_session(session: &SessionSummary, target: CliTool) -> WorkCapsule {
    let (goal, state, rewind_point, risks) = match session.id.as_str() {
        "claude-qc-platform" => (
            "Continue QC trace propagation repair without losing staging context.",
            "Trace propagation patch is drafted; staging verification is still pending.",
            "evt-074 / before staging verification",
            vec![
                "Gateway fallback may hide upstream request_id bugs.".into(),
                "Staging traffic volume may not cover async retry paths.".into(),
            ],
        ),
        "hermes-cxcp-502" => (
            "Recover the cxcp investigation by avoiding raw copied-session resume.",
            "Raw resume failed with 502. The target path is Work Capsule handoff.",
            "evt-052 / before raw resume retry",
            vec![
                "Copied session rows can miss hidden provider state.".into(),
                "Target CLI resume protocol may reject raw source metadata.".into(),
            ],
        ),
        _ => (
            "Build Moonbox as a cross-CLI session rewind workbench.",
            "Raw resume is rejected. The target path is new branch + Work Capsule.",
            "evt-091 / before raw resume",
            vec![
                "Tool outputs and attachments can exceed target token budget.".into(),
                "Target CLI injection protocol may differ per tool.".into(),
            ],
        ),
    };

    WorkCapsule {
        version: 1,
        source_cli: session.cli,
        target_cli: target,
        source_session: session.id.clone(),
        rewind_point: rewind_point.into(),
        compiler: "engineering-handoff".into(),
        target_branch: format!(
            "moonbox/{}-rewind-{}",
            target.id(),
            rewind_point.split_whitespace().next().unwrap_or("evt-000")
        ),
        goal: goal.into(),
        state: state.into(),
        decisions: vec![
            "Source sessions are read-only.".into(),
            "Compression and compatibility live in replaceable compiler skills.".into(),
            "TUI is a first-class workbench, not an fzf picker.".into(),
        ],
        todo: vec![
            ChecklistItem {
                done: true,
                text: "Define canonical timeline and capsule schema.".into(),
            },
            ChecklistItem {
                done: false,
                text: "Implement source adapters for Codex, Claude, Hermes.".into(),
            },
            ChecklistItem {
                done: false,
                text: "Implement target launcher and verification loop.".into(),
            },
        ],
        evidence: vec![
            format!("session: {} ({})", session.id, session.cli),
            format!("cwd: {}", session.cwd),
            session
                .health_reason
                .clone()
                .unwrap_or_else(|| "no health reason".into()),
        ],
        risks,
    }
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

    #[test]
    fn demo_sessions_are_adapter_collected_and_time_sorted() {
        let sessions = demo_sessions();

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
        let request = demo_compile_request(CliTool::Codex, CliTool::Hermes, "evt-091");

        assert_eq!(request.version, 1);
        assert_eq!(request.source_session.id, "codex-cxcp-design");
        assert_eq!(request.rewind_event_id, "evt-091");
        assert_eq!(request.timeline.events.len(), 7);
        assert_eq!(request.token_budget, 100_000);
    }
}
