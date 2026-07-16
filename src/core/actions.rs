use serde::{Deserialize, Serialize};

use super::{
    codex,
    model::{CliTool, SessionSummary},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionAvailableActionKind {
    Inspect,
    Resume,
    FullAccessResume,
    Jump,
    Fork,
    Handoff,
    LarkExport,
    NewSession,
    Yank,
    Archive,
}

impl SessionAvailableActionKind {
    pub fn id(self) -> &'static str {
        match self {
            Self::Inspect => "inspect",
            Self::Resume => "resume",
            Self::FullAccessResume => "full_access_resume",
            Self::Jump => "jump",
            Self::Fork => "fork",
            Self::Handoff => "handoff",
            Self::LarkExport => "lark_export",
            Self::NewSession => "new_session",
            Self::Yank => "yank",
            Self::Archive => "archive",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Inspect => "Inspect",
            Self::Resume => "Resume",
            Self::FullAccessResume => "Resume (Full Access)",
            Self::Jump => "Jump",
            Self::Fork => "Fork",
            Self::Handoff => "Handoff",
            Self::LarkExport => "Lark Doc",
            Self::NewSession => "New Session",
            Self::Yank => "Yank",
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
    BypassesApprovalsAndSandbox,
    SelectsTmuxPane,
    GeneratesHandoffArtifact,
    WritesExternalDocument,
    SendsPromptCopy,
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
    let mut actions = vec![inspect_action(), resume_action(context)];
    if let Some(action) = full_access_resume_action(session, context) {
        actions.push(action);
    }
    actions.extend([
        jump_action(context),
        fork_action(session, context),
        handoff_action(context),
        lark_export_action(context),
        new_session_action(context),
        share_action(),
        archive_action(),
    ]);
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

fn full_access_resume_action(
    session: &SessionSummary,
    context: &SessionActionContext,
) -> Option<SessionAvailableAction> {
    if !context.local_data_space
        || session.cli != CliTool::Codex
        || codex::is_k2_session_summary(session)
    {
        return None;
    }

    Some(action(
        SessionAvailableActionKind::FullAccessResume,
        SessionActionAvailability::Warning,
        "Skips all Codex confirmation prompts and runs without sandboxing; use only in an externally sandboxed environment.",
        vec![
            SessionActionSafety::SourceStoreReadOnly,
            SessionActionSafety::LaunchesProviderProcess,
            SessionActionSafety::BypassesApprovalsAndSandbox,
        ],
        Vec::new(),
    ))
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

fn fork_action(session: &SessionSummary, context: &SessionActionContext) -> SessionAvailableAction {
    if !context.local_data_space {
        return action(
            SessionAvailableActionKind::Fork,
            SessionActionAvailability::Blocked,
            "SSH data space is read-only; native fork requires a local provider CLI.",
            vec![SessionActionSafety::SourceStoreReadOnly],
            Vec::new(),
        );
    }
    match session.cli {
        CliTool::Codex => action(
            SessionAvailableActionKind::Fork,
            SessionActionAvailability::Available,
            "Codex native session fork is available.",
            vec![
                SessionActionSafety::SourceStoreReadOnly,
                SessionActionSafety::LaunchesProviderProcess,
            ],
            Vec::new(),
        ),
        CliTool::Claude => action(
            SessionAvailableActionKind::Fork,
            SessionActionAvailability::Available,
            "Claude native resume fork is available.",
            vec![
                SessionActionSafety::SourceStoreReadOnly,
                SessionActionSafety::LaunchesProviderProcess,
            ],
            Vec::new(),
        ),
        CliTool::Hermes => action(
            SessionAvailableActionKind::Fork,
            SessionActionAvailability::Unavailable,
            "Hermes does not currently expose native session fork.",
            vec![SessionActionSafety::SourceStoreReadOnly],
            Vec::new(),
        ),
    }
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

fn lark_export_action(context: &SessionActionContext) -> SessionAvailableAction {
    if !context.local_data_space {
        return action(
            SessionAvailableActionKind::LarkExport,
            SessionActionAvailability::Blocked,
            "SSH data space is read-only; Lark export requires local session context.",
            vec![SessionActionSafety::SourceStoreReadOnly],
            Vec::new(),
        );
    }
    action(
        SessionAvailableActionKind::LarkExport,
        SessionActionAvailability::Available,
        "Generate and create a Feishu/Lark handoff document for this session.",
        vec![
            SessionActionSafety::SourceStoreReadOnly,
            SessionActionSafety::GeneratesHandoffArtifact,
            SessionActionSafety::WritesExternalDocument,
        ],
        Vec::new(),
    )
}

fn new_session_action(context: &SessionActionContext) -> SessionAvailableAction {
    if !context.local_data_space {
        return action(
            SessionAvailableActionKind::NewSession,
            SessionActionAvailability::Blocked,
            "SSH data space is read-only; starting a new target session requires a local target CLI.",
            vec![SessionActionSafety::SourceStoreReadOnly],
            Vec::new(),
        );
    }
    action(
        SessionAvailableActionKind::NewSession,
        SessionActionAvailability::Available,
        "Start the target CLI with the first user prompt and attachment paths.",
        vec![
            SessionActionSafety::SourceStoreReadOnly,
            SessionActionSafety::LaunchesProviderProcess,
            SessionActionSafety::SendsPromptCopy,
        ],
        Vec::new(),
    )
}

fn share_action() -> SessionAvailableAction {
    action(
        SessionAvailableActionKind::Yank,
        SessionActionAvailability::Available,
        "Yank session content without launching provider processes.",
        vec![SessionActionSafety::SourceStoreReadOnly],
        vec!["y"],
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
            context_health: None,
            anatomy: None,
        }
    }

    #[test]
    fn local_actions_include_available_native_fork_and_yank() {
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
            SessionActionAvailability::Available
        );
        assert_eq!(
            actions
                .action(SessionAvailableActionKind::Yank)
                .expect("yank")
                .status,
            SessionActionAvailability::Available
        );
        assert_eq!(
            actions
                .action(SessionAvailableActionKind::NewSession)
                .expect("new session")
                .status,
            SessionActionAvailability::Available
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
    fn full_access_resume_is_limited_to_local_native_codex_sessions() {
        let codex = session();
        let actions = session_action_set(&codex, &SessionActionContext::local_without_live());
        let action = actions
            .action(SessionAvailableActionKind::FullAccessResume)
            .expect("full-access Codex resume");

        assert_eq!(action.status, SessionActionAvailability::Warning);
        assert_eq!(
            action.reason,
            "Skips all Codex confirmation prompts and runs without sandboxing; use only in an externally sandboxed environment."
        );
        assert_eq!(
            action.safety,
            vec![
                SessionActionSafety::SourceStoreReadOnly,
                SessionActionSafety::LaunchesProviderProcess,
                SessionActionSafety::BypassesApprovalsAndSandbox,
            ]
        );

        let remote_context = SessionActionContext {
            local_data_space: false,
            ..SessionActionContext::local_without_live()
        };
        assert!(
            session_action_set(&codex, &remote_context)
                .action(SessionAvailableActionKind::FullAccessResume)
                .is_none()
        );

        let mut claude = session();
        claude.cli = CliTool::Claude;
        assert!(
            session_action_set(&claude, &SessionActionContext::local_without_live())
                .action(SessionAvailableActionKind::FullAccessResume)
                .is_none()
        );

        let mut hermes = session();
        hermes.cli = CliTool::Hermes;
        assert!(
            session_action_set(&hermes, &SessionActionContext::local_without_live())
                .action(SessionAvailableActionKind::FullAccessResume)
                .is_none()
        );

        let mut k2 = session();
        k2.id = "codex:019eef43-5ee0-78a0-b9c7-7f85f951fa74".into();
        k2.source_path =
            Some("k2-session:///Users/me/.k2/chat/sessions/codex_019eef43.json".into());
        assert!(
            session_action_set(&k2, &SessionActionContext::local_without_live())
                .action(SessionAvailableActionKind::FullAccessResume)
                .is_none()
        );
    }

    #[test]
    fn native_fork_availability_tracks_provider_support() {
        let mut codex = session();
        codex.cli = CliTool::Codex;
        let codex_action = session_action_set(&codex, &SessionActionContext::local_without_live())
            .action(SessionAvailableActionKind::Fork)
            .expect("codex fork")
            .clone();
        assert_eq!(codex_action.status, SessionActionAvailability::Available);
        assert_eq!(
            codex_action.reason,
            "Codex native session fork is available."
        );

        let mut claude = session();
        claude.cli = CliTool::Claude;
        let claude_action =
            session_action_set(&claude, &SessionActionContext::local_without_live())
                .action(SessionAvailableActionKind::Fork)
                .expect("claude fork")
                .clone();
        assert_eq!(claude_action.status, SessionActionAvailability::Available);
        assert_eq!(
            claude_action.reason,
            "Claude native resume fork is available."
        );

        let mut hermes = session();
        hermes.cli = CliTool::Hermes;
        let hermes_action =
            session_action_set(&hermes, &SessionActionContext::local_without_live())
                .action(SessionAvailableActionKind::Fork)
                .expect("hermes fork")
                .clone();
        assert_eq!(hermes_action.status, SessionActionAvailability::Unavailable);
        assert_eq!(
            hermes_action.reason,
            "Hermes does not currently expose native session fork."
        );
    }

    #[test]
    fn remote_native_fork_is_blocked() {
        let context = SessionActionContext {
            local_data_space: false,
            ..SessionActionContext::local_without_live()
        };
        let action = session_action_set(&session(), &context)
            .action(SessionAvailableActionKind::Fork)
            .expect("remote fork")
            .clone();

        assert_eq!(action.status, SessionActionAvailability::Blocked);
        assert_eq!(
            action.reason,
            "SSH data space is read-only; native fork requires a local provider CLI."
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
