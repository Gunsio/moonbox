use super::model::{
    BranchNode, ChecklistItem, CliTool, DemoData, SessionStatus, SessionSummary, TimelineEvent,
    TimelineKind, WorkCapsule,
};

pub fn demo_data(source: CliTool, target: CliTool) -> DemoData {
    let mut sessions = vec![
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
    ];
    sessions.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
    let source_session_id = sessions
        .iter()
        .find(|session| session.cli == source)
        .map(|session| session.id.clone())
        .unwrap_or_else(|| format!("{}-session", source.id()));

    let timeline = vec![
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
    ];

    let capsule = WorkCapsule {
        version: 1,
        source_cli: source,
        target_cli: target,
        source_session: source_session_id.clone(),
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
    };

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
        compilers: vec![
            "engineering-handoff".into(),
            "bugfix-continuation".into(),
            "design-review".into(),
        ],
    }
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
