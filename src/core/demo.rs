use super::model::{
    BranchNode, ChecklistItem, CliTool, DemoData, SessionStatus, SessionSummary, TimelineEvent,
    TimelineKind, WorkCapsule,
};

pub fn demo_data(source: CliTool, target: CliTool) -> DemoData {
    let sessions = vec![
        SessionSummary {
            id: "codex-cxcp-design".into(),
            cli: source,
            title: "Moonbox session rewind design".into(),
            cwd: "~/coding/moonbox".into(),
            updated: "updated 10 min ago".into(),
            status: SessionStatus::Healthy,
            event_count: 148,
        },
        SessionSummary {
            id: "claude-qc-platform".into(),
            cli: CliTool::Claude,
            title: "QC platform trace repair".into(),
            cwd: "~/coding/qc-platform".into(),
            updated: "updated 2 hours ago".into(),
            status: SessionStatus::Warning,
            event_count: 92,
        },
        SessionSummary {
            id: "hermes-cxcp-502".into(),
            cli: CliTool::Hermes,
            title: "cxcp 502 resume failure".into(),
            cwd: "~/.codex".into(),
            updated: "failed yesterday".into(),
            status: SessionStatus::Failed,
            event_count: 61,
        },
    ];

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
        source_session: "codex-cxcp-design".into(),
        rewind_point: "evt-091 / before raw resume".into(),
        compiler: "engineering-handoff".into(),
        target_branch: "moonbox/hermes-rewind-evt-091".into(),
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
            label: "source/codex-cxcp-design".into(),
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
            label: "target/hermes-new-branch".into(),
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
