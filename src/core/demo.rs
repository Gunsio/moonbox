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
    let timeline = demo_timeline();

    let capsule = demo_capsule(source, target, &source_session_id);

    let branches = vec![
        BranchNode {
            id: "root".into(),
            label: format!("original/{source_session_id}"),
            detail: "original session, read-only".into(),
            active: false,
        },
        BranchNode {
            id: "evt-091".into(),
            label: "rewind/evt-091".into(),
            detail: "before raw resume failure".into(),
            active: false,
        },
        BranchNode {
            id: "target".into(),
            label: format!("handoff/{}-new-branch", target.id()),
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
            source_session: session.id,
            events: demo_timeline(),
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

pub fn demo_timeline() -> Vec<TimelineEvent> {
    vec![
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
    ]
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
            event_count: 61,
            resume_command: "hermes resume hermes-cxcp-502".into(),
        },
    ]
}

fn demo_capsule(source: CliTool, target: CliTool, source_session_id: &str) -> WorkCapsule {
    WorkCapsule {
        version: 1,
        source_cli: source,
        target_cli: target,
        source_session: source_session_id.into(),
        rewind_point: "evt-091 / before raw resume".into(),
        compiler: "engineering-handoff".into(),
        target_branch: format!("moonbox/{}-rewind-evt-091", target.id()),
        goal: "Build Moonbox as a cross-CLI session rewind workbench.".into(),
        state: "Raw resume is rejected. The target path is new branch + Work Capsule.".into(),
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
            "~/.zshrc: cxcp alias points to codex-session-to-cx".into(),
            "~/.local/bin/codex-session-to-cx copies DB rows and rollout JSONL".into(),
            "Failure mode: provider/session schema mismatch can surface as 502".into(),
        ],
        risks: vec![
            "Tool outputs and attachments can exceed target token budget.".into(),
            "Target CLI injection protocol may differ per tool.".into(),
        ],
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
