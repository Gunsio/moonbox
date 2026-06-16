use serde::{Deserialize, Serialize};

use super::model::{CliTool, SessionSummary};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionAvailableActionKind {
    Inspect,
    Resume,
    Jump,
    Fork,
    Handoff,
    Copy,
    CopySessionId,
    Export,
    Archive,
}

impl SessionAvailableActionKind {
    pub fn id(self) -> &'static str {
        match self {
            Self::Inspect => "inspect",
            Self::Resume => "resume",
            Self::Jump => "jump",
            Self::Fork => "fork",
            Self::Handoff => "handoff",
            Self::Copy => "copy",
            Self::CopySessionId => "copy_session_id",
            Self::Export => "export",
            Self::Archive => "archive",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Inspect => "Inspect",
            Self::Resume => "Resume",
            Self::Jump => "Jump",
            Self::Fork => "Fork",
            Self::Handoff => "Handoff",
            Self::Copy => "Copy",
            Self::CopySessionId => "Copy Session ID",
            Self::Export => "Export",
            Self::Archive => "Archive",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionActionAvailability {
    Available,
    Unavailable,
    Blocked,
    Warning,
}

impl SessionActionAvailability {
    pub fn id(self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::Unavailable => "unavailable",
            Self::Blocked => "blocked",
            Self::Warning => "warning",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionActionSafety {
    SourceStoreReadOnly,
    MoonboxOverlayWrite,
    LaunchesProviderProcess,
    SelectsTmuxPane,
    GeneratesHandoffArtifact,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionAvailableAction {
    pub kind: SessionAvailableActionKind,
    pub label: String,
    pub status: SessionActionAvailability,
    pub reason: String,
    pub safety: Vec<SessionActionSafety>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keys: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionActionSet {
    pub version: u16,
    pub source_cli: CliTool,
    pub source_session: String,
    pub actions: Vec<SessionAvailableAction>,
}

impl SessionActionSet {
    pub fn action(&self, kind: SessionAvailableActionKind) -> Option<&SessionAvailableAction> {
        self.actions.iter().find(|action| action.kind == kind)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionActionContext {
    pub local_data_space: bool,
    pub hooks_enabled: bool,
    pub smart_enter_tmux: bool,
    pub live: Option<SessionActionLiveContext>,
}

impl SessionActionContext {
    pub fn local_without_live() -> Self {
        Self {
            local_data_space: true,
            hooks_enabled: false,
            smart_enter_tmux: false,
            live: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionActionLiveStatus {
    Running,
    Waiting,
    Idle,
    Dead,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionActionLiveContext {
    pub status: SessionActionLiveStatus,
    pub tmux_target: Result<String, String>,
}

pub fn session_action_set(
    session: &SessionSummary,
    context: &SessionActionContext,
) -> SessionActionSet {
    let actions = vec![
        inspect_action(),
        resume_action(context),
        jump_action(context),
        fork_action(),
        handoff_action(context),
        copy_action(),
        copy_session_id_action(),
        export_action(),
        archive_action(),
    ];
    SessionActionSet {
        version: 1,
        source_cli: session.cli,
        source_session: session.id.clone(),
        actions,
    }
}

fn inspect_action() -> SessionAvailableAction {
    action(
        SessionAvailableActionKind::Inspect,
        SessionActionAvailability::Available,
        "Session details can be inspected without touching the provider store.",
        vec![SessionActionSafety::SourceStoreReadOnly],
        vec!["d"],
    )
}

fn resume_action(context: &SessionActionContext) -> SessionAvailableAction {
    if !context.local_data_space {
        return action(
            SessionAvailableActionKind::Resume,
            SessionActionAvailability::Blocked,
            "SSH data space is read-only; resume requires a local provider CLI.",
            vec![SessionActionSafety::SourceStoreReadOnly],
            Vec::new(),
        );
    }
    if jump_is_available(context) {
        return action(
            SessionAvailableActionKind::Resume,
            SessionActionAvailability::Warning,
            "A live tmux pane is available; resume may start a separate provider process.",
            vec![
                SessionActionSafety::SourceStoreReadOnly,
                SessionActionSafety::LaunchesProviderProcess,
            ],
            Vec::new(),
        );
    }
    action(
        SessionAvailableActionKind::Resume,
        SessionActionAvailability::Available,
        "Local provider resume is available.",
        vec![
            SessionActionSafety::SourceStoreReadOnly,
            SessionActionSafety::LaunchesProviderProcess,
        ],
        vec!["enter"],
    )
}

fn jump_action(context: &SessionActionContext) -> SessionAvailableAction {
    if !context.local_data_space {
        return action(
            SessionAvailableActionKind::Jump,
            SessionActionAvailability::Blocked,
            "SSH data space is read-only; tmux jump is only checked locally.",
            vec![SessionActionSafety::SourceStoreReadOnly],
            Vec::new(),
        );
    }
    if !context.hooks_enabled {
        return action(
            SessionAvailableActionKind::Jump,
            SessionActionAvailability::Unavailable,
            "Hooks are disabled; no live tmux state is available.",
            vec![SessionActionSafety::SourceStoreReadOnly],
            Vec::new(),
        );
    }
    if !context.smart_enter_tmux {
        return action(
            SessionAvailableActionKind::Jump,
            SessionActionAvailability::Unavailable,
            "Smart Enter / tmux jump is disabled in Settings.",
            vec![SessionActionSafety::SourceStoreReadOnly],
            Vec::new(),
        );
    }
    let Some(live) = context.live.as_ref() else {
        return action(
            SessionAvailableActionKind::Jump,
            SessionActionAvailability::Unavailable,
            "No hook live state for this session.",
            vec![SessionActionSafety::SourceStoreReadOnly],
            Vec::new(),
        );
    };
    if live.status == SessionActionLiveStatus::Dead {
        return action(
            SessionAvailableActionKind::Jump,
            SessionActionAvailability::Unavailable,
            "Hook state marks this session ended.",
            vec![SessionActionSafety::SourceStoreReadOnly],
            Vec::new(),
        );
    }
    match &live.tmux_target {
        Ok(pane_id) => action(
            SessionAvailableActionKind::Jump,
            SessionActionAvailability::Available,
            format!("Live tmux pane {pane_id} is available."),
            vec![
                SessionActionSafety::SourceStoreReadOnly,
                SessionActionSafety::SelectsTmuxPane,
            ],
            vec!["enter"],
        ),
        Err(reason) => action(
            SessionAvailableActionKind::Jump,
            SessionActionAvailability::Unavailable,
            reason.clone(),
            vec![SessionActionSafety::SourceStoreReadOnly],
            Vec::new(),
        ),
    }
}

fn fork_action() -> SessionAvailableAction {
    action(
        SessionAvailableActionKind::Fork,
        SessionActionAvailability::Unavailable,
        "Whole-session fork is planned for a later milestone.",
        vec![SessionActionSafety::SourceStoreReadOnly],
        Vec::new(),
    )
}

fn handoff_action(context: &SessionActionContext) -> SessionAvailableAction {
    let reason = if context.local_data_space {
        "Handoff review can generate a target-agent continuation."
    } else {
        "SSH data space is read-only; guarded handoff is available."
    };
    action(
        SessionAvailableActionKind::Handoff,
        SessionActionAvailability::Available,
        reason,
        vec![
            SessionActionSafety::SourceStoreReadOnly,
            SessionActionSafety::GeneratesHandoffArtifact,
        ],
        vec!["x"],
    )
}

fn copy_action() -> SessionAvailableAction {
    action(
        SessionAvailableActionKind::Copy,
        SessionActionAvailability::Unavailable,
        "Copy last AI output is planned for a later milestone.",
        vec![SessionActionSafety::SourceStoreReadOnly],
        Vec::new(),
    )
}

fn copy_session_id_action() -> SessionAvailableAction {
    action(
        SessionAvailableActionKind::CopySessionId,
        SessionActionAvailability::Unavailable,
        "Copy session id is planned for a later milestone.",
        vec![SessionActionSafety::SourceStoreReadOnly],
        Vec::new(),
    )
}

fn export_action() -> SessionAvailableAction {
    action(
        SessionAvailableActionKind::Export,
        SessionActionAvailability::Unavailable,
        "Session export action is planned for a later milestone.",
        vec![
            SessionActionSafety::SourceStoreReadOnly,
            SessionActionSafety::MoonboxOverlayWrite,
        ],
        Vec::new(),
    )
}

fn archive_action() -> SessionAvailableAction {
    action(
        SessionAvailableActionKind::Archive,
        SessionActionAvailability::Unavailable,
        "Archive overlay is planned for a later milestone.",
        vec![SessionActionSafety::MoonboxOverlayWrite],
        Vec::new(),
    )
}

fn jump_is_available(context: &SessionActionContext) -> bool {
    context.local_data_space
        && context.hooks_enabled
        && context.smart_enter_tmux
        && context.live.as_ref().is_some_and(|live| {
            live.status != SessionActionLiveStatus::Dead && live.tmux_target.is_ok()
        })
}

fn action(
    kind: SessionAvailableActionKind,
    status: SessionActionAvailability,
    reason: impl Into<String>,
    safety: Vec<SessionActionSafety>,
    keys: Vec<&'static str>,
) -> SessionAvailableAction {
    SessionAvailableAction {
        kind,
        label: kind.label().into(),
        status,
        reason: reason.into(),
        safety,
        keys: keys.into_iter().map(String::from).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::model::{SessionStatus, SourceProvenance};

    fn session() -> SessionSummary {
        SessionSummary {
            id: "s1".into(),
            cli: CliTool::Codex,
            title: "Fixture".into(),
            cwd: "/repo".into(),
            updated_at: "2026-06-16T00:00:00Z".into(),
            updated: "now".into(),
            runtime_status: Default::default(),
            runtime_reason: None,
            status: SessionStatus::Healthy,
            branch: None,
            token_count: None,
            health_reason: None,
            event_count: 1,
            resume_command: "codex resume s1".into(),
            source_provenance: SourceProvenance::Fixture,
            source_path: None,
            source_size_bytes: None,
            parse_skip_count: 0,
            provider_metadata: None,
            anatomy: None,
        }
    }

    #[test]
    fn local_actions_include_planned_future_actions_without_enabling_them() {
        let actions = session_action_set(&session(), &SessionActionContext::local_without_live());

        assert_eq!(
            actions
                .action(SessionAvailableActionKind::Resume)
                .expect("resume")
                .status,
            SessionActionAvailability::Available
        );
        assert_eq!(
            actions
                .action(SessionAvailableActionKind::Fork)
                .expect("fork")
                .status,
            SessionActionAvailability::Unavailable
        );
        assert_eq!(
            actions
                .action(SessionAvailableActionKind::Copy)
                .expect("copy")
                .status,
            SessionActionAvailability::Unavailable
        );
        assert_eq!(
            actions
                .action(SessionAvailableActionKind::CopySessionId)
                .expect("copy session id")
                .status,
            SessionActionAvailability::Unavailable
        );
        assert_eq!(
            actions
                .action(SessionAvailableActionKind::Export)
                .expect("export")
                .status,
            SessionActionAvailability::Unavailable
        );
        assert_eq!(
            actions
                .action(SessionAvailableActionKind::Archive)
                .expect("archive")
                .safety,
            vec![SessionActionSafety::MoonboxOverlayWrite]
        );
    }

    #[test]
    fn remote_data_space_blocks_resume_and_jump_but_keeps_handoff_available() {
        let context = SessionActionContext {
            local_data_space: false,
            ..SessionActionContext::local_without_live()
        };
        let actions = session_action_set(&session(), &context);

        assert_eq!(
            actions
                .action(SessionAvailableActionKind::Resume)
                .expect("resume")
                .status,
            SessionActionAvailability::Blocked
        );
        assert_eq!(
            actions
                .action(SessionAvailableActionKind::Jump)
                .expect("jump")
                .status,
            SessionActionAvailability::Blocked
        );
        assert_eq!(
            actions
                .action(SessionAvailableActionKind::Handoff)
                .expect("handoff")
                .status,
            SessionActionAvailability::Available
        );
    }

    #[test]
    fn live_tmux_context_exposes_jump_and_marks_resume_as_warning() {
        let context = SessionActionContext {
            local_data_space: true,
            hooks_enabled: true,
            smart_enter_tmux: true,
            live: Some(SessionActionLiveContext {
                status: SessionActionLiveStatus::Running,
                tmux_target: Ok("%42".into()),
            }),
        };
        let actions = session_action_set(&session(), &context);

        assert_eq!(
            actions
                .action(SessionAvailableActionKind::Jump)
                .expect("jump")
                .status,
            SessionActionAvailability::Available
        );
        assert_eq!(
            actions
                .action(SessionAvailableActionKind::Resume)
                .expect("resume")
                .status,
            SessionActionAvailability::Warning
        );
    }
}
