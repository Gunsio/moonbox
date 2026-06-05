use super::{
    adapter::{AdapterError, SourceAdapter, collect_sessions},
    model::{
        BranchNode, CanonicalTimeline, CapsuleCompileOutput, CapsuleCompileRequest, ChecklistItem,
        CliTool, DemoData, SessionStatus, SessionSummary, TimelineEvent, TimelineKind, WorkCapsule,
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

#[derive(Debug, Clone, Copy)]
pub struct DemoSourceAdapter {
    tool: CliTool,
}

impl DemoSourceAdapter {
    pub fn new(tool: CliTool) -> Self {
        Self { tool }
    }
}

impl SourceAdapter for DemoSourceAdapter {
    fn tool(&self) -> CliTool {
        self.tool
    }

    fn list_sessions(&self) -> Vec<SessionSummary> {
        demo_session_fixtures()
            .into_iter()
            .filter(|session| session.cli == self.tool)
            .collect()
    }

    fn load_timeline(&self, session_id: &str) -> Result<CanonicalTimeline, AdapterError> {
        let session = self
            .list_sessions()
            .into_iter()
            .find(|session| session.id == session_id)
            .ok_or_else(|| AdapterError::SessionNotFound {
                tool: self.tool,
                session_id: session_id.to_string(),
            })?;
        Ok(CanonicalTimeline {
            version: 1,
            source_cli: self.tool,
            source_session: session.id.clone(),
            events: demo_timeline_for_session(&session),
        })
    }
}

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
    match session.id.as_str() {
        "claude-qc-platform" => vec![
            event(
                "evt-001",
                "15:02",
                TimelineKind::User,
                "User",
                "Repair trace propagation across the QC platform gateway.",
            ),
            event(
                "evt-018",
                "15:09",
                TimelineKind::Tool,
                "Tool: rg",
                "Found missing request_id forwarding in gateway middleware.",
            ),
            event(
                "evt-041",
                "15:21",
                TimelineKind::Assistant,
                "Assistant",
                "Prepared trace header patch and marked health as warning pending staging verification.",
            ),
            event(
                "evt-059",
                "15:37",
                TimelineKind::GitDiff,
                "Git Diff",
                "+ gateway trace propagation, + request_id fallback.",
            ),
            event(
                "evt-074",
                "15:48",
                TimelineKind::RewindPoint,
                "Rewind Point",
                "Before staging verification. Compile continuation for Codex.",
            ),
        ],
        "hermes-cxcp-502" => vec![
            event(
                "evt-001",
                "18:04",
                TimelineKind::User,
                "User",
                "Investigate why copied Codex sessions return 502 after Hermes resume.",
            ),
            event(
                "evt-013",
                "18:11",
                TimelineKind::Tool,
                "Tool: sqlite",
                "Inspected copied session rows and rollout JSONL metadata.",
            ),
            event(
                "evt-029",
                "18:28",
                TimelineKind::Error,
                "Error",
                "Hermes resume rejected provider/session mismatch and returned 502.",
            ),
            event(
                "evt-044",
                "18:43",
                TimelineKind::Compact,
                "Compact",
                "Captured root cause: raw DB copy skips hidden provider state.",
            ),
            event(
                "evt-052",
                "18:55",
                TimelineKind::RewindPoint,
                "Rewind Point",
                "Before retrying raw resume. Compile Work Capsule instead.",
            ),
        ],
        _ => vec![
            event(
                "evt-001",
                "16:02",
                TimelineKind::User,
                "User",
                "Analyze cxcp and explain why copied sessions fail after resume.",
            ),
            event(
                "evt-017",
                "16:04",
                TimelineKind::Tool,
                "Tool: rg",
                "Found cxcp alias in ~/.zshrc and migration script in ~/.local/bin.",
            ),
            event(
                "evt-034",
                "16:08",
                TimelineKind::Assistant,
                "Assistant",
                "Conclusion: raw session copy is the wrong abstraction.",
            ),
            event(
                "evt-049",
                "16:14",
                TimelineKind::Compact,
                "Compact",
                "Conversation summary created; hidden state cannot be ported safely.",
            ),
            event(
                "evt-063",
                "16:18",
                TimelineKind::Error,
                "Error",
                "Target CLI resume returned 502 after provider/session mismatch.",
            ),
            event(
                "evt-078",
                "16:23",
                TimelineKind::GitDiff,
                "Git Diff",
                "+ Canonical Timeline schema, + Work Capsule schema.",
            ),
            event(
                "evt-091",
                "16:26",
                TimelineKind::RewindPoint,
                "Rewind Point",
                "Before raw resume. Compile new Work Capsule for Hermes.",
            ),
        ],
    }
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

fn demo_session_fixtures() -> Vec<SessionSummary> {
    vec![
        SessionSummary {
            id: "codex-cxcp-design".into(),
            cli: CliTool::Codex,
            title: "Moonbox session rewind design".into(),
            cwd: "~/coding/moonbox".into(),
            updated_at: "2026-06-05T16:50:00+08:00".into(),
            updated: "updated 10 min ago".into(),
            status: SessionStatus::Healthy,
            branch: Some("main".into()),
            token_count: Some(42_000),
            health_reason: Some("ready".into()),
            event_count: 148,
            resume_command: "codex resume codex-cxcp-design".into(),
        },
        SessionSummary {
            id: "claude-qc-platform".into(),
            cli: CliTool::Claude,
            title: "QC platform trace repair".into(),
            cwd: "~/coding/qc-platform".into(),
            updated_at: "2026-06-05T15:00:00+08:00".into(),
            updated: "updated 2 hours ago".into(),
            status: SessionStatus::Warning,
            branch: Some("fix/trace-propagation".into()),
            token_count: Some(67_000),
            health_reason: Some("staging verification pending".into()),
            event_count: 92,
            resume_command: "claude --resume claude-qc-platform".into(),
        },
        SessionSummary {
            id: "hermes-cxcp-502".into(),
            cli: CliTool::Hermes,
            title: "cxcp 502 resume failure".into(),
            cwd: "~/.codex".into(),
            updated_at: "2026-06-04T18:00:00+08:00".into(),
            updated: "failed yesterday".into(),
            status: SessionStatus::Failed,
            branch: Some("cxcp/raw-copy".into()),
            token_count: Some(58_000),
            health_reason: Some("502 after raw resume".into()),
            event_count: 61,
            resume_command: "hermes resume hermes-cxcp-502".into(),
        },
    ]
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

fn event(id: &str, time: &str, kind: TimelineKind, title: &str, detail: &str) -> TimelineEvent {
    TimelineEvent {
        id: id.into(),
        time: time.into(),
        kind,
        title: title.into(),
        detail: detail.into(),
    }
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
