use std::{
    env, fmt,
    sync::mpsc::{self, TryRecvError},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::core::{
    actions::{
        self, SessionActionAvailability, SessionActionContext, SessionActionLiveContext,
        SessionActionLiveStatus, SessionActionSet, SessionAvailableAction,
        SessionAvailableActionKind,
    },
    capsule_store::CapsuleSummary,
    compiler, config, continuation, dataspace, doctor,
    error::CoreError,
    handoff, hooks,
    image_preview::{TimelineImagePreview, build_timeline_image_previews},
    lark, launcher,
    model::{
        CliTool, CompilerPresetInfo, CompilerPresetStatus, ContinuationOptions, DoctorReport,
        LaunchExecution, LaunchExecutionStatus, LaunchPlan, LaunchValidation,
        LaunchValidationState, OriginalSessionPlan, SessionAction, SessionSummary,
        SourceProvenance, TimelineAttachment, TimelineEvent, TimelineKind, VerificationReport,
        WorkCapsule, WorkbenchData,
    },
    setup, tmux, verifier, workbench,
};

type SessionLoadResult = Result<WorkbenchData, CoreError>;
type SessionPreviewResult = Result<WorkbenchData, CoreError>;
type DataSpaceLoadResult = Result<WorkbenchData, CoreError>;
type LaunchReviewResult = Result<WorkbenchData, CoreError>;
type LaunchReviewReceiver = mpsc::Receiver<LaunchReviewMessage>;

pub const HANDOFF_TRAIL_DURATION_MS: u64 = 720;
const HANDOFF_TRAIL_FRAME_COUNT: usize = 6;
const SESSION_PREVIEW_DEBOUNCE_MS: u64 = 180;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandoffTrailPhase {
    Review,
}

impl HandoffTrailPhase {
    pub fn label(self) -> &'static str {
        match self {
            Self::Review => "Review",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HandoffTrailFrame {
    pub phase: HandoffTrailPhase,
    pub step: usize,
    pub elapsed_ms: u64,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookWaitingItem {
    pub cli: CliTool,
    pub session_id: String,
    pub title: String,
    pub reason: String,
    pub waiting_for_ms: u128,
    pub cwd: Option<String>,
    pub tmux_pane: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TmuxJumpPlan {
    pub source_session: SessionSummary,
    pub command: tmux::TmuxJumpCommand,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetupInstallPlan {
    pub target: setup::SetupInstallTarget,
    pub label: String,
    pub command_display: String,
    pub compiler_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct LarkExportTuiPlan {
    pub session_id: String,
    pub target: CliTool,
    pub rewind: String,
    pub compiler: String,
    pub command_display: String,
    pub title: String,
    pub markdown: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsField {
    Language,
    Theme,
    SmartEnter,
    LarkCli,
}

impl SettingsField {
    const ALL: [Self; 4] = [Self::Language, Self::Theme, Self::SmartEnter, Self::LarkCli];

    fn index(self) -> usize {
        match self {
            Self::Language => 0,
            Self::Theme => 1,
            Self::SmartEnter => 2,
            Self::LarkCli => 3,
        }
    }

    fn from_index(index: usize) -> Self {
        Self::ALL[index % Self::ALL.len()]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnterRouteKind {
    Disabled,
    Resume,
    Jump,
    Unavailable,
    Handoff,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnterRoutePreview {
    pub kind: EnterRouteKind,
    pub label: &'static str,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionMenuEntry {
    pub action: SessionAvailableAction,
    pub selected: bool,
    pub runnable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SharePanelActionKind {
    FirstUserInput,
    LastAiOutput,
    SessionId,
    HandoffContent,
    PortableJson,
}

impl SharePanelActionKind {
    const ALL: [Self; 5] = [
        Self::FirstUserInput,
        Self::LastAiOutput,
        Self::SessionId,
        Self::HandoffContent,
        Self::PortableJson,
    ];
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharePanelEntry {
    pub kind: SharePanelActionKind,
    pub selected: bool,
    pub runnable: bool,
    pub status: SessionActionAvailability,
    pub reason: String,
}

fn target_runner_id(target: CliTool) -> Option<&'static str> {
    match target {
        CliTool::Codex => Some("codex"),
        CliTool::Claude => Some("claude"),
        CliTool::Hermes => None,
    }
}

fn session_action_live_status(status: hooks::HookSessionStatus) -> SessionActionLiveStatus {
    match status {
        hooks::HookSessionStatus::Running => SessionActionLiveStatus::Running,
        hooks::HookSessionStatus::Waiting => SessionActionLiveStatus::Waiting,
        hooks::HookSessionStatus::Idle => SessionActionLiveStatus::Idle,
        hooks::HookSessionStatus::Dead => SessionActionLiveStatus::Dead,
    }
}

fn action_is_runnable(status: SessionActionAvailability) -> bool {
    matches!(
        status,
        SessionActionAvailability::Available | SessionActionAvailability::Warning
    )
}

fn action_menu_order() -> [SessionAvailableActionKind; 9] {
    [
        SessionAvailableActionKind::Resume,
        SessionAvailableActionKind::Handoff,
        SessionAvailableActionKind::LarkExport,
        SessionAvailableActionKind::NewSession,
        SessionAvailableActionKind::Fork,
        SessionAvailableActionKind::Jump,
        SessionAvailableActionKind::Inspect,
        SessionAvailableActionKind::Yank,
        SessionAvailableActionKind::Archive,
    ]
}

const SHARE_PORTABLE_JSON_CLIPBOARD_LIMIT_BYTES: usize = 512 * 1024;
const ARCHIVE_FEEDBACK_FRAMES: usize = 3;

#[derive(Debug, Clone, Copy)]
struct HandoffTrail {
    phase: HandoffTrailPhase,
    started_at: Instant,
}

struct PendingSessionLoad {
    request_id: u64,
    session_id: String,
    target: CliTool,
    started_at: Instant,
    receiver: mpsc::Receiver<SessionLoadResult>,
}

struct PendingSessionPreview {
    request_id: u64,
    session_id: String,
    target: CliTool,
    started_at: Instant,
    receiver: mpsc::Receiver<SessionPreviewResult>,
}

#[derive(Debug, Clone)]
struct DeferredSessionPreview {
    session_id: String,
    due_at: Instant,
}

struct PendingDataSpaceLoad {
    request_id: u64,
    index: usize,
    space: dataspace::DataSpaceEntry,
    started_at: Instant,
    receiver: mpsc::Receiver<DataSpaceLoadResult>,
}

struct PendingLaunchReview {
    request_id: u64,
    session_id: String,
    target: CliTool,
    selected_compiler: usize,
    compiler_id: String,
    rewind_event_id: String,
    started_at: Instant,
    timeout_ms: u128,
    stage: LaunchReviewStage,
    stage_detail: String,
    receiver: LaunchReviewReceiver,
}

pub const DATA_SPACE_CONFIG_FIELD_COUNT: usize = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchReviewStage {
    Queued,
    PreparingContext,
    StartingRunner,
    RunningSkill,
    Verifying,
}

impl LaunchReviewStage {
    fn label(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::PreparingContext => "preparing_context",
            Self::StartingRunner => "starting_runner",
            Self::RunningSkill => "running_skill",
            Self::Verifying => "verifying",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchReviewJobStatus {
    pub stage: LaunchReviewStage,
    pub stage_label: &'static str,
    pub detail: String,
    pub target: CliTool,
    pub session_id: String,
    pub compiler_id: String,
    pub elapsed_ms: u128,
    pub timeout_ms: u128,
}

#[derive(Debug, Clone)]
struct LaunchReviewProgress {
    stage: LaunchReviewStage,
    detail: String,
}

enum LaunchReviewMessage {
    Progress(LaunchReviewProgress),
    Finished(Box<LaunchReviewResult>),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DataSpaceConfigForm {
    pub quick: String,
    pub name: String,
    pub host: String,
    pub user: String,
    pub port: String,
    pub identity_file: String,
}

impl DataSpaceConfigForm {
    fn field_mut(&mut self, index: usize) -> &mut String {
        match index.min(DATA_SPACE_CONFIG_FIELD_COUNT - 1) {
            0 => &mut self.quick,
            1 => &mut self.name,
            2 => &mut self.host,
            3 => &mut self.user,
            4 => &mut self.port,
            _ => &mut self.identity_file,
        }
    }

    fn parse_quick_into_fields(&mut self) -> Result<bool, String> {
        let input = self.quick.trim();
        if input.is_empty() {
            return Ok(false);
        }
        let parsed = parse_ssh_target(input)?;
        if self.name.trim().is_empty() {
            self.name = parsed.name;
        }
        self.host = parsed.host;
        if let Some(user) = parsed.user {
            self.user = user;
        }
        if let Some(port) = parsed.port {
            self.port = port.to_string();
        }
        if let Some(identity_file) = parsed.identity_file {
            self.identity_file = identity_file;
        }
        Ok(true)
    }

    fn to_config(&self) -> Result<config::SshHostConfig, String> {
        let name = self.name.trim();
        let host = self.host.trim();
        if name.is_empty() {
            return Err("name is required".into());
        }
        if host.is_empty() {
            return Err("host is required".into());
        }
        let port = if self.port.trim().is_empty() {
            None
        } else {
            Some(
                self.port
                    .trim()
                    .parse::<u16>()
                    .map_err(|_| "port must be 1-65535".to_string())?,
            )
        };
        Ok(config::SshHostConfig {
            name: name.into(),
            host: host.into(),
            user: non_empty_optional(&self.user),
            port,
            identity_file: non_empty_optional(&self.identity_file),
            tags: vec!["ssh".into()],
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedSshTarget {
    name: String,
    host: String,
    user: Option<String>,
    port: Option<u16>,
    identity_file: Option<String>,
}

fn parse_ssh_target(input: &str) -> Result<ParsedSshTarget, String> {
    let input = input.trim();
    if input.is_empty() {
        return Err("paste an ssh target or fill host manually".into());
    }
    if let Some(parsed) = parse_openssh_host_block(input)? {
        return Ok(parsed);
    }

    let words = split_ssh_words(input)?;
    let mut words = words.as_slice();
    if words
        .first()
        .is_some_and(|word| word.eq_ignore_ascii_case("ssh"))
    {
        words = &words[1..];
    }

    let mut user = None;
    let mut port = None;
    let mut identity_file = None;
    let mut target = None;
    let mut index = 0;
    while index < words.len() {
        let word = &words[index];
        match word.as_str() {
            "-p" => {
                index += 1;
                let value = words
                    .get(index)
                    .ok_or_else(|| "-p requires a port".to_string())?;
                port = Some(parse_ssh_port(value)?);
            }
            "-i" => {
                index += 1;
                identity_file = Some(
                    words
                        .get(index)
                        .ok_or_else(|| "-i requires an identity file".to_string())?
                        .clone(),
                );
            }
            "-l" => {
                index += 1;
                user = Some(
                    words
                        .get(index)
                        .ok_or_else(|| "-l requires a user".to_string())?
                        .clone(),
                );
            }
            "-o" | "-F" | "-J" => {
                index += 1;
                if words.get(index).is_none() {
                    return Err(format!("{word} requires a value"));
                }
            }
            "--" => {
                target = words.get(index + 1).cloned();
                break;
            }
            _ if word.starts_with("-p") && word.len() > 2 => {
                port = Some(parse_ssh_port(&word[2..])?);
            }
            _ if word.starts_with("-i") && word.len() > 2 => {
                identity_file = Some(word[2..].into());
            }
            _ if word.starts_with("-l") && word.len() > 2 => {
                user = Some(word[2..].into());
            }
            _ if word.starts_with('-') => {}
            _ => target = Some(word.clone()),
        }
        index += 1;
    }

    let target = target.ok_or_else(|| "ssh target is required".to_string())?;
    parse_ssh_target_literal(&target, user, port, identity_file)
}

fn parse_openssh_host_block(input: &str) -> Result<Option<ParsedSshTarget>, String> {
    if !input
        .lines()
        .any(|line| line.trim_start().to_ascii_lowercase().starts_with("host "))
    {
        return Ok(None);
    }

    let mut name = None;
    let mut host = None;
    let mut user = None;
    let mut port = None;
    let mut identity_file = None;
    for line in input.lines() {
        let line = line.split_once('#').map(|(left, _)| left).unwrap_or(line);
        let mut parts = line.split_whitespace();
        let Some(keyword) = parts.next() else {
            continue;
        };
        match keyword.to_ascii_lowercase().as_str() {
            "host" => {
                name = parts
                    .find(|alias| {
                        !alias.starts_with('!') && !alias.contains('*') && !alias.contains('?')
                    })
                    .map(str::to_string);
            }
            "hostname" => host = parts.next().map(str::to_string),
            "user" => user = parts.next().map(str::to_string),
            "port" => {
                if let Some(value) = parts.next() {
                    port = Some(parse_ssh_port(value)?);
                }
            }
            "identityfile" => identity_file = parts.next().map(str::to_string),
            _ => {}
        }
    }

    let name = name.ok_or_else(|| "OpenSSH Host alias is required".to_string())?;
    let host = host.unwrap_or_else(|| name.clone());
    Ok(Some(ParsedSshTarget {
        name,
        host,
        user,
        port,
        identity_file,
    }))
}

fn parse_ssh_target_literal(
    target: &str,
    user: Option<String>,
    port: Option<u16>,
    identity_file: Option<String>,
) -> Result<ParsedSshTarget, String> {
    let mut value = target.trim().trim_matches('"').trim_matches('\'');
    if let Some(rest) = value.strip_prefix("ssh://") {
        value = rest.split('/').next().unwrap_or(rest);
    }
    let (user_from_target, host_port) = value
        .split_once('@')
        .map(|(left, right)| (Some(left.to_string()), right))
        .unwrap_or((None, value));
    let (host, port_from_target) = split_host_port(host_port)?;
    if host.trim().is_empty() {
        return Err("host is required".into());
    }
    let user = user.or(user_from_target);
    let port = port.or(port_from_target);
    Ok(ParsedSshTarget {
        name: sanitize_ssh_space_name(host),
        host: host.into(),
        user,
        port,
        identity_file,
    })
}

fn split_host_port(value: &str) -> Result<(&str, Option<u16>), String> {
    if let Some(stripped) = value.strip_prefix('[')
        && let Some((host, rest)) = stripped.split_once(']')
    {
        let port = rest.strip_prefix(':').map(parse_ssh_port).transpose()?;
        return Ok((host, port));
    }
    if let Some((host, port)) = value.rsplit_once(':')
        && !host.contains(':')
        && port.chars().all(|ch| ch.is_ascii_digit())
    {
        return Ok((host, Some(parse_ssh_port(port)?)));
    }
    Ok((value, None))
}

fn parse_ssh_port(value: &str) -> Result<u16, String> {
    value
        .trim()
        .parse::<u16>()
        .map_err(|_| "port must be 1-65535".to_string())
        .and_then(|port| {
            if port == 0 {
                Err("port must be 1-65535".into())
            } else {
                Ok(port)
            }
        })
}

fn sanitize_ssh_space_name(host: &str) -> String {
    host.trim()
        .trim_matches(|ch| ch == '[' || ch == ']')
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

fn split_ssh_words(input: &str) -> Result<Vec<String>, String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut escaped = false;
    for ch in input.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if let Some(open_quote) = quote {
            if ch == open_quote {
                quote = None;
            } else {
                current.push(ch);
            }
            continue;
        }
        match ch {
            '\'' | '"' => quote = Some(ch),
            ch if ch.is_whitespace() => {
                if !current.is_empty() {
                    words.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(ch),
        }
    }
    if escaped {
        current.push('\\');
    }
    if quote.is_some() {
        return Err("unterminated quote in ssh target".into());
    }
    if !current.is_empty() {
        words.push(current);
    }
    Ok(words)
}

fn non_empty_optional(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.into())
}

impl fmt::Debug for PendingDataSpaceLoad {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PendingDataSpaceLoad")
            .field("request_id", &self.request_id)
            .field("index", &self.index)
            .field("space", &self.space.label)
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for PendingSessionLoad {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PendingSessionLoad")
            .field("request_id", &self.request_id)
            .field("session_id", &self.session_id)
            .field("target", &self.target)
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for PendingSessionPreview {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PendingSessionPreview")
            .field("request_id", &self.request_id)
            .field("session_id", &self.session_id)
            .field("target", &self.target)
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for PendingLaunchReview {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PendingLaunchReview")
            .field("request_id", &self.request_id)
            .field("session_id", &self.session_id)
            .field("target", &self.target)
            .field("selected_compiler", &self.selected_compiler)
            .field("compiler_id", &self.compiler_id)
            .field("rewind_event_id", &self.rewind_event_id)
            .field("stage", &self.stage)
            .field("stage_detail", &self.stage_detail)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Sessions,
    Timeline,
    Capsule,
    Branches,
}

impl Focus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Sessions => "Sessions",
            Self::Timeline => "Timeline",
            Self::Capsule => "Details",
            Self::Branches => "Action Path",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionFilter {
    Starred,
    Archived,
    All,
    Tool(CliTool),
}

impl SessionFilter {
    pub fn label(self) -> &'static str {
        match self {
            Self::Starred => "Star",
            Self::Archived => "Archived",
            Self::All => "All",
            Self::Tool(CliTool::Codex) => "Codex",
            Self::Tool(CliTool::Claude) => "Claude",
            Self::Tool(CliTool::Hermes) => "Hermes",
        }
    }

    fn matches(
        self,
        session: &SessionSummary,
        starred_sessions: &[String],
        archived_sessions: &[String],
    ) -> bool {
        let archived = archived_sessions
            .iter()
            .any(|key| key == &session_overlay_key(session));
        match self {
            Self::Starred => {
                starred_sessions
                    .iter()
                    .any(|key| key == &session_overlay_key(session))
                    && !archived
            }
            Self::Archived => archived,
            Self::All => !archived,
            Self::Tool(tool) => session.cli == tool && !archived,
        }
    }

    fn next(self) -> Self {
        match self {
            Self::Starred => Self::Archived,
            Self::Archived => Self::All,
            Self::All => Self::Tool(CliTool::Codex),
            Self::Tool(CliTool::Codex) => Self::Tool(CliTool::Claude),
            Self::Tool(CliTool::Claude) => Self::Tool(CliTool::Hermes),
            Self::Tool(CliTool::Hermes) => Self::Starred,
        }
    }

    fn previous(self) -> Self {
        match self {
            Self::Starred => Self::Tool(CliTool::Hermes),
            Self::Archived => Self::Starred,
            Self::All => Self::Archived,
            Self::Tool(CliTool::Codex) => Self::All,
            Self::Tool(CliTool::Claude) => Self::Tool(CliTool::Codex),
            Self::Tool(CliTool::Hermes) => Self::Tool(CliTool::Claude),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveFeedbackKind {
    Archive,
    Unarchive,
}

#[derive(Debug, Clone)]
struct PendingArchiveFeedback {
    session_key: String,
    session_id: String,
    kind: ArchiveFeedbackKind,
    visible_position: usize,
    started_tick: usize,
}

#[derive(Debug, Clone)]
pub enum TuiExitAction {
    OriginalResume(Box<OriginalSessionPlan>),
    NativeFork(Box<OriginalSessionPlan>),
    NewSession(Box<OriginalSessionPlan>),
}

#[derive(Debug, Clone)]
pub struct TargetLaunchResult {
    pub target: CliTool,
    pub source: CliTool,
    pub session_id: String,
    pub command: String,
    pub command_summary: String,
    pub outcome: String,
    pub success: bool,
    pub plan: Box<LaunchPlan>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchReviewErrorState {
    pub target: CliTool,
    pub compiler_id: String,
    pub message: String,
    pub elapsed_ms: u128,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OriginalResumeMode {
    Suspend,
    Exec,
}

#[derive(Debug, Clone, Copy)]
pub struct CommandPaletteEntry {
    pub command: &'static str,
    pub aliases: &'static [&'static str],
    pub description: &'static str,
    pub params: &'static str,
    pub badge: &'static str,
    pub dangerous: bool,
}

const COMMAND_PALETTE_ENTRIES: &[CommandPaletteEntry] = &[
    CommandPaletteEntry {
        command: "open",
        aliases: &["o", "original", "resume"],
        description: "Preview original CLI resume for the selected session",
        params: "selected session",
        badge: "PREVIEW",
        dangerous: false,
    },
    CommandPaletteEntry {
        command: "handoff",
        aliases: &["x", "target", "launch"],
        description: "Choose a target CLI and open guarded Handoff Review",
        params: "target CLI",
        badge: "DRY-RUN",
        dangerous: false,
    },
    CommandPaletteEntry {
        command: "capsule",
        aliases: &["c", "compile", "review"],
        description: "Refresh the Capsule and open Handoff Review",
        params: "selected rewind",
        badge: "REVIEW",
        dangerous: false,
    },
    CommandPaletteEntry {
        command: "capsules",
        aliases: &["saved capsules", "capsule list", "store"],
        description: "Open saved local Capsule inventory",
        params: "local Capsule store",
        badge: "PICKER",
        dangerous: false,
    },
    CommandPaletteEntry {
        command: "verify",
        aliases: &["v", "preflight"],
        description: "Run non-executing verifier checks for the current Capsule",
        params: "target CLI",
        badge: "CHECK",
        dangerous: false,
    },
    CommandPaletteEntry {
        command: "doctor",
        aliases: &["D", "diag", "health", "pre-flight"],
        description: "Open Pre-flight evidence for compiler, Doctor, and verifier",
        params: "no args",
        badge: "CHECK",
        dangerous: false,
    },
    CommandPaletteEntry {
        command: "hooks",
        aliases: &["hook status", "events", "spool"],
        description: "Open opt-in hook event channel status",
        params: "disabled by default",
        badge: "CHECK",
        dangerous: false,
    },
    CommandPaletteEntry {
        command: "settings",
        aliases: &["prefs", "preferences", "smart enter", "tmux jump"],
        description: "Configure opt-in TUI behavior",
        params: "Smart Enter / tmux jump",
        badge: "SAFE",
        dangerous: false,
    },
    CommandPaletteEntry {
        command: "source next",
        aliases: &["filter", "filter next", "source"],
        description: "Switch to the next session source filter",
        params: "All, Starred, Archived, Codex, Claude, Hermes",
        badge: "SWITCH",
        dangerous: false,
    },
    CommandPaletteEntry {
        command: "source prev",
        aliases: &["filter prev", "filter previous", "source previous"],
        description: "Switch to the previous session source filter",
        params: "All, Starred, Archived, Codex, Claude, Hermes",
        badge: "SWITCH",
        dangerous: false,
    },
    CommandPaletteEntry {
        command: "source codex",
        aliases: &["filter codex", "codex"],
        description: "Show only Codex sessions",
        params: "source filter",
        badge: "SWITCH",
        dangerous: false,
    },
    CommandPaletteEntry {
        command: "source claude",
        aliases: &["filter claude", "claude"],
        description: "Show only Claude sessions",
        params: "source filter",
        badge: "SWITCH",
        dangerous: false,
    },
    CommandPaletteEntry {
        command: "source hermes",
        aliases: &["filter hermes", "hermes"],
        description: "Show only Hermes sessions",
        params: "source filter",
        badge: "SWITCH",
        dangerous: false,
    },
    CommandPaletteEntry {
        command: "starred",
        aliases: &["filter star", "filter starred"],
        description: "Show starred sessions only",
        params: "source filter",
        badge: "SWITCH",
        dangerous: false,
    },
    CommandPaletteEntry {
        command: "archived",
        aliases: &["filter archived", "archive filter"],
        description: "Show archived sessions only",
        params: "archive overlay",
        badge: "SWITCH",
        dangerous: false,
    },
    CommandPaletteEntry {
        command: "archive",
        aliases: &["a", "unarchive"],
        description: "Archive or unarchive the selected session in the Moonbox overlay",
        params: "selected session",
        badge: "OVERLAY",
        dangerous: false,
    },
    CommandPaletteEntry {
        command: "clear",
        aliases: &["all", "filter all", "filter clear"],
        description: "Clear search and source filters",
        params: "no args",
        badge: "SAFE",
        dangerous: false,
    },
    CommandPaletteEntry {
        command: "data",
        aliases: &["spaces", "dataspace"],
        description: "Open the Local / SSH data space picker",
        params: "Local or saved SSH spaces",
        badge: "PICKER",
        dangerous: false,
    },
    CommandPaletteEntry {
        command: "data next",
        aliases: &["space next", "dataspace next"],
        description: "Switch to the next configured data space",
        params: "Local or saved SSH spaces",
        badge: "SWITCH",
        dangerous: false,
    },
    CommandPaletteEntry {
        command: "data prev",
        aliases: &["data previous", "space prev", "dataspace prev"],
        description: "Switch to the previous configured data space",
        params: "Local or saved SSH spaces",
        badge: "SWITCH",
        dangerous: false,
    },
    CommandPaletteEntry {
        command: "skill",
        aliases: &["compiler"],
        description: "Open the compiler Skill Picker",
        params: "compiler skill",
        badge: "PICKER",
        dangerous: false,
    },
    CommandPaletteEntry {
        command: "help",
        aliases: &["?"],
        description: "Open keyboard help",
        params: "no args",
        badge: "SAFE",
        dangerous: false,
    },
    CommandPaletteEntry {
        command: "quit",
        aliases: &["q", "exit"],
        description: "Exit Moonbox without launching a session",
        params: "no args",
        badge: "EXIT",
        dangerous: true,
    },
];

fn command_palette_matches(query: &str) -> Vec<&'static CommandPaletteEntry> {
    let query = normalize_command(query);
    let mut entries = COMMAND_PALETTE_ENTRIES
        .iter()
        .enumerate()
        .filter(|entry| command_palette_entry_matches(&query, entry))
        .collect::<Vec<_>>();
    entries.sort_by_key(|(index, entry)| (command_palette_rank(&query, entry), *index));
    entries.into_iter().map(|(_, entry)| entry).collect()
}

fn resolve_command_palette_entry(query: &str) -> Option<&'static CommandPaletteEntry> {
    let query = normalize_command(query);
    COMMAND_PALETTE_ENTRIES.iter().find(|entry| {
        normalize_command(entry.command) == query
            || entry
                .aliases
                .iter()
                .any(|alias| normalize_command(alias) == query)
    })
}

fn command_palette_entry_matches(query: &str, (_, entry): &(usize, &CommandPaletteEntry)) -> bool {
    if query.is_empty() {
        return true;
    }
    command_palette_search_text(entry)
        .iter()
        .any(|item| item.contains(query) || fuzzy_contains(item, query))
}

fn command_palette_rank(query: &str, entry: &CommandPaletteEntry) -> usize {
    if query.is_empty() {
        return 10;
    }
    let command = normalize_command(entry.command);
    if command == query {
        return 0;
    }
    if entry
        .aliases
        .iter()
        .any(|alias| normalize_command(alias) == query)
    {
        return 1;
    }
    if command.starts_with(query) {
        return 2;
    }
    if entry
        .aliases
        .iter()
        .any(|alias| normalize_command(alias).starts_with(query))
    {
        return 3;
    }
    if command.contains(query) {
        return 4;
    }
    if fuzzy_contains(&command, query) {
        return 5;
    }
    if entry
        .aliases
        .iter()
        .any(|alias| fuzzy_contains(&normalize_command(alias), query))
    {
        return 6;
    }
    if normalize_command(entry.description).contains(query)
        || normalize_command(entry.params).contains(query)
    {
        return 7;
    }
    8
}

fn command_palette_search_text(entry: &CommandPaletteEntry) -> Vec<String> {
    let mut text = vec![
        normalize_command(entry.command),
        normalize_command(entry.description),
        normalize_command(entry.params),
        normalize_command(entry.badge),
    ];
    text.extend(entry.aliases.iter().map(|alias| normalize_command(alias)));
    text
}

fn normalize_command(value: &str) -> String {
    value
        .trim()
        .to_ascii_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn fuzzy_contains(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    let mut chars = needle.chars();
    let Some(mut wanted) = chars.next() else {
        return true;
    };
    for ch in haystack.chars() {
        if ch == wanted {
            match chars.next() {
                Some(next) => wanted = next,
                None => return true,
            }
        }
    }
    false
}

fn sync_compiler_ids_with_catalog(
    data: &mut WorkbenchData,
    catalog: &[CompilerPresetInfo],
    preserved_ids: impl IntoIterator<Item = String>,
) {
    let mut compilers = catalog
        .iter()
        .map(|entry| entry.id.clone())
        .collect::<Vec<_>>();
    let existing_ids = data.compilers.clone();
    for id in preserved_ids.into_iter().chain(existing_ids) {
        if !id.trim().is_empty() && !compilers.contains(&id) {
            compilers.push(id);
        }
    }
    data.compilers = compilers;
}

#[derive(Debug)]
pub struct App {
    pub data: WorkbenchData,
    pub compiler_catalog: Vec<CompilerPresetInfo>,
    pub focus: Focus,
    pub zoomed_focus: Option<Focus>,
    pub selected_session: usize,
    pub selected_event: usize,
    pub selected_compiler: usize,
    pub command_mode: bool,
    pub command_input: String,
    pub command_selection: usize,
    pub command_selection_active: bool,
    pub show_help: bool,
    pub show_launch: bool,
    pub launch_review: bool,
    pub launch_review_details: bool,
    pub show_action_menu: bool,
    pub action_menu_selection: usize,
    pub show_share_panel: bool,
    pub show_lark_export: bool,
    pub launch_review_lark_export: bool,
    pub share_panel_selection: usize,
    pub show_open_original: bool,
    pub show_doctor: bool,
    pub show_skill_picker: bool,
    pub show_capsules: bool,
    pub show_settings: bool,
    pub show_data_spaces: bool,
    pub show_data_space_config: bool,
    pub show_timeline_detail: bool,
    pub target_launch_result: Option<TargetLaunchResult>,
    launch_review_error: Option<LaunchReviewErrorState>,
    pub timeline_image_previews: Vec<TimelineImagePreview>,
    pub saved_capsules: Vec<CapsuleSummary>,
    pub saved_capsule_error: Option<String>,
    pub session_filter: SessionFilter,
    pub starred_sessions: Vec<String>,
    pub archived_sessions: Vec<String>,
    pub search_query: String,
    visible_session_indices: Vec<usize>,
    pub data_spaces: Vec<dataspace::DataSpaceEntry>,
    pub selected_data_space: usize,
    pub data_space_selection: usize,
    pub data_space_error: Option<String>,
    pub data_space_config_form: DataSpaceConfigForm,
    pub data_space_config_field: usize,
    pub data_space_delete_confirmation: Option<String>,
    ui_preferences: config::UiPreferencesConfig,
    pub settings_language: config::UiLanguage,
    pub settings_theme: config::UiThemeName,
    pub settings_field: SettingsField,
    pub settings_smart_enter_tmux: bool,
    pub lark_cli_readiness: lark::LarkCliReadiness,
    pub pending_target: CliTool,
    pub pending_compiler: usize,
    pub status_message: String,
    pub rewind_event_id: String,
    pub capsule_scroll: u16,
    pub modal_scroll: u16,
    pub verify_passed: bool,
    pub doctor_report: DoctorReport,
    pub compile_status: &'static str,
    pub pending_g: bool,
    animation_tick: usize,
    session_load_request_id: u64,
    pending_session_load: Option<PendingSessionLoad>,
    session_preview_request_id: u64,
    pending_session_preview: Option<PendingSessionPreview>,
    deferred_session_preview: Option<DeferredSessionPreview>,
    data_space_load_request_id: u64,
    pending_data_space_load: Option<PendingDataSpaceLoad>,
    launch_review_request_id: u64,
    pending_launch_review: Option<PendingLaunchReview>,
    pending_share_handoff_copy: bool,
    handoff_trail: Option<HandoffTrail>,
    pending_archive_feedback: Option<PendingArchiveFeedback>,
    hooks_config: config::HooksConfig,
    hook_live: Option<hooks::HookLiveState>,
    clipboard_text: Option<String>,
    pending_resume: Option<Box<OriginalSessionPlan>>,
    pending_native_fork: Option<Box<OriginalSessionPlan>>,
    pending_seed_prompt: Option<Box<OriginalSessionPlan>>,
    pending_launch: Option<Box<LaunchPlan>>,
    pending_tmux_jump: Option<Box<TmuxJumpPlan>>,
    pending_setup_install: Option<Box<SetupInstallPlan>>,
    pending_lark_export: Option<Box<LarkExportTuiPlan>>,
    pub lark_export_plan: Option<lark::LarkExportPlan>,
    exit_action: Option<TuiExitAction>,
    should_quit: bool,
}

impl App {
    pub fn new(source: CliTool, target: CliTool) -> Result<Self, CoreError> {
        let data = workbench::load_workbench(source, target)?;
        Ok(Self::from_data(data, target))
    }

    pub fn new_fixture(source: CliTool, target: CliTool) -> Result<Self, CoreError> {
        let data = workbench::load_fixture_workbench(source, target)?;
        Ok(Self::from_data(data, target))
    }

    fn from_data(mut data: WorkbenchData, target: CliTool) -> Self {
        let compiler_catalog = compiler::compiler_catalog_entries();
        let selected_compiler_id = data.capsule.compiler.clone();
        sync_compiler_ids_with_catalog(&mut data, &compiler_catalog, [selected_compiler_id]);
        let rewind_event_id = initial_rewind_event_id(&data);
        let selected_session = data
            .sessions
            .iter()
            .position(|session| session.id == data.capsule.source_session)
            .unwrap_or(0);
        let selected_event = rewind_event_index(&data, &rewind_event_id);
        let selected_compiler = compiler_index_for_id(&data, &data.capsule.compiler, 0);
        let doctor_report = doctor::diagnose_with_inventory(&data.sessions, &data.source_adapters);
        #[cfg(not(test))]
        let hooks_config = config::load_hooks_config();
        #[cfg(test)]
        let hooks_config = config::HooksConfig::default();
        #[cfg(not(test))]
        let ui_preferences = config::load_ui_preferences_config();
        #[cfg(test)]
        let ui_preferences = config::UiPreferencesConfig::default();
        #[cfg(not(test))]
        let hook_live = hooks::live_state_from_config(&hooks_config);
        #[cfg(test)]
        let hook_live = None;
        let settings_smart_enter_tmux = hooks_config.smart_enter_tmux;
        let settings_language = ui_preferences.language;
        let settings_theme = ui_preferences.theme;
        let lark_cli_readiness = lark::readiness(Some(
            setup::setup_command_display_for_current_exe(setup::SetupInstallTarget::LarkCli),
        ));
        #[cfg(not(test))]
        let starred_sessions = config::load_starred_sessions();
        #[cfg(test)]
        let starred_sessions = Vec::new();
        #[cfg(not(test))]
        let archived_sessions = config::load_archived_sessions();
        #[cfg(test)]
        let archived_sessions = Vec::new();

        let session_details_loaded =
            !data.timeline.is_empty() && data.capsule.state != "pending_rewind";
        let mut app = Self {
            data,
            compiler_catalog,
            focus: Focus::Sessions,
            zoomed_focus: None,
            selected_session,
            selected_event,
            selected_compiler,
            command_mode: false,
            command_input: String::new(),
            command_selection: 0,
            command_selection_active: false,
            show_help: false,
            show_launch: false,
            launch_review: false,
            launch_review_details: false,
            show_action_menu: false,
            action_menu_selection: 0,
            show_share_panel: false,
            show_lark_export: false,
            launch_review_lark_export: false,
            share_panel_selection: 0,
            show_open_original: false,
            show_doctor: false,
            show_skill_picker: false,
            show_capsules: false,
            show_settings: false,
            show_data_spaces: false,
            show_data_space_config: false,
            show_timeline_detail: false,
            target_launch_result: None,
            launch_review_error: None,
            timeline_image_previews: Vec::new(),
            saved_capsules: Vec::new(),
            saved_capsule_error: None,
            session_filter: SessionFilter::All,
            starred_sessions,
            archived_sessions,
            search_query: String::new(),
            data_spaces: dataspace::list_data_spaces(),
            selected_data_space: 0,
            data_space_selection: 0,
            data_space_error: None,
            data_space_config_form: DataSpaceConfigForm::default(),
            data_space_config_field: 0,
            data_space_delete_confirmation: None,
            ui_preferences,
            settings_language,
            settings_theme,
            settings_field: SettingsField::Language,
            settings_smart_enter_tmux,
            lark_cli_readiness,
            pending_target: target,
            pending_compiler: selected_compiler,
            status_message: if session_details_loaded {
                "Ready".into()
            } else {
                "Ready: session details load on demand".into()
            },
            rewind_event_id,
            capsule_scroll: 0,
            modal_scroll: 0,
            verify_passed: session_details_loaded,
            doctor_report,
            compile_status: if session_details_loaded {
                "ACTIVE"
            } else {
                "PENDING"
            },
            pending_g: false,
            animation_tick: 0,
            session_load_request_id: 0,
            pending_session_load: None,
            session_preview_request_id: 0,
            pending_session_preview: None,
            deferred_session_preview: None,
            data_space_load_request_id: 0,
            pending_data_space_load: None,
            launch_review_request_id: 0,
            pending_launch_review: None,
            pending_share_handoff_copy: false,
            handoff_trail: None,
            pending_archive_feedback: None,
            hooks_config,
            hook_live,
            clipboard_text: None,
            pending_resume: None,
            pending_native_fork: None,
            pending_seed_prompt: None,
            pending_launch: None,
            pending_tmux_jump: None,
            pending_setup_install: None,
            pending_lark_export: None,
            lark_export_plan: None,
            exit_action: None,
            should_quit: false,
            visible_session_indices: Vec::new(),
        };
        app.refresh_visible_sessions();
        if let Some(hook_live) = app.hook_live.as_mut() {
            hook_live.replay_existing();
        }
        if let Some(session) = app.current_session() {
            app.schedule_selected_session_preview(session.id.clone());
        }
        app
    }

    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    pub fn advance_animation(&mut self) {
        self.animation_tick = self.animation_tick.wrapping_add(1);
        self.advance_archive_feedback();
    }

    pub fn animation_tick(&self) -> usize {
        self.animation_tick
    }

    pub fn take_clipboard_text(&mut self) -> Option<String> {
        self.clipboard_text.take()
    }

    pub fn take_pending_resume(&mut self) -> Option<Box<OriginalSessionPlan>> {
        self.pending_resume.take()
    }

    pub fn take_pending_native_fork(&mut self) -> Option<Box<OriginalSessionPlan>> {
        self.pending_native_fork.take()
    }

    pub fn take_pending_seed_prompt(&mut self) -> Option<Box<OriginalSessionPlan>> {
        self.pending_seed_prompt.take()
    }

    pub fn take_pending_launch(&mut self) -> Option<Box<LaunchPlan>> {
        self.pending_launch.take()
    }

    pub fn take_pending_tmux_jump(&mut self) -> Option<Box<TmuxJumpPlan>> {
        self.pending_tmux_jump.take()
    }

    pub fn take_pending_setup_install(&mut self) -> Option<Box<SetupInstallPlan>> {
        self.pending_setup_install.take()
    }

    pub fn take_pending_lark_export(&mut self) -> Option<Box<LarkExportTuiPlan>> {
        self.pending_lark_export.take()
    }

    pub fn take_exit_action(&mut self) -> Option<TuiExitAction> {
        self.exit_action.take()
    }

    pub fn complete_setup_install(
        &mut self,
        plan: &SetupInstallPlan,
        outcome: String,
        success: bool,
    ) {
        if success {
            self.refresh_lark_cli_readiness();
            let selected_id = plan
                .compiler_id
                .clone()
                .or_else(|| self.data.compilers.get(self.selected_compiler).cloned());
            self.refresh_compiler_catalog();
            if let Some(selected_id) = selected_id {
                self.selected_compiler =
                    compiler_index_for_equivalent_id(&self.data, &selected_id, self.data.target, 0);
                self.pending_compiler = self.selected_compiler;
                if let Some(compiler) = self.data.compilers.get(self.selected_compiler).cloned() {
                    self.data.capsule.compiler = compiler;
                }
            }
            self.launch_review_error = None;
        }
        if plan.target == setup::SetupInstallTarget::LarkCli {
            self.refresh_lark_cli_readiness();
        }
        self.set_status(format!("{}: {outcome}", plan.label));
        self.pending_g = false;
    }

    pub fn complete_lark_export(
        &mut self,
        plan: &LarkExportTuiPlan,
        outcome: String,
        success: bool,
    ) {
        self.show_lark_export = false;
        self.lark_export_plan = None;
        self.refresh_lark_cli_readiness();
        self.set_status(format!(
            "Lark export {}: {}",
            if success { "completed" } else { "failed" },
            outcome
        ));
        self.pending_g = false;
        if success {
            self.modal_scroll = 0;
        } else {
            self.show_action_menu = true;
            self.set_status(format!(
                "Lark export failed for {}: {outcome}",
                plan.session_id
            ));
        }
    }

    pub fn complete_tmux_jump(&mut self, plan: Box<TmuxJumpPlan>, result: Result<(), String>) {
        match result {
            Ok(()) => self.set_status(format!(
                "Jumped to pane {} for {} {}",
                plan.command.pane_id, plan.source_session.cli, plan.source_session.id
            )),
            Err(reason) => {
                self.queue_original_resume_with_status(format!(
                    "Tmux jump unavailable: {reason}; falling back to resume"
                ));
            }
        }
        self.pending_g = false;
    }

    pub fn complete_target_handoff(
        &mut self,
        plan: Box<LaunchPlan>,
        result: Result<LaunchExecution, CoreError>,
    ) {
        let (outcome, success) = match result {
            Ok(execution) => match execution.status {
                LaunchExecutionStatus::Success => (
                    execution
                        .exit_code
                        .map(|code| {
                            format!("{} exited successfully (code {code})", plan.target_cli)
                        })
                        .unwrap_or_else(|| format!("{} exited successfully", plan.target_cli)),
                    true,
                ),
                LaunchExecutionStatus::Failed => (
                    execution
                        .exit_code
                        .map(|code| format!("{} exited with code {code}", plan.target_cli))
                        .unwrap_or_else(|| {
                            format!("{} exited without a success code", plan.target_cli)
                        }),
                    false,
                ),
            },
            Err(error) => (
                format!("{} failed to start: {error}", plan.target_cli),
                false,
            ),
        };
        self.target_launch_result = Some(TargetLaunchResult {
            target: plan.target_cli,
            source: plan.source_session.cli,
            session_id: plan.source_session.id.clone(),
            command: plan.target_command.display.clone(),
            command_summary: launcher::concise_command_display(&plan.target_command),
            outcome: outcome.clone(),
            success,
            plan,
        });
        self.show_launch = true;
        self.launch_review = false;
        self.launch_review_details = false;
        self.launch_review_error = None;
        self.modal_scroll = 0;
        self.clear_handoff_trail();
        self.set_status(outcome);
        self.pending_g = false;
    }

    pub fn complete_original_resume(&mut self, plan: &OriginalSessionPlan, outcome: String) {
        let selected_session_id = self
            .current_session()
            .map(|session| session.id.clone())
            .unwrap_or_else(|| plan.source_session.id.clone());
        let selected_event_id = self
            .data
            .timeline
            .get(self.selected_event)
            .map(|event| event.id.clone());
        let selected_compiler = self.selected_compiler;
        let selected_compiler_id = self
            .data
            .compilers
            .get(selected_compiler)
            .cloned()
            .unwrap_or_else(|| self.data.capsule.compiler.clone());
        let rewind_event_id = self.rewind_event_id.clone();

        if !self.current_data_space().is_local() {
            self.set_status(format!("{outcome}; remote data space not reloaded"));
            self.pending_g = false;
            return;
        }

        match workbench::load_workbench_for_session(&plan.source_session.id, self.data.target) {
            Ok(Some(data)) => {
                self.data = data;
                self.refresh_visible_sessions();
                self.selected_session = self
                    .data
                    .sessions
                    .iter()
                    .position(|session| session.id == selected_session_id)
                    .or_else(|| {
                        self.data
                            .sessions
                            .iter()
                            .position(|session| session.id == plan.source_session.id)
                    })
                    .unwrap_or(self.selected_session)
                    .min(self.data.sessions.len().saturating_sub(1));
                self.clamp_selected_session();
                self.selected_event = selected_event_id
                    .and_then(|id| self.data.timeline.iter().position(|event| event.id == id))
                    .unwrap_or_else(|| {
                        self.selected_event
                            .min(self.data.timeline.len().saturating_sub(1))
                    });
                self.selected_compiler =
                    compiler_index_for_id(&self.data, &selected_compiler_id, selected_compiler);
                if let Some(compiler) = self.data.compilers.get(self.selected_compiler) {
                    self.data.capsule.compiler = compiler.clone();
                }
                if let Some(title) = self.timeline_event_title(&rewind_event_id) {
                    self.apply_rewind_event(rewind_event_id, title);
                } else {
                    self.rewind_event_id = initial_rewind_event_id(&self.data);
                }
                self.doctor_report = doctor::diagnose_with_inventory(
                    &self.data.sessions,
                    &self.data.source_adapters,
                );
                self.compile_status = "ACTIVE";
                self.verify_passed = true;
                self.set_status(format!(
                    "{outcome}; session reloaded ({} events)",
                    self.data.timeline.len()
                ));
            }
            Ok(None) => {
                self.set_status(format!(
                    "{outcome}; session {} not found after resume",
                    plan.source_session.id
                ));
            }
            Err(error) => {
                self.compile_status = "FAILED";
                self.verify_passed = false;
                self.set_status(format!("{outcome}; reload failed: {error}"));
            }
        }
        self.pending_g = false;
    }

    pub fn is_session_load_pending(&self) -> bool {
        self.pending_session_load.is_some()
    }

    pub fn is_session_preview_pending(&self) -> bool {
        self.pending_session_preview.is_some() || self.deferred_session_preview.is_some()
    }

    pub fn selected_session_timeline_loaded(&self) -> bool {
        self.current_session().is_some_and(|session| {
            self.data.capsule.source_session == session.id
                && self.data.capsule.source_cli == session.cli
                && !self.data.timeline.is_empty()
        })
    }

    pub fn selected_session_context_loaded(&self) -> bool {
        self.current_session().is_some_and(|session| {
            self.data.capsule.source_session == session.id
                && self.data.capsule.source_cli == session.cli
                && self.data.capsule.state != "pending_rewind"
        })
    }

    pub fn is_launch_review_pending(&self) -> bool {
        self.pending_launch_review.is_some()
    }

    pub fn launch_review_job_status(&self) -> Option<LaunchReviewJobStatus> {
        self.pending_launch_review
            .as_ref()
            .map(|pending| LaunchReviewJobStatus {
                stage: pending.stage,
                stage_label: pending.stage.label(),
                detail: pending.stage_detail.clone(),
                target: pending.target,
                session_id: pending.session_id.clone(),
                compiler_id: pending.compiler_id.clone(),
                elapsed_ms: pending.started_at.elapsed().as_millis(),
                timeout_ms: pending.timeout_ms,
            })
    }

    pub fn launch_review_error(&self) -> Option<&LaunchReviewErrorState> {
        self.launch_review_error.as_ref()
    }

    #[cfg(test)]
    pub(crate) fn set_launch_review_error_for_test(&mut self, error: LaunchReviewErrorState) {
        self.launch_review_error = Some(error);
    }

    #[cfg(test)]
    pub(crate) fn set_hooks_config_for_test(&mut self, hooks_config: config::HooksConfig) {
        self.settings_smart_enter_tmux = hooks_config.smart_enter_tmux;
        self.hooks_config = hooks_config;
    }

    pub(crate) fn set_ui_preferences_for_render(&mut self, ui: config::UiPreferencesConfig) {
        let ui = config::UiPreferencesConfig {
            language: ui.language,
            theme: ui.theme.normalized_for_ui(),
        };
        self.ui_preferences = ui;
        self.settings_language = ui.language;
        self.settings_theme = ui.theme;
    }

    #[cfg(test)]
    pub(crate) fn set_ui_preferences_for_test(&mut self, ui: config::UiPreferencesConfig) {
        self.set_ui_preferences_for_render(ui);
    }

    #[cfg(test)]
    pub(crate) fn set_hook_live_events_for_test(&mut self, events: Vec<hooks::HookSpoolEvent>) {
        let mut hook_live =
            hooks::HookLiveState::new(std::path::PathBuf::from("/tmp/moonbox-test-hooks.jsonl"));
        for event in events {
            hook_live.apply_event_for_test(event);
        }
        self.hook_live = Some(hook_live);
    }

    pub fn hook_live_indicator(&self) -> Option<hooks::HookLiveIndicator> {
        let hook_live = self.hook_live.as_ref()?;
        if !self.current_data_space().is_local() {
            return Some(hooks::HookLiveIndicator {
                label: "Live unavailable: SSH data".into(),
                is_error: false,
                is_stale: true,
            });
        }
        Some(hook_live.indicator(hooks::current_millis()))
    }

    pub fn hooks_enabled(&self) -> bool {
        self.hooks_config.enabled
    }

    pub fn smart_enter_tmux_enabled(&self) -> bool {
        self.hooks_config.smart_enter_tmux
    }

    pub fn settings_smart_enter_dirty(&self) -> bool {
        self.settings_smart_enter_tmux != self.hooks_config.smart_enter_tmux
    }

    pub fn ui_language(&self) -> config::UiLanguage {
        self.ui_preferences.language
    }

    pub fn ui_theme(&self) -> config::UiThemeName {
        self.ui_preferences.theme
    }

    pub fn effective_language(&self) -> config::UiLanguage {
        if self.show_settings {
            self.settings_language
        } else {
            self.ui_language()
        }
    }

    pub fn effective_theme(&self) -> config::UiThemeName {
        if self.show_settings {
            self.settings_theme
        } else {
            self.ui_theme()
        }
    }

    pub fn settings_language_dirty(&self) -> bool {
        self.settings_language != self.ui_preferences.language
    }

    pub fn settings_theme_dirty(&self) -> bool {
        self.settings_theme != self.ui_preferences.theme
    }

    pub fn settings_dirty(&self) -> bool {
        self.settings_language_dirty()
            || self.settings_theme_dirty()
            || self.settings_smart_enter_dirty()
    }

    pub fn settings_field_is_focused(&self, field: SettingsField) -> bool {
        self.settings_field == field
    }

    pub fn hook_live_for_session(
        &self,
        session: &SessionSummary,
    ) -> Option<&hooks::HookSessionLiveInfo> {
        if !self.current_data_space().is_local() {
            return None;
        }
        self.hook_live.as_ref()?.session_for(
            session.cli,
            &session.id,
            session.source_path.as_deref(),
        )
    }

    pub fn hook_waiting_items(&self) -> Vec<HookWaitingItem> {
        let Some(hook_live) = self.hook_live.as_ref() else {
            return Vec::new();
        };
        if !self.current_data_space().is_local() {
            return Vec::new();
        }
        let now_ms = hooks::current_millis();
        hook_live
            .waiting_sessions()
            .into_iter()
            .map(|session| {
                let title = self
                    .data
                    .sessions
                    .iter()
                    .find(|candidate| {
                        candidate.cli == session.cli
                            && (candidate.id == session.session_id
                                || candidate.source_path.as_deref()
                                    == session.transcript_path.as_deref())
                    })
                    .map(|session| session.title.clone())
                    .unwrap_or_else(|| session.session_id.clone());
                HookWaitingItem {
                    cli: session.cli,
                    session_id: session.session_id.clone(),
                    title,
                    reason: session
                        .wait_reason
                        .clone()
                        .unwrap_or_else(|| session.summary.clone()),
                    waiting_for_ms: now_ms.saturating_sub(session.status_since_ms),
                    cwd: session.cwd.clone(),
                    tmux_pane: session.tmux_pane.clone(),
                }
            })
            .collect()
    }

    pub fn enter_key_hint(&self) -> &'static str {
        self.current_session()
            .map(|session| self.enter_route_preview(session).label)
            .unwrap_or("Unavailable")
    }

    pub fn enter_route_preview(&self, session: &SessionSummary) -> EnterRoutePreview {
        self.enter_route_preview_for(session, self.hooks_config.smart_enter_tmux)
    }

    pub fn settings_enter_route_preview(&self) -> Option<EnterRoutePreview> {
        self.current_session()
            .map(|session| self.enter_route_preview_for(session, self.settings_smart_enter_tmux))
    }

    pub fn session_actions(&self, session: &SessionSummary) -> SessionActionSet {
        self.session_actions_for(session, self.hooks_config.smart_enter_tmux)
    }

    pub fn action_menu_entries(&self) -> Vec<ActionMenuEntry> {
        let Some(session) = self.current_session() else {
            return Vec::new();
        };
        let actions = self.session_actions(session);
        action_menu_order()
            .into_iter()
            .filter_map(|kind| actions.action(kind).cloned())
            .enumerate()
            .map(|(index, mut action)| {
                if action.kind == SessionAvailableActionKind::Archive {
                    let archived = self.is_session_archived(session);
                    action.status = SessionActionAvailability::Available;
                    action.label = if archived {
                        "Unarchive".into()
                    } else {
                        "Archive".into()
                    };
                    action.reason = if archived {
                        "Remove this session from the Moonbox archive overlay.".into()
                    } else {
                        "Archive this session in the Moonbox overlay without touching the provider store."
                            .into()
                    };
                    action.keys = vec!["a".into()];
                }
                ActionMenuEntry {
                    runnable: action_is_runnable(action.status),
                    selected: index == self.action_menu_selection,
                    action,
                }
            })
            .collect()
    }

    pub fn share_panel_entries(&self) -> Vec<SharePanelEntry> {
        SharePanelActionKind::ALL
            .into_iter()
            .enumerate()
            .map(|(index, kind)| {
                let (status, reason) = self.share_action_state(kind);
                SharePanelEntry {
                    kind,
                    selected: index == self.share_panel_selection,
                    runnable: action_is_runnable(status),
                    status,
                    reason,
                }
            })
            .collect()
    }

    fn share_action_state(
        &self,
        kind: SharePanelActionKind,
    ) -> (SessionActionAvailability, String) {
        match kind {
            SharePanelActionKind::FirstUserInput => {
                if self.first_user_input().is_some() {
                    (
                        SessionActionAvailability::Available,
                        "Copy the first user message from the loaded timeline.".into(),
                    )
                } else if self.is_session_load_pending() || self.is_session_preview_pending() {
                    (
                        SessionActionAvailability::Unavailable,
                        "Timeline is still loading; try again after the preview is ready.".into(),
                    )
                } else if self.selected_session_timeline_loaded() {
                    (
                        SessionActionAvailability::Unavailable,
                        "Loaded timeline has no user input to copy.".into(),
                    )
                } else {
                    (
                        SessionActionAvailability::Unavailable,
                        "Load session details before copying the first user input.".into(),
                    )
                }
            }
            SharePanelActionKind::LastAiOutput => {
                if self.last_ai_output().is_some() {
                    (
                        SessionActionAvailability::Available,
                        "Copy the latest assistant message from the loaded timeline.".into(),
                    )
                } else if self.is_session_load_pending() || self.is_session_preview_pending() {
                    (
                        SessionActionAvailability::Unavailable,
                        "Timeline is still loading; try again after the preview is ready.".into(),
                    )
                } else if self.selected_session_timeline_loaded() {
                    (
                        SessionActionAvailability::Unavailable,
                        "Loaded timeline has no assistant output to copy.".into(),
                    )
                } else {
                    (
                        SessionActionAvailability::Unavailable,
                        "Load session details before copying the latest assistant output.".into(),
                    )
                }
            }
            SharePanelActionKind::SessionId => {
                if self.current_session().is_some() {
                    (
                        SessionActionAvailability::Available,
                        "Copy the selected provider session id.".into(),
                    )
                } else {
                    (
                        SessionActionAvailability::Unavailable,
                        "No session is selected.".into(),
                    )
                }
            }
            SharePanelActionKind::HandoffContent => {
                if self.selected_handoff_artifact().is_some() {
                    (
                        SessionActionAvailability::Available,
                        "Copy the ready handoff artifact without launching a target session."
                            .into(),
                    )
                } else if self.is_launch_review_pending() {
                    (
                        SessionActionAvailability::Unavailable,
                        "Handoff generation is already running.".into(),
                    )
                } else {
                    (
                        SessionActionAvailability::Available,
                        "Generate a handoff artifact, then copy it without launching the target."
                            .into(),
                    )
                }
            }
            SharePanelActionKind::PortableJson => {
                if self.current_session().is_none() {
                    (
                        SessionActionAvailability::Unavailable,
                        "No session is selected.".into(),
                    )
                } else if self.is_session_load_pending() || self.is_session_preview_pending() {
                    (
                        SessionActionAvailability::Unavailable,
                        "Timeline is still loading; compact JSON waits for loaded session context."
                            .into(),
                    )
                } else {
                    (
                        SessionActionAvailability::Available,
                        "Copy a compact Moonbox JSON envelope for this selected session.".into(),
                    )
                }
            }
        }
    }

    pub fn selected_action_menu_entry(&self) -> Option<ActionMenuEntry> {
        self.action_menu_entries()
            .into_iter()
            .nth(self.action_menu_selection)
    }

    fn session_actions_for(
        &self,
        session: &SessionSummary,
        smart_enter_tmux: bool,
    ) -> SessionActionSet {
        actions::session_action_set(
            session,
            &self.session_action_context_for(session, smart_enter_tmux),
        )
    }

    fn session_action_context_for(
        &self,
        session: &SessionSummary,
        smart_enter_tmux: bool,
    ) -> SessionActionContext {
        let local_data_space = self.current_data_space().is_local();
        let live = if local_data_space {
            self.hook_live_for_session(session)
                .map(|live| SessionActionLiveContext {
                    status: session_action_live_status(live.status),
                    tmux_target: tmux::target_from_hook(
                        live.tmux.as_deref(),
                        live.tmux_pane.as_deref(),
                    )
                    .map(|target| target.pane_id),
                })
        } else {
            None
        };
        SessionActionContext {
            local_data_space,
            hooks_enabled: self.hooks_config.enabled,
            smart_enter_tmux,
            live,
        }
    }

    fn enter_route_preview_for(
        &self,
        session: &SessionSummary,
        smart_enter_tmux: bool,
    ) -> EnterRoutePreview {
        let actions = if smart_enter_tmux == self.hooks_config.smart_enter_tmux {
            self.session_actions(session)
        } else {
            self.session_actions_for(session, smart_enter_tmux)
        };
        let handoff = actions.action(SessionAvailableActionKind::Handoff);
        let resume = actions.action(SessionAvailableActionKind::Resume);
        let jump = actions.action(SessionAvailableActionKind::Jump);
        if let Some(handoff) = handoff.filter(|action| {
            action.status == SessionActionAvailability::Available
                && !self.current_data_space().is_local()
        }) {
            return EnterRoutePreview {
                kind: EnterRouteKind::Handoff,
                label: "Handoff",
                detail: handoff.reason.clone(),
            };
        }
        if !self.hooks_config.enabled {
            return EnterRoutePreview {
                kind: EnterRouteKind::Disabled,
                label: "Resume",
                detail: jump.map(|action| action.reason.clone()).unwrap_or_else(|| {
                    "Hooks are disabled; Enter keeps the existing resume path".into()
                }),
            };
        }
        if !smart_enter_tmux {
            return EnterRoutePreview {
                kind: EnterRouteKind::Disabled,
                label: "Resume",
                detail: jump
                    .map(|action| action.reason.clone())
                    .unwrap_or_else(|| "Smart Enter / tmux jump is disabled in Settings".into()),
            };
        }
        let Some(jump) = jump else {
            return EnterRoutePreview {
                kind: EnterRouteKind::Resume,
                label: "Resume",
                detail: resume
                    .map(|action| action.reason.clone())
                    .unwrap_or_else(|| "Local provider resume is available.".into()),
            };
        };
        if jump.status == SessionActionAvailability::Available {
            return EnterRoutePreview {
                kind: EnterRouteKind::Jump,
                label: "Jump",
                detail: jump.reason.clone(),
            };
        }
        if let Some(live) = self.hook_live_for_session(session) {
            if live.status == hooks::HookSessionStatus::Dead {
                return EnterRoutePreview {
                    kind: EnterRouteKind::Resume,
                    label: "Resume",
                    detail: format!("{}; Enter resumes normally", jump.reason),
                };
            }
            return EnterRoutePreview {
                kind: EnterRouteKind::Unavailable,
                label: "Resume",
                detail: format!("{}; Enter falls back to resume", jump.reason),
            };
        }
        EnterRoutePreview {
            kind: EnterRouteKind::Resume,
            label: "Resume",
            detail: jump.reason.clone(),
        }
    }

    pub fn current_data_space(&self) -> &dataspace::DataSpaceEntry {
        self.data_spaces
            .get(self.selected_data_space)
            .unwrap_or_else(|| &self.data_spaces[0])
    }

    pub fn poll_background(&mut self) -> bool {
        let mut changed = self.prune_handoff_trail();

        if self.start_deferred_session_preview_if_due() {
            changed = true;
        }

        if let Some(pending) = self.pending_session_load.take() {
            match pending.receiver.try_recv() {
                Ok(result) => {
                    self.apply_session_load_result(pending, result);
                    changed = true;
                }
                Err(TryRecvError::Empty) => {
                    self.pending_session_load = Some(pending);
                }
                Err(TryRecvError::Disconnected) => {
                    self.pending_share_handoff_copy = false;
                    self.compile_status = "FAILED";
                    self.set_status(format!("Session load failed: {}", pending.session_id));
                    changed = true;
                }
            }
        }

        if let Some(pending) = self.pending_session_preview.take() {
            match pending.receiver.try_recv() {
                Ok(result) => {
                    self.apply_session_preview_result(pending, result);
                    changed = true;
                }
                Err(TryRecvError::Empty) => {
                    self.pending_session_preview = Some(pending);
                }
                Err(TryRecvError::Disconnected) => {
                    self.set_status(format!("Timeline preview failed: {}", pending.session_id));
                    changed = true;
                }
            }
        }

        if self.poll_data_space_background() {
            changed = true;
        }
        if self.poll_launch_review_background() {
            changed = true;
        }
        if self.poll_hook_live_background() {
            changed = true;
        }
        changed
    }

    fn poll_hook_live_background(&mut self) -> bool {
        self.hook_live
            .as_mut()
            .is_some_and(hooks::HookLiveState::poll)
    }

    fn prune_handoff_trail(&mut self) -> bool {
        if self.handoff_trail_frame().is_some() {
            return false;
        }
        self.handoff_trail.take().is_some()
    }

    pub fn handoff_trail_frame(&self) -> Option<HandoffTrailFrame> {
        let trail = self.handoff_trail?;
        let elapsed = trail.started_at.elapsed();
        let duration = Duration::from_millis(HANDOFF_TRAIL_DURATION_MS);
        let elapsed_ms = elapsed.as_millis();
        if elapsed >= duration {
            return None;
        }
        let step = ((elapsed_ms * HANDOFF_TRAIL_FRAME_COUNT as u128)
            / u128::from(HANDOFF_TRAIL_DURATION_MS))
        .min((HANDOFF_TRAIL_FRAME_COUNT - 1) as u128) as usize;
        Some(HandoffTrailFrame {
            phase: trail.phase,
            step,
            elapsed_ms: elapsed_ms as u64,
            duration_ms: HANDOFF_TRAIL_DURATION_MS,
        })
    }

    pub(crate) fn start_handoff_trail_for_review(&mut self) {
        self.start_handoff_trail(HandoffTrailPhase::Review);
    }

    fn start_handoff_trail(&mut self, phase: HandoffTrailPhase) {
        self.handoff_trail = Some(HandoffTrail {
            phase,
            started_at: Instant::now(),
        });
    }

    fn clear_handoff_trail(&mut self) {
        self.handoff_trail = None;
    }

    #[cfg(test)]
    fn set_handoff_trail_elapsed_for_test(&mut self, elapsed: Duration) {
        self.handoff_trail = Some(HandoffTrail {
            phase: HandoffTrailPhase::Review,
            started_at: Instant::now() - elapsed,
        });
    }

    fn poll_data_space_background(&mut self) -> bool {
        let Some(pending) = self.pending_data_space_load.take() else {
            return false;
        };

        match pending.receiver.try_recv() {
            Ok(result) => {
                self.apply_data_space_load_result(pending, result);
                true
            }
            Err(TryRecvError::Empty) => {
                self.pending_data_space_load = Some(pending);
                false
            }
            Err(TryRecvError::Disconnected) => {
                self.compile_status = "FAILED";
                self.set_status("Data space load failed: worker disconnected");
                true
            }
        }
    }

    fn poll_launch_review_background(&mut self) -> bool {
        let Some(mut pending) = self.pending_launch_review.take() else {
            return false;
        };

        let mut changed = false;
        loop {
            match pending.receiver.try_recv() {
                Ok(LaunchReviewMessage::Progress(progress)) => {
                    pending.stage = progress.stage;
                    pending.stage_detail = progress.detail;
                    self.set_status(format!(
                        "Handoff job {}: {}",
                        pending.stage.label(),
                        pending.stage_detail
                    ));
                    changed = true;
                }
                Ok(LaunchReviewMessage::Finished(result)) => {
                    self.apply_launch_review_result(pending, *result);
                    return true;
                }
                Err(TryRecvError::Empty) => {
                    self.pending_launch_review = Some(pending);
                    return changed;
                }
                Err(TryRecvError::Disconnected) => {
                    self.compile_status = "FAILED";
                    self.verify_passed = false;
                    self.pending_target = pending.target;
                    self.show_launch = self.show_launch || self.launch_review;
                    self.launch_review = false;
                    self.launch_review_details = false;
                    self.target_launch_result = None;
                    self.launch_review_error = Some(LaunchReviewErrorState {
                        target: pending.target,
                        compiler_id: pending.compiler_id,
                        message: format!(
                            "worker disconnected while {}: {}",
                            pending.stage.label(),
                            pending.stage_detail
                        ),
                        elapsed_ms: pending.started_at.elapsed().as_millis(),
                    });
                    self.clear_handoff_trail();
                    self.set_status("Handoff review failed: worker disconnected");
                    return true;
                }
            }
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        if self.command_mode {
            self.handle_command_key(key);
            return;
        }

        if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('c')) {
            self.should_quit = true;
            return;
        }

        if self.has_overlay() {
            self.handle_overlay_key(key);
            return;
        }
        if self.show_launch {
            self.handle_launch_key(key);
            return;
        }

        match key.code {
            KeyCode::Esc => self.cancel_main_escape(),
            KeyCode::Char('q') => self.back_or_quit(),
            KeyCode::Char('?') => self.open_help(),
            KeyCode::Char('[') => self.cycle_session_filter(false),
            KeyCode::Char(']') => self.cycle_session_filter(true),
            KeyCode::Char('{') => self.cycle_data_space(false),
            KeyCode::Char('}') => self.cycle_data_space(true),
            KeyCode::Char('d') => self.open_data_space_picker(),
            KeyCode::Char('f') => self.cycle_session_filter(true),
            KeyCode::Char('a') => self.toggle_archived_session(),
            KeyCode::Char(',') => self.open_settings(),
            KeyCode::Char('s') | KeyCode::Char('*') => self.toggle_starred_session(),
            KeyCode::Char('o') => self.open_action_menu(),
            KeyCode::Char('y') => self.open_share_panel(),
            KeyCode::Char('x') | KeyCode::Char('t') | KeyCode::Char('H') => {
                self.open_launch_picker()
            }
            KeyCode::Char('D') => self.open_doctor(),
            KeyCode::Char(':') => self.open_command_palette(),
            KeyCode::Tab => self.next_focus(),
            KeyCode::BackTab => self.prev_focus(),
            KeyCode::Char('j') | KeyCode::Down => self.move_down(),
            KeyCode::Char('k') | KeyCode::Up => self.move_up(),
            KeyCode::Char('G') => self.move_bottom(),
            KeyCode::Char('g') => self.handle_g(),
            KeyCode::Char('h') | KeyCode::Left => self.prev_focus(),
            KeyCode::Char('l') | KeyCode::Right => self.next_focus(),
            KeyCode::Char('/') => {
                self.command_mode = true;
                self.command_input = format!("/{}", self.search_query);
                self.command_selection = 0;
                self.command_selection_active = false;
            }
            KeyCode::Char(' ') => self.set_rewind_point(),
            KeyCode::Char('c') => self.review_capsule(),
            KeyCode::Char('e') if self.focus == Focus::Timeline => self.open_timeline_detail(),
            KeyCode::Char('v') => self.toggle_verify(),
            KeyCode::Char('S') => self.open_skill_picker(),
            KeyCode::Char('+') | KeyCode::Char('=') => self.zoom_current_panel(),
            KeyCode::Char('-') => self.restore_zoom(),
            KeyCode::Enter => self.handle_main_enter(),
            _ => self.pending_g = false,
        }
    }

    fn handle_main_enter(&mut self) {
        if !self.current_data_space().is_local() {
            self.open_launch_picker_for_remote_session();
            return;
        }
        let Some(session) = self.current_session().cloned() else {
            self.set_status("No session selected");
            return;
        };
        let preview = self.enter_route_preview(&session);
        if preview.kind != EnterRouteKind::Jump {
            self.queue_original_resume();
            return;
        }
        self.queue_tmux_jump_or_resume();
    }

    fn queue_tmux_jump_or_resume(&mut self) {
        let Some(session) = self.current_session().cloned() else {
            self.set_status("No session selected");
            return;
        };
        let Some(live) = self.hook_live_for_session(&session) else {
            self.queue_original_resume();
            return;
        };
        match tmux::target_from_hook(live.tmux.as_deref(), live.tmux_pane.as_deref()) {
            Ok(target) => {
                let command = target.command();
                let pane_id = command.pane_id.clone();
                self.pending_tmux_jump = Some(Box::new(TmuxJumpPlan {
                    source_session: session.clone(),
                    command,
                }));
                self.set_status(format!(
                    "Jumping to pane {pane_id}: {} {}",
                    session.cli, session.id
                ));
            }
            Err(reason) => {
                self.queue_original_resume_with_status(format!("{reason}; falling back to resume"));
            }
        }
        self.pending_g = false;
    }

    fn handle_command_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                let was_search = self.is_search_command();
                self.command_mode = false;
                self.command_input.clear();
                self.command_selection = 0;
                self.command_selection_active = false;
                if was_search {
                    self.set_search_status();
                } else {
                    self.set_status("Command palette closed");
                }
            }
            KeyCode::Enter => {
                if self.is_search_command() {
                    self.sync_live_search();
                    self.command_mode = false;
                    self.command_input.clear();
                    self.command_selection = 0;
                    self.command_selection_active = false;
                    self.set_search_status();
                    return;
                }

                let command = self.command_input.trim().to_ascii_lowercase();
                let selected = self
                    .selected_command_palette_entry()
                    .map(|entry| entry.command);
                self.command_mode = false;
                self.command_input.clear();
                self.command_selection = 0;
                let selection_active = self.command_selection_active;
                self.command_selection_active = false;
                if command.is_empty() && !selection_active {
                    self.set_status("Command cancelled");
                } else if let Some(entry) = resolve_command_palette_entry(&command) {
                    self.run_palette_command(entry.command);
                } else if let Some(command) = selected {
                    self.run_palette_command(command);
                } else {
                    self.set_status(format!("Unknown command: {command}"));
                }
            }
            KeyCode::Tab | KeyCode::BackTab if !self.is_search_command() => {
                if let Some(entry) = self.selected_command_palette_entry() {
                    self.command_input = entry.command.into();
                    self.command_selection = 0;
                    self.command_selection_active = false;
                    self.set_status(format!("Completed command: {}", entry.command));
                }
            }
            KeyCode::Char('j') if !self.is_search_command() && self.command_input.is_empty() => {
                self.move_command_selection(true)
            }
            KeyCode::Char('k') if !self.is_search_command() && self.command_input.is_empty() => {
                self.move_command_selection(false)
            }
            KeyCode::Down if !self.is_search_command() => self.move_command_selection(true),
            KeyCode::Up if !self.is_search_command() => self.move_command_selection(false),
            KeyCode::Backspace => {
                if self.is_search_command() {
                    if self.command_input.len() > 1 {
                        self.command_input.pop();
                    }
                    self.sync_live_search();
                } else {
                    self.command_input.pop();
                    self.command_selection = 0;
                    self.command_selection_active = false;
                }
            }
            KeyCode::Char(ch) => {
                self.command_input.push(ch);
                if self.is_search_command() {
                    self.sync_live_search();
                } else {
                    self.command_selection = 0;
                    self.command_selection_active = false;
                }
            }
            _ => {}
        }
    }

    fn is_search_command(&self) -> bool {
        self.command_input.starts_with('/')
    }

    fn sync_live_search(&mut self) {
        if let Some(query) = self.command_input.strip_prefix('/') {
            self.search_query = query.trim().to_string();
            self.refresh_visible_sessions();
            self.clamp_selected_session();
            self.defer_selected_session_context();
        }
    }

    fn set_search_status(&mut self) {
        let suffix = if self.is_session_load_pending() {
            " - loading selected session"
        } else if self.is_session_preview_pending() {
            " - loading preview"
        } else {
            ""
        };
        if self.search_query.is_empty() {
            self.set_status(format!("Search cleared{suffix}"));
        } else {
            self.set_status(format!("Search: /{}{suffix}", self.search_query));
        }
    }

    fn open_command_palette(&mut self) {
        self.command_mode = true;
        self.command_input.clear();
        self.command_selection = 0;
        self.command_selection_active = false;
        self.set_status("Command palette opened");
        self.pending_g = false;
    }

    pub fn command_palette_matches(&self) -> Vec<&'static CommandPaletteEntry> {
        command_palette_matches(&self.command_input)
    }

    pub fn selected_command_palette_entry(&self) -> Option<&'static CommandPaletteEntry> {
        let matches = self.command_palette_matches();
        if matches.is_empty() {
            return None;
        }
        Some(matches[self.command_selection.min(matches.len() - 1)])
    }

    fn move_command_selection(&mut self, forward: bool) {
        let count = self.command_palette_matches().len();
        if count == 0 {
            self.command_selection = 0;
            self.command_selection_active = false;
            self.set_status("No matching commands");
            return;
        }
        if forward {
            self.command_selection = (self.command_selection + 1) % count;
        } else if self.command_selection == 0 {
            self.command_selection = count - 1;
        } else {
            self.command_selection -= 1;
        }
        self.command_selection_active = true;
        if let Some(entry) = self.selected_command_palette_entry() {
            self.set_status(format!("Command candidate: {}", entry.command));
        }
    }

    fn run_palette_command(&mut self, command: &str) {
        match command {
            "quit" => self.should_quit = true,
            "open" => self.open_original(),
            "capsule" => self.review_capsule(),
            "capsules" => self.open_capsules(),
            "verify" => self.mark_verify_passed(),
            "help" => self.open_help(),
            "doctor" => self.open_doctor(),
            "hooks" => self.open_doctor(),
            "settings" => self.open_settings(),
            "source next" => self.cycle_session_filter(true),
            "source prev" => self.cycle_session_filter(false),
            "starred" => self.apply_session_filter(SessionFilter::Starred),
            "archived" => self.apply_session_filter(SessionFilter::Archived),
            "archive" => self.toggle_archived_session(),
            "clear" => self.clear_session_filters(),
            "source codex" => self.apply_session_filter(SessionFilter::Tool(CliTool::Codex)),
            "source claude" => self.apply_session_filter(SessionFilter::Tool(CliTool::Claude)),
            "source hermes" => self.apply_session_filter(SessionFilter::Tool(CliTool::Hermes)),
            "data" => self.open_data_space_picker(),
            "data next" => self.cycle_data_space(true),
            "data prev" => self.cycle_data_space(false),
            "skill" => self.open_skill_picker(),
            "handoff" => self.open_launch_picker(),
            _ => self.set_status(format!("Unknown command: {command}")),
        }
    }

    fn handle_launch_key(&mut self, key: KeyEvent) {
        if self.is_launch_review_pending() {
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => {
                    self.show_launch = false;
                    self.launch_review = false;
                    self.modal_scroll = 0;
                    self.set_status("Handoff job continues in background");
                }
                KeyCode::Enter => {
                    if let Some(status) = self.launch_review_job_status() {
                        self.set_status(format!(
                            "Handoff job {}: {}",
                            status.stage_label, status.detail
                        ));
                    } else {
                        self.set_status(format!(
                            "Preparing handoff review: {}",
                            self.pending_target
                        ));
                    }
                }
                _ => {}
            }
            self.pending_g = false;
            return;
        }

        if self.target_launch_result.is_some() {
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => {
                    self.show_launch = false;
                    self.target_launch_result = None;
                    self.modal_scroll = 0;
                    self.set_status("Launch result closed");
                }
                KeyCode::Char('r') => self.rerun_target_handoff(),
                KeyCode::Char('y') => self.copy_target_launch_result_command(),
                KeyCode::Enter => self.set_status("Launch finished - press r, y, or Esc"),
                _ => {}
            }
            return;
        }

        if self.launch_review_error.is_some() {
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => {
                    self.show_launch = false;
                    self.launch_review_error = None;
                    self.modal_scroll = 0;
                    self.set_status("Handoff review error closed");
                }
                KeyCode::Enter => {
                    if let Some(plan) = self.launch_review_error_setup_install_plan() {
                        self.queue_setup_install(plan);
                    } else {
                        self.set_status(
                            "Handoff review failed; press r to retry or S to choose skill",
                        );
                    }
                }
                KeyCode::Char('r') => self.confirm_launch_target(),
                KeyCode::Char('S') => {
                    self.open_skill_picker();
                    self.show_launch = true;
                    self.set_status("Choose handoff skill");
                }
                KeyCode::Char('y') => {
                    if let Some(plan) = self.launch_review_error_setup_install_plan() {
                        self.copy_setup_install_command(plan);
                    } else {
                        self.set_status(
                            "Handoff review failed; press r to retry or S to choose skill",
                        );
                    }
                }
                KeyCode::Char('G') => {
                    self.modal_scroll = u16::MAX;
                    self.pending_g = false;
                    self.set_status("Review bottom");
                }
                KeyCode::Char('g') => self.handle_modal_g(),
                KeyCode::PageDown => self.scroll_modal(true, 6),
                KeyCode::PageUp => self.scroll_modal(false, 6),
                KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.scroll_modal(true, 6)
                }
                KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.scroll_modal(false, 6)
                }
                KeyCode::Char('j') | KeyCode::Down => {
                    self.scroll_modal(true, 1);
                    self.pending_g = false;
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    self.scroll_modal(false, 1);
                    self.pending_g = false;
                }
                _ => self.pending_g = false,
            }
            return;
        }

        if self.launch_review {
            let validation = self.validate_launch_for_target(self.pending_target);
            let needs_handoff_skill = self.launch_requires_handoff_skill(self.pending_target);
            let can_regenerate_handoff = launch_validation_can_regenerate_handoff(&validation);
            let skill_handoff_ready = self.skill_handoff_review_ready();
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => {
                    if self.launch_review_details {
                        self.launch_review_details = false;
                        self.modal_scroll = 0;
                        self.pending_g = false;
                        self.set_status("Handoff details closed");
                        return;
                    }
                    self.show_launch = false;
                    self.launch_review = false;
                    self.launch_review_details = false;
                    self.launch_review_lark_export = false;
                    self.modal_scroll = 0;
                    self.clear_handoff_trail();
                    self.set_status("Launch review closed");
                }
                KeyCode::Char('y') => {
                    if needs_handoff_skill {
                        self.set_status("Choose an AI handoff skill before copying");
                    } else if can_regenerate_handoff {
                        self.set_status("Regenerate handoff with Enter before copying");
                    } else if skill_handoff_ready {
                        self.copy_handoff_artifact();
                    } else {
                        self.copy_launch_command();
                    }
                }
                KeyCode::Char('p') => {
                    if skill_handoff_ready {
                        self.copy_handoff_artifact_path();
                    } else {
                        self.set_status("No handoff file path to copy");
                    }
                }
                KeyCode::Char('d') if skill_handoff_ready && key.modifiers.is_empty() => {
                    self.launch_review_details = !self.launch_review_details;
                    self.modal_scroll = 0;
                    self.pending_g = false;
                    if self.launch_review_details {
                        self.set_status("Handoff details opened");
                    } else {
                        self.set_status("Handoff body opened");
                    }
                }
                KeyCode::Char('S') => {
                    self.open_skill_picker();
                    self.show_launch = true;
                    self.launch_review_details = false;
                    self.set_status("Choose handoff skill");
                }
                KeyCode::Char('r') => {
                    if needs_handoff_skill {
                        self.set_status("Choose an AI handoff skill before running");
                    } else if can_regenerate_handoff {
                        self.set_status("Regenerate handoff with Enter before running");
                    } else {
                        self.launch_review_details = false;
                        self.queue_target_handoff();
                    }
                }
                KeyCode::Enter => {
                    if needs_handoff_skill {
                        self.open_skill_picker();
                        self.set_status("Choose an AI handoff skill before Handoff Review");
                    } else if self.launch_review_lark_export && skill_handoff_ready {
                        self.launch_review_details = false;
                        self.queue_lark_export_from_review();
                    } else if can_regenerate_handoff {
                        self.launch_review_details = false;
                        self.confirm_launch_target();
                    } else if self.launch_review_lark_export {
                        self.launch_review_details = false;
                        self.queue_lark_export_from_review();
                    } else {
                        self.launch_review_details = false;
                        self.queue_target_handoff();
                    }
                }
                KeyCode::Char('G') => {
                    self.modal_scroll = u16::MAX;
                    self.pending_g = false;
                    self.set_status("Review bottom");
                }
                KeyCode::Char('g') => self.handle_modal_g(),
                KeyCode::PageDown => self.scroll_modal(true, 6),
                KeyCode::PageUp => self.scroll_modal(false, 6),
                KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.scroll_modal(true, 6)
                }
                KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.scroll_modal(false, 6)
                }
                KeyCode::Char('j') | KeyCode::Down => {
                    self.scroll_modal(true, 1);
                    self.pending_g = false;
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    self.scroll_modal(false, 1);
                    self.pending_g = false;
                }
                _ => self.pending_g = false,
            }
            return;
        }

        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.show_launch = false;
                self.launch_review = false;
                self.launch_review_error = None;
                self.launch_review_details = false;
                self.modal_scroll = 0;
                self.clear_handoff_trail();
                self.set_status("Launch cancelled");
            }
            KeyCode::Char('S') => self.open_skill_picker(),
            KeyCode::Char('R') => self.cycle_handoff_runner(),
            KeyCode::Enter => self.confirm_launch_target(),
            KeyCode::Char('y') => {
                if let Some(plan) = self.selected_compiler_setup_install_plan() {
                    self.copy_setup_install_command(plan);
                } else {
                    self.copy_launch_command();
                }
            }
            KeyCode::PageDown => self.scroll_modal(true, 6),
            KeyCode::PageUp => self.scroll_modal(false, 6),
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.scroll_modal(true, 6)
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.scroll_modal(false, 6)
            }
            KeyCode::Char('j')
            | KeyCode::Char('l')
            | KeyCode::Down
            | KeyCode::Right
            | KeyCode::Char('}') => self.cycle_target(true),
            KeyCode::Char('k')
            | KeyCode::Char('h')
            | KeyCode::Up
            | KeyCode::Left
            | KeyCode::Char('{') => self.cycle_target(false),
            _ => {}
        }
    }

    fn handle_overlay_key(&mut self, key: KeyEvent) {
        if self.show_share_panel {
            self.handle_share_panel_key(key);
            return;
        }
        if self.show_lark_export {
            self.handle_lark_export_key(key);
            return;
        }
        if self.show_action_menu {
            self.handle_action_menu_key(key);
            return;
        }
        if self.show_settings {
            self.handle_settings_key(key);
            return;
        }
        if self.show_skill_picker {
            self.handle_skill_picker_key(key);
            return;
        }
        if self.show_data_space_config {
            self.handle_data_space_config_key(key);
            return;
        }
        if self.show_data_spaces {
            self.handle_data_space_picker_key(key);
            return;
        }
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => self.back_or_quit(),
            KeyCode::Char('r') if self.show_doctor => self.refresh_doctor(),
            KeyCode::Char('r') if self.show_capsules => self.refresh_capsules(),
            KeyCode::Char('v') if self.show_doctor => self.toggle_verify(),
            KeyCode::Char('y') if self.show_doctor => self.copy_doctor_report(),
            KeyCode::Char('y') if self.show_open_original => self.copy_focused_command(),
            KeyCode::Char('y') => self.open_share_panel(),
            KeyCode::Enter if self.show_open_original => self.queue_original_resume(),
            KeyCode::Char('j') | KeyCode::Down => self.scroll_modal(true, 1),
            KeyCode::Char('k') | KeyCode::Up => self.scroll_modal(false, 1),
            KeyCode::PageDown => self.scroll_modal(true, 6),
            KeyCode::PageUp => self.scroll_modal(false, 6),
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.scroll_modal(true, 6)
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.scroll_modal(false, 6)
            }
            _ => {}
        }
    }

    fn back_or_quit(&mut self) {
        if self.show_skill_picker {
            self.show_skill_picker = false;
            self.modal_scroll = 0;
            self.pending_compiler = self.selected_compiler;
            self.set_status("Skill picker closed");
        } else if self.show_share_panel {
            self.show_share_panel = false;
            self.share_panel_selection = 0;
            self.modal_scroll = 0;
            self.set_status("Yank closed");
        } else if self.show_lark_export {
            self.show_lark_export = false;
            self.lark_export_plan = None;
            self.modal_scroll = 0;
            self.set_status("Lark export closed");
        } else if self.show_doctor {
            self.show_doctor = false;
            self.modal_scroll = 0;
            self.set_status("Pre-flight closed");
        } else if self.show_capsules {
            self.show_capsules = false;
            self.modal_scroll = 0;
            self.set_status("Capsule inventory closed");
        } else if self.show_data_space_config {
            self.show_data_space_config = false;
            self.data_space_config_form = DataSpaceConfigForm::default();
            self.data_space_config_field = 0;
            self.set_status("SSH data space config cancelled");
        } else if self.show_data_spaces {
            self.show_data_spaces = false;
            self.modal_scroll = 0;
            self.data_space_selection = self.selected_data_space;
            self.data_space_delete_confirmation = None;
            self.set_status("Data spaces closed");
        } else if self.show_timeline_detail {
            self.show_timeline_detail = false;
            self.timeline_image_previews.clear();
            self.modal_scroll = 0;
            self.set_status("Timeline detail closed");
        } else if self.show_settings {
            self.show_settings = false;
            self.settings_language = self.ui_preferences.language;
            self.settings_theme = self.ui_preferences.theme;
            self.settings_smart_enter_tmux = self.hooks_config.smart_enter_tmux;
            self.settings_field = SettingsField::Language;
            self.modal_scroll = 0;
            self.set_status("Settings closed");
        } else if self.show_action_menu {
            self.show_action_menu = false;
            self.action_menu_selection = 0;
            self.modal_scroll = 0;
            self.set_status("Action menu closed");
        } else if self.show_open_original {
            self.show_open_original = false;
            self.modal_scroll = 0;
            self.set_status("Original preview closed");
        } else if self.show_launch {
            self.show_launch = false;
            self.launch_review = false;
            self.modal_scroll = 0;
            self.clear_handoff_trail();
            self.set_status("Launch cancelled");
        } else if self.show_help {
            self.show_help = false;
            self.modal_scroll = 0;
            self.set_status("Help closed");
        } else {
            self.should_quit = true;
        }
    }

    fn cancel_main_escape(&mut self) {
        self.pending_g = false;
        self.set_status("Press q or Ctrl-C to quit");
    }

    fn next_focus(&mut self) {
        self.focus = match self.focus {
            Focus::Sessions => Focus::Timeline,
            Focus::Timeline => Focus::Capsule,
            Focus::Capsule => Focus::Branches,
            Focus::Branches => Focus::Sessions,
        };
        if self.zoomed_focus.is_some() {
            self.zoomed_focus = Some(self.focus);
        }
        self.pending_g = false;
    }

    fn open_settings(&mut self) {
        self.settings_language = self.ui_preferences.language;
        self.settings_theme = self.ui_preferences.theme;
        self.settings_smart_enter_tmux = self.hooks_config.smart_enter_tmux;
        self.refresh_lark_cli_readiness();
        self.settings_field = SettingsField::Language;
        self.show_settings = true;
        self.modal_scroll = 0;
        self.set_status("Settings opened");
        self.pending_g = false;
    }

    fn handle_settings_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => self.back_or_quit(),
            KeyCode::Tab | KeyCode::Char('j') | KeyCode::Down => self.cycle_settings_field(true),
            KeyCode::BackTab | KeyCode::Char('k') | KeyCode::Up => self.cycle_settings_field(false),
            KeyCode::Char(' ') | KeyCode::Char('t') | KeyCode::Char('l') | KeyCode::Right => {
                self.cycle_focused_setting(true)
            }
            KeyCode::Char('h') | KeyCode::Left => self.cycle_focused_setting(false),
            KeyCode::Char('r') => self.reset_settings_draft(),
            KeyCode::Enter => {
                if self.settings_field == SettingsField::LarkCli {
                    self.queue_lark_cli_setup_install();
                } else {
                    self.save_settings();
                }
            }
            KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.save_settings()
            }
            _ => {}
        }
    }

    fn cycle_settings_field(&mut self, forward: bool) {
        let current = self.settings_field.index();
        let next = if forward {
            current + 1
        } else {
            current + SettingsField::ALL.len() - 1
        };
        self.settings_field = SettingsField::from_index(next);
        self.pending_g = false;
    }

    fn cycle_focused_setting(&mut self, forward: bool) {
        match self.settings_field {
            SettingsField::Language => {
                self.settings_language = if forward {
                    self.settings_language.next()
                } else {
                    self.settings_language.previous()
                };
                self.set_status(format!(
                    "Language draft: {}",
                    self.settings_language.label()
                ));
            }
            SettingsField::Theme => {
                self.settings_theme = if forward {
                    self.settings_theme.next()
                } else {
                    self.settings_theme.previous()
                };
                self.set_status(format!("Theme preview: {}", self.settings_theme.label()));
            }
            SettingsField::SmartEnter => {
                self.settings_smart_enter_tmux = !self.settings_smart_enter_tmux;
                let state = if self.settings_smart_enter_tmux {
                    "On"
                } else {
                    "Off"
                };
                self.set_status(format!("Smart Enter draft: {state}"));
            }
            SettingsField::LarkCli => {
                self.refresh_lark_cli_readiness();
                self.set_status(format!("Lark CLI: {}", self.lark_cli_readiness.reason));
            }
        }
        self.pending_g = false;
    }

    fn reset_settings_draft(&mut self) {
        self.settings_language = config::UiLanguage::default();
        self.settings_theme = config::UiThemeName::default();
        self.settings_smart_enter_tmux = false;
        self.refresh_lark_cli_readiness();
        self.set_status("Settings draft reset to defaults");
        self.pending_g = false;
    }

    fn refresh_lark_cli_readiness(&mut self) {
        self.lark_cli_readiness = lark::readiness(Some(
            setup::setup_command_display_for_current_exe(setup::SetupInstallTarget::LarkCli),
        ));
    }

    fn queue_lark_cli_setup_install(&mut self) {
        if self.lark_cli_readiness.state == lark::LarkCliState::Ready {
            self.set_status("Lark CLI is ready");
            self.pending_g = false;
            return;
        }
        self.queue_setup_install(SetupInstallPlan {
            target: setup::SetupInstallTarget::LarkCli,
            label: "Install lark-cli".into(),
            command_display: setup::setup_command_display_for_current_exe(
                setup::SetupInstallTarget::LarkCli,
            ),
            compiler_id: None,
        });
    }

    fn save_settings(&mut self) {
        let ui = config::UiPreferencesConfig {
            language: self.settings_language,
            theme: self.settings_theme,
        };
        match config::save_ui_preferences_and_smart_enter(ui, self.settings_smart_enter_tmux) {
            Ok((ui_preferences, hooks_config)) => {
                self.ui_preferences = ui_preferences;
                self.hooks_config = hooks_config;
                self.settings_language = self.ui_preferences.language;
                self.settings_theme = self.ui_preferences.theme;
                self.settings_smart_enter_tmux = self.hooks_config.smart_enter_tmux;
                self.show_settings = false;
                self.settings_field = SettingsField::Language;
                self.modal_scroll = 0;
                self.set_status(format!(
                    "Settings saved: language {}, theme {}, Smart Enter {}",
                    self.ui_preferences.language.label(),
                    self.ui_preferences.theme.label(),
                    if self.hooks_config.smart_enter_tmux {
                        "On"
                    } else {
                        "Off"
                    }
                ));
            }
            Err(error) => {
                self.set_status(format!("Settings save failed: {error}"));
            }
        }
        self.pending_g = false;
    }

    fn prev_focus(&mut self) {
        self.focus = match self.focus {
            Focus::Sessions => Focus::Branches,
            Focus::Timeline => Focus::Sessions,
            Focus::Capsule => Focus::Timeline,
            Focus::Branches => Focus::Capsule,
        };
        if self.zoomed_focus.is_some() {
            self.zoomed_focus = Some(self.focus);
        }
        self.pending_g = false;
    }

    fn zoom_current_panel(&mut self) {
        self.zoomed_focus = Some(self.focus);
        self.set_status(format!("Zoomed {}", self.focus.label()));
        self.pending_g = false;
    }

    fn restore_zoom(&mut self) {
        if self.zoomed_focus.take().is_some() {
            self.set_status("Zoom restored");
        } else {
            self.set_status("No panel zoom active");
        }
        self.pending_g = false;
    }

    fn move_down(&mut self) {
        match self.focus {
            Focus::Sessions => self.move_session(true),
            Focus::Timeline => {
                self.selected_event = next_visible_timeline_event(
                    &self.data,
                    &self.rewind_event_id,
                    self.selected_event,
                );
            }
            Focus::Capsule => self.scroll_capsule(true, 1),
            Focus::Branches => {}
        }
        self.pending_g = false;
    }

    fn move_up(&mut self) {
        match self.focus {
            Focus::Sessions => self.move_session(false),
            Focus::Timeline => {
                self.selected_event = previous_visible_timeline_event(
                    &self.data,
                    &self.rewind_event_id,
                    self.selected_event,
                );
            }
            Focus::Capsule => self.scroll_capsule(false, 1),
            Focus::Branches => {}
        }
        self.pending_g = false;
    }

    fn move_top(&mut self) {
        match self.focus {
            Focus::Sessions => {
                if let Some(first) = self.visible_session_indices.first().copied() {
                    self.select_session_index(first);
                }
            }
            Focus::Timeline => {
                self.selected_event =
                    first_visible_timeline_event(&self.data, &self.rewind_event_id)
            }
            Focus::Capsule => self.capsule_scroll = 0,
            Focus::Branches => {}
        }
        self.pending_g = false;
    }

    fn move_bottom(&mut self) {
        match self.focus {
            Focus::Sessions => {
                if let Some(last) = self.visible_session_indices.last().copied() {
                    self.select_session_index(last);
                }
            }
            Focus::Timeline => {
                self.selected_event = last_visible_timeline_event(&self.data, &self.rewind_event_id)
            }
            Focus::Capsule => self.scroll_capsule(true, 999),
            Focus::Branches => {}
        }
        self.pending_g = false;
    }

    fn handle_g(&mut self) {
        if self.pending_g {
            self.move_top();
        } else {
            self.pending_g = true;
        }
    }

    fn set_rewind_point(&mut self) {
        if !self.ensure_session_details_ready("Rewind") {
            return;
        }
        self.selected_event =
            nearest_visible_timeline_event(&self.data, &self.rewind_event_id, self.selected_event);
        if let Some((id, title)) = self
            .data
            .timeline
            .get(self.selected_event)
            .and_then(|event| {
                timeline_event_is_rewind_anchor(event)
                    .then(|| (event.id.clone(), event.title.clone()))
            })
        {
            self.apply_rewind_event(id.clone(), title);
            self.set_status(format!("Rewind set: {id}"));
        } else {
            self.set_status("Rewind anchor must be a User turn");
        }
        self.pending_g = false;
    }

    fn refresh_compiler_catalog(&mut self) {
        let selected_id = self
            .data
            .compilers
            .get(self.selected_compiler)
            .cloned()
            .unwrap_or_else(|| self.data.capsule.compiler.clone());
        let pending_id = self.data.compilers.get(self.pending_compiler).cloned();
        let catalog = compiler::compiler_catalog_entries();
        if catalog.is_empty() {
            return;
        }
        self.compiler_catalog = catalog;
        let preserved = [selected_id.clone(), pending_id.clone().unwrap_or_default()];
        sync_compiler_ids_with_catalog(&mut self.data, &self.compiler_catalog, preserved);
        self.selected_compiler = compiler_index_for_equivalent_id(
            &self.data,
            &selected_id,
            self.data.target,
            self.selected_compiler,
        );
        self.pending_compiler = pending_id
            .as_deref()
            .map(|id| {
                compiler_index_for_equivalent_id(
                    &self.data,
                    id,
                    self.data.target,
                    self.pending_compiler,
                )
            })
            .unwrap_or(self.selected_compiler);
    }

    pub(crate) fn pending_skill_setup_install_plan(&self) -> Option<SetupInstallPlan> {
        self.setup_install_plan_for_compiler_index(self.pending_compiler, SetupInstallScope::Skill)
    }

    pub(crate) fn selected_compiler_setup_install_plan(&self) -> Option<SetupInstallPlan> {
        self.setup_install_plan_for_compiler_index(
            self.selected_compiler,
            SetupInstallScope::Launch,
        )
    }

    fn setup_install_plan_for_compiler_index(
        &self,
        index: usize,
        scope: SetupInstallScope,
    ) -> Option<SetupInstallPlan> {
        let compiler_id = self.data.compilers.get(index)?;
        let info = self
            .compiler_catalog
            .iter()
            .find(|entry| entry.id == *compiler_id)?;
        setup_install_plan_for_compiler_info(info, scope)
    }

    pub(crate) fn launch_review_error_setup_install_plan(&self) -> Option<SetupInstallPlan> {
        let error = self.launch_review_error.as_ref()?;
        setup_install_plan_for_compiler_error(&error.compiler_id, &error.message)
    }

    fn queue_setup_install(&mut self, plan: SetupInstallPlan) {
        self.pending_setup_install = Some(Box::new(plan.clone()));
        self.set_status(format!(
            "Installing {} outside Moonbox: {}",
            plan.label, plan.command_display
        ));
        self.pending_g = false;
    }

    fn copy_setup_install_command(&mut self, plan: SetupInstallPlan) {
        self.clipboard_text = Some(plan.command_display.clone());
        self.set_status(format!("Copied setup command: {}", plan.label));
        self.pending_g = false;
    }

    fn open_skill_picker(&mut self) {
        self.refresh_compiler_catalog();
        let candidates = self.skill_picker_candidate_indices();
        self.pending_compiler = if candidates.contains(&self.selected_compiler) {
            self.selected_compiler
        } else {
            candidates
                .iter()
                .copied()
                .find(|candidate| {
                    self.compiler_selection_matches(*candidate, self.selected_compiler)
                })
                .or_else(|| candidates.first().copied())
                .unwrap_or(0)
        };
        self.show_skill_picker = true;
        self.modal_scroll = 0;
        self.set_status("Choose handoff skill");
        self.pending_g = false;
    }

    fn handle_skill_picker_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => self.back_or_quit(),
            KeyCode::Enter => self.confirm_skill_picker(),
            KeyCode::Char('j') | KeyCode::Char('l') | KeyCode::Down | KeyCode::Right => {
                self.move_skill_picker(true)
            }
            KeyCode::Char('k') | KeyCode::Char('h') | KeyCode::Up | KeyCode::Left => {
                self.move_skill_picker(false)
            }
            KeyCode::Char('y') => self.copy_pending_skill_reference(),
            _ => {}
        }
    }

    fn move_skill_picker(&mut self, forward: bool) {
        let candidates = self.skill_picker_candidate_indices();
        if candidates.is_empty() {
            self.pending_compiler = 0;
            self.set_status("No compiler skills configured");
            return;
        }
        let position = candidates
            .iter()
            .position(|candidate| *candidate == self.pending_compiler)
            .unwrap_or(0);
        let next_position = if forward {
            (position + 1) % candidates.len()
        } else if position == 0 {
            candidates.len() - 1
        } else {
            position - 1
        };
        self.pending_compiler = candidates[next_position];
        self.set_status(format!(
            "Skill candidate: {}",
            self.skill_picker_candidate_label(self.pending_compiler)
        ));
    }

    fn ensure_launch_handoff_skill_default(&mut self) {
        self.ensure_handoff_skill_default(false);
    }

    fn ensure_handoff_skill_default(&mut self, include_fixture: bool) {
        if !include_fixture
            && !self
                .current_session()
                .is_some_and(|session| session.source_provenance != SourceProvenance::Fixture)
        {
            return;
        }
        let Some(current) = self.data.compilers.get(self.selected_compiler) else {
            return;
        };
        if !compiler::compiler_is_builtin(current) {
            return;
        }
        let Some(candidate) = self.skill_picker_candidate_indices().first().copied() else {
            return;
        };
        self.selected_compiler = candidate;
        if let Some(compiler) = self.data.compilers.get(self.selected_compiler).cloned() {
            self.data.capsule.compiler = compiler;
        }
    }

    fn select_lark_handoff_skill_default(&mut self) {
        let current_ready_agent = self
            .data
            .compilers
            .get(self.selected_compiler)
            .is_some_and(|compiler_id| handoff::parse_compiler_id(compiler_id).is_some())
            && self.selected_compiler_setup_install_plan().is_none();
        if current_ready_agent {
            if let Some(compiler) = self.data.compilers.get(self.selected_compiler).cloned() {
                self.data.capsule.compiler = compiler;
            }
            return;
        }

        let ready_candidate = self
            .skill_picker_candidate_indices()
            .into_iter()
            .find(|index| {
                self.setup_install_plan_for_compiler_index(*index, SetupInstallScope::Launch)
                    .is_none()
                    && self.data.compilers.get(*index).is_some_and(|compiler_id| {
                        self.compiler_catalog.iter().any(|info| {
                            info.id == *compiler_id && info.status == CompilerPresetStatus::Ready
                        })
                    })
            });
        if let Some(candidate) = ready_candidate {
            self.selected_compiler = candidate;
            if let Some(compiler) = self.data.compilers.get(self.selected_compiler).cloned() {
                self.data.capsule.compiler = compiler;
            }
            return;
        }

        self.ensure_handoff_skill_default(true);
    }

    fn cycle_handoff_runner(&mut self) {
        self.ensure_launch_handoff_skill_default();
        let Some(current_id) = self.data.compilers.get(self.selected_compiler) else {
            self.set_status("No handoff runner selected");
            return;
        };
        let Some(current_spec) = handoff::parse_compiler_id(current_id) else {
            self.set_status("Choose a handoff skill before switching runner");
            return;
        };
        let matching = self
            .data
            .compilers
            .iter()
            .enumerate()
            .filter_map(|(index, compiler_id)| {
                handoff::parse_compiler_id(compiler_id)
                    .filter(|spec| spec.skill_id == current_spec.skill_id)
                    .map(|spec| (index, spec))
            })
            .collect::<Vec<_>>();
        if matching.len() <= 1 {
            self.set_status(format!(
                "No alternate runner for {}",
                handoff::skill_display_label(&current_spec.skill_id)
            ));
            return;
        }
        let position = matching
            .iter()
            .position(|(index, _)| *index == self.selected_compiler)
            .unwrap_or(0);
        let (next_index, next_spec) = &matching[(position + 1) % matching.len()];
        self.selected_compiler = *next_index;
        self.pending_compiler = *next_index;
        self.data.capsule.compiler = self.data.compilers[*next_index].clone();
        self.launch_review_error = None;
        self.set_status(format!(
            "Runner: {} / {}",
            next_spec.runner.label(),
            handoff::skill_display_label(&next_spec.skill_id)
        ));
        self.pending_g = false;
    }

    pub(crate) fn skill_picker_candidate_indices(&self) -> Vec<usize> {
        let mut candidates = Vec::new();
        let mut seen_agent_skills: Vec<String> = Vec::new();
        for (index, compiler_id) in self.data.compilers.iter().enumerate() {
            if compiler::compiler_is_builtin(compiler_id) {
                continue;
            }
            if let Some(spec) = handoff::parse_compiler_id(compiler_id) {
                if seen_agent_skills
                    .iter()
                    .any(|skill_id| skill_id == &spec.skill_id)
                {
                    continue;
                }
                seen_agent_skills.push(spec.skill_id.clone());
                candidates.push(
                    self.preferred_agent_compiler_index(&spec.skill_id)
                        .unwrap_or(index),
                );
            } else {
                candidates.push(index);
            }
        }
        candidates
    }

    fn preferred_agent_compiler_index(&self, skill_id: &str) -> Option<usize> {
        let matching_indices = self
            .data
            .compilers
            .iter()
            .enumerate()
            .filter_map(|(index, compiler_id)| {
                handoff::parse_compiler_id(compiler_id)
                    .filter(|spec| spec.skill_id == skill_id)
                    .map(|spec| (index, spec))
            })
            .collect::<Vec<_>>();
        if let Some(target_runner_id) = target_runner_id(self.data.target)
            && let Some((index, _)) = matching_indices
                .iter()
                .find(|(_, spec)| spec.runner.id() == target_runner_id)
        {
            return Some(*index);
        }
        if matching_indices
            .iter()
            .any(|(index, _)| *index == self.selected_compiler)
        {
            return Some(self.selected_compiler);
        }
        matching_indices.first().map(|(index, _)| *index)
    }

    pub(crate) fn compiler_selection_matches(&self, candidate: usize, selected: usize) -> bool {
        let Some(candidate_id) = self.data.compilers.get(candidate) else {
            return false;
        };
        let Some(selected_id) = self.data.compilers.get(selected) else {
            return false;
        };
        match (
            handoff::parse_compiler_id(candidate_id),
            handoff::parse_compiler_id(selected_id),
        ) {
            (Some(candidate), Some(selected)) => candidate.skill_id == selected.skill_id,
            _ => candidate == selected,
        }
    }

    fn skill_picker_candidate_label(&self, index: usize) -> String {
        self.data
            .compilers
            .get(index)
            .and_then(|compiler_id| handoff::parse_compiler_id(compiler_id))
            .map(|spec| handoff::skill_display_label(&spec.skill_id).to_string())
            .or_else(|| self.data.compilers.get(index).cloned())
            .unwrap_or_else(|| "unknown".into())
    }

    fn confirm_skill_picker(&mut self) {
        if self.is_launch_review_pending() {
            self.show_skill_picker = false;
            let _ = self.reveal_pending_launch_review();
            return;
        }
        let candidates = self.skill_picker_candidate_indices();
        if candidates.is_empty() {
            self.show_skill_picker = false;
            self.set_status("No compiler skills configured");
            return;
        }
        if !candidates.contains(&self.pending_compiler) {
            self.pending_compiler = candidates[0];
        }
        if let Some(plan) = self.pending_skill_setup_install_plan() {
            self.queue_setup_install(plan);
            return;
        }
        self.selected_compiler = self.pending_compiler;
        self.data.capsule.compiler = self.data.compilers[self.selected_compiler].clone();
        let selected_label = self.skill_picker_candidate_label(self.selected_compiler);
        let continue_launch = self.show_launch;
        self.show_skill_picker = false;
        self.launch_review_error = None;
        self.modal_scroll = 0;
        if continue_launch {
            self.set_status(format!(
                "Handoff skill: {selected_label}; press Enter to generate Review"
            ));
        } else {
            self.set_status(format!("Handoff skill: {selected_label}"));
        }
        self.pending_g = false;
    }

    fn copy_pending_skill_reference(&mut self) {
        let Some(skill) = self.data.compilers.get(self.pending_compiler) else {
            self.set_status("No compiler skill selected");
            return;
        };
        let info = self
            .compiler_catalog
            .iter()
            .find(|entry| entry.id == *skill);
        let copied = info
            .and_then(compiler::compiler_skill_clipboard_reference)
            .unwrap_or_else(|| self.skill_picker_candidate_label(self.pending_compiler));
        self.clipboard_text = Some(copied);
        self.set_status(format!(
            "Copied skill reference: {}",
            self.skill_picker_candidate_label(self.pending_compiler)
        ));
        self.pending_g = false;
    }

    fn toggle_verify(&mut self) {
        self.run_verification();
        self.pending_g = false;
    }

    fn mark_verify_passed(&mut self) {
        self.run_verification();
        self.pending_g = false;
    }

    fn run_verification(&mut self) {
        if !self.ensure_session_details_ready("Verify") {
            return;
        }
        let session_id = self.current_session().map(|session| session.id.clone());
        let report = match workbench::verify_launch(session_id.as_deref(), self.data.target, None) {
            Ok(Some(report)) => report,
            Ok(None) => {
                self.verify_passed = false;
                self.set_status("Verify: FAIL No session selected");
                return;
            }
            Err(error) => {
                self.verify_passed = false;
                self.set_status(format!("Verify: FAIL {error}"));
                return;
            }
        };
        self.verify_passed = report.ready;
        self.set_status(format!(
            "Verify: {} ({} checks)",
            report.status,
            report.checks.len()
        ));
    }

    fn open_help(&mut self) {
        self.show_help = true;
        self.modal_scroll = 0;
        self.set_status("Help opened");
        self.pending_g = false;
    }

    fn open_data_space_picker(&mut self) {
        self.data_space_selection = self
            .selected_data_space
            .min(self.data_spaces.len().saturating_sub(1));
        self.show_data_spaces = true;
        self.modal_scroll = 0;
        self.data_space_delete_confirmation = None;
        self.set_status("Data spaces opened");
        self.pending_g = false;
    }

    fn handle_data_space_picker_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') if self.data_space_delete_confirmation.is_some() => {
                self.data_space_delete_confirmation = None;
                self.set_status("Data space delete cancelled");
            }
            KeyCode::Esc | KeyCode::Char('q') => self.back_or_quit(),
            KeyCode::Char('n') | KeyCode::Char('a') => self.open_data_space_config(),
            KeyCode::Char('r') => self.refresh_data_spaces(),
            KeyCode::Char('x') | KeyCode::Delete => self.delete_selected_data_space(),
            KeyCode::Enter => self.confirm_data_space_selection(),
            KeyCode::Char('j') | KeyCode::Down | KeyCode::Char('}') => {
                self.move_data_space_selection(true)
            }
            KeyCode::Char('k') | KeyCode::Up | KeyCode::Char('{') => {
                self.move_data_space_selection(false)
            }
            _ => {}
        }
    }

    fn open_data_space_config(&mut self) {
        self.show_data_space_config = true;
        self.data_space_config_form = DataSpaceConfigForm::default();
        self.data_space_config_field = 0;
        self.data_space_error = None;
        self.data_space_delete_confirmation = None;
        self.set_status("Add SSH data space");
        self.pending_g = false;
    }

    fn handle_data_space_config_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.back_or_quit(),
            KeyCode::Tab | KeyCode::Down => self.move_data_space_config_field(true),
            KeyCode::BackTab | KeyCode::Up => self.move_data_space_config_field(false),
            KeyCode::Enter => {
                let quick_save = self.data_space_config_field == 0
                    && !self.data_space_config_form.quick.trim().is_empty();
                let last_field = self.data_space_config_field + 1 >= DATA_SPACE_CONFIG_FIELD_COUNT;
                if quick_save || last_field {
                    self.save_data_space_config();
                } else {
                    self.move_data_space_config_field(true);
                }
            }
            KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.save_data_space_config()
            }
            KeyCode::Backspace => {
                self.data_space_config_form
                    .field_mut(self.data_space_config_field)
                    .pop();
            }
            KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.data_space_config_form
                    .field_mut(self.data_space_config_field)
                    .push(ch);
            }
            _ => {}
        }
    }

    fn move_data_space_config_field(&mut self, forward: bool) {
        self.data_space_config_field = if forward {
            (self.data_space_config_field + 1) % DATA_SPACE_CONFIG_FIELD_COUNT
        } else if self.data_space_config_field == 0 {
            DATA_SPACE_CONFIG_FIELD_COUNT - 1
        } else {
            self.data_space_config_field - 1
        };
    }

    fn save_data_space_config(&mut self) {
        if let Err(error) = self.data_space_config_form.parse_quick_into_fields() {
            self.data_space_error = Some(error.clone());
            self.set_status(format!("SSH target parse failed: {error}"));
            return;
        }
        let host = match self.data_space_config_form.to_config() {
            Ok(host) => host,
            Err(error) => {
                self.data_space_error = Some(error.clone());
                self.set_status(format!("SSH config invalid: {error}"));
                return;
            }
        };
        let host_name = host.name.clone();
        match config::add_ssh_host_config(host) {
            Ok(()) => {
                self.show_data_space_config = false;
                self.data_space_config_form = DataSpaceConfigForm::default();
                self.data_space_config_field = 0;
                self.refresh_data_spaces();
                if let Some(index) = self
                    .data_spaces
                    .iter()
                    .position(|space| space.id == format!("ssh:{host_name}"))
                {
                    self.data_space_selection = index;
                }
                self.data_space_error = None;
                self.data_space_delete_confirmation = None;
                self.set_status(format!("SSH data space saved: {host_name}"));
            }
            Err(error) => {
                self.data_space_error = Some(error.to_string());
                self.set_status(format!("SSH config save failed: {error}"));
            }
        }
    }

    fn move_data_space_selection(&mut self, forward: bool) {
        if self.data_spaces.is_empty() {
            self.data_space_selection = 0;
            self.set_status("No data spaces configured");
            return;
        }
        let len = self.data_spaces.len();
        self.data_space_selection = if forward {
            (self.data_space_selection + 1) % len
        } else if self.data_space_selection == 0 {
            len - 1
        } else {
            self.data_space_selection - 1
        };
        if let Some(space) = self.data_spaces.get(self.data_space_selection) {
            self.set_status(format!("Data candidate: {}", space.label));
        }
        self.data_space_delete_confirmation = None;
    }

    fn delete_selected_data_space(&mut self) {
        let Some(space) = self.data_spaces.get(self.data_space_selection) else {
            self.set_status("No data space selected");
            return;
        };
        if space.is_local() {
            self.data_space_delete_confirmation = None;
            self.set_status("Local data space cannot be deleted");
            return;
        }
        if space.config_source.as_deref() != Some("Moonbox config") {
            self.data_space_delete_confirmation = None;
            self.set_status("Only Moonbox data spaces can be deleted here");
            return;
        }
        let name = space.label.clone();
        if self.data_space_delete_confirmation.as_deref() != Some(name.as_str()) {
            self.data_space_delete_confirmation = Some(name.clone());
            self.set_status(format!("Press x again to delete SSH data space: {name}"));
            return;
        }

        match config::remove_ssh_host_config(&name) {
            Ok(true) => {
                self.data_space_delete_confirmation = None;
                let deleted_active = self
                    .data_spaces
                    .get(self.selected_data_space)
                    .is_some_and(|space| space.label == name);
                self.refresh_data_spaces();
                if deleted_active {
                    self.selected_data_space = 0;
                    self.data_space_selection = 0;
                    self.load_data_space(0);
                }
                self.set_status(format!("SSH data space deleted: {name}"));
            }
            Ok(false) => {
                self.data_space_delete_confirmation = None;
                self.refresh_data_spaces();
                self.set_status(format!("SSH data space was already gone: {name}"));
            }
            Err(error) => {
                self.data_space_error = Some(error.to_string());
                self.set_status(format!("SSH data space delete failed: {error}"));
            }
        }
    }

    fn confirm_data_space_selection(&mut self) {
        self.data_space_delete_confirmation = None;
        if self.data_space_selection == self.selected_data_space {
            if let Some(space) = self.data_spaces.get(self.selected_data_space) {
                self.set_status(format!("Data space already active: {}", space.label));
            }
            self.show_data_spaces = false;
            self.modal_scroll = 0;
            self.pending_g = false;
            return;
        }
        let index = self.data_space_selection;
        self.show_data_spaces = false;
        self.modal_scroll = 0;
        self.load_data_space(index);
    }

    fn refresh_data_spaces(&mut self) {
        let current_id = self.current_data_space().id.clone();
        let selected_id = self
            .data_spaces
            .get(self.data_space_selection)
            .map(|space| space.id.clone());
        self.data_spaces = dataspace::list_data_spaces();
        self.selected_data_space = self
            .data_spaces
            .iter()
            .position(|space| space.id == current_id)
            .unwrap_or(0);
        self.data_space_selection = selected_id
            .and_then(|id| self.data_spaces.iter().position(|space| space.id == id))
            .unwrap_or(self.selected_data_space);
        self.data_space_delete_confirmation = None;
        self.set_status(format!("Data spaces refreshed: {}", self.data_spaces.len()));
        self.pending_g = false;
    }

    fn open_capsules(&mut self) {
        self.show_capsules = true;
        self.modal_scroll = 0;
        self.refresh_capsules();
    }

    fn open_doctor(&mut self) {
        self.refresh_doctor();
        self.show_doctor = true;
        self.modal_scroll = 0;
        self.pending_g = false;
    }

    fn refresh_doctor(&mut self) {
        self.doctor_report = doctor::diagnose();
        self.set_status(format!(
            "Pre-flight: {} ({} doctor checks)",
            self.doctor_report.status,
            self.doctor_report.checks.len()
        ));
    }

    fn refresh_capsules(&mut self) {
        match workbench::list_saved_capsules() {
            Ok(capsules) => {
                let count = capsules.len();
                self.saved_capsules = capsules;
                self.saved_capsule_error = None;
                self.set_status(format!("Capsules: {count} saved"));
            }
            Err(error) => {
                self.saved_capsules.clear();
                self.saved_capsule_error = Some(error.to_string());
                self.set_status(format!("Capsules failed: {error}"));
            }
        }
        self.pending_g = false;
    }

    fn open_action_menu(&mut self) {
        if self.current_session().is_none() {
            self.show_action_menu = false;
            self.set_status("No session selected");
            self.pending_g = false;
            return;
        }
        self.show_action_menu = true;
        self.action_menu_selection = self.preferred_action_menu_selection();
        self.modal_scroll = 0;
        if let Some(entry) = self.selected_action_menu_entry() {
            self.set_status(format!("Choose session action: {}", entry.action.label));
        } else {
            self.set_status("Choose session action");
        }
        self.pending_g = false;
    }

    fn preferred_action_menu_selection(&self) -> usize {
        let entries = self.action_menu_entries();
        [
            SessionAvailableActionKind::LarkExport,
            SessionAvailableActionKind::Handoff,
            SessionAvailableActionKind::Yank,
            SessionAvailableActionKind::Inspect,
            SessionAvailableActionKind::Resume,
        ]
        .into_iter()
        .find_map(|kind| {
            entries.iter().position(|entry| {
                entry.action.kind == kind && action_is_runnable(entry.action.status)
            })
        })
        .unwrap_or(0)
    }

    fn open_share_panel(&mut self) {
        if self.current_session().is_none() {
            self.show_share_panel = false;
            self.set_status("No session selected");
            self.pending_g = false;
            return;
        }
        self.show_action_menu = false;
        self.show_share_panel = true;
        self.share_panel_selection = self
            .share_panel_selection
            .min(SharePanelActionKind::ALL.len().saturating_sub(1));
        self.modal_scroll = 0;
        self.set_status("Choose yank action");
        self.pending_g = false;
    }

    fn handle_action_menu_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => self.back_or_quit(),
            KeyCode::Char('j') | KeyCode::Down => self.move_action_menu_selection(true),
            KeyCode::Char('k') | KeyCode::Up => self.move_action_menu_selection(false),
            KeyCode::Char('y') => self.open_share_panel(),
            KeyCode::Char('r') => self.execute_action_menu_resume_shortcut(),
            KeyCode::Enter => self.execute_action_menu_selection_from_enter(),
            _ => {}
        }
    }

    fn move_action_menu_selection(&mut self, forward: bool) {
        let count = self.action_menu_entries().len();
        if count == 0 {
            self.action_menu_selection = 0;
            self.set_status("No session actions");
            return;
        }
        if forward {
            self.action_menu_selection = (self.action_menu_selection + 1) % count;
        } else if self.action_menu_selection == 0 {
            self.action_menu_selection = count - 1;
        } else {
            self.action_menu_selection -= 1;
        }
        if let Some(entry) = self.selected_action_menu_entry() {
            self.set_status(format!("Action candidate: {}", entry.action.label));
        }
    }

    fn execute_action_menu_selection_from_enter(&mut self) {
        let Some(entry) = self.selected_action_menu_entry() else {
            self.set_status("No session action selected");
            self.pending_g = false;
            return;
        };
        if entry.action.kind == SessionAvailableActionKind::Resume {
            self.set_status("Resume requires explicit r from Action Menu");
            self.pending_g = false;
            return;
        }
        self.execute_action_menu_selection();
    }

    fn execute_action_menu_resume_shortcut(&mut self) {
        let Some(index) = self
            .action_menu_entries()
            .iter()
            .position(|entry| entry.action.kind == SessionAvailableActionKind::Resume)
        else {
            self.set_status("Resume is unavailable");
            self.pending_g = false;
            return;
        };
        self.action_menu_selection = index;
        self.execute_action_menu_selection();
    }

    fn execute_action_menu_selection(&mut self) {
        let Some(entry) = self.selected_action_menu_entry() else {
            self.set_status("No session action selected");
            return;
        };
        if !entry.runnable {
            self.set_status(format!(
                "{} unavailable: {}",
                entry.action.label, entry.action.reason
            ));
            return;
        }
        match entry.action.kind {
            SessionAvailableActionKind::Resume => {
                self.show_action_menu = false;
                self.queue_original_resume();
            }
            SessionAvailableActionKind::Handoff => {
                self.show_action_menu = false;
                if self.current_data_space().is_local() {
                    self.open_launch_picker();
                } else {
                    self.open_launch_picker_for_remote_session();
                }
            }
            SessionAvailableActionKind::LarkExport => {
                self.show_action_menu = false;
                self.start_lark_export_from_action_menu();
            }
            SessionAvailableActionKind::NewSession => {
                self.show_action_menu = false;
                self.queue_seed_prompt_session();
            }
            SessionAvailableActionKind::Jump => {
                self.show_action_menu = false;
                self.queue_tmux_jump_or_resume();
            }
            SessionAvailableActionKind::Fork => {
                self.show_action_menu = false;
                self.queue_native_fork();
            }
            SessionAvailableActionKind::Inspect => {
                self.show_action_menu = false;
                self.focus = Focus::Capsule;
                self.zoomed_focus = Some(Focus::Capsule);
                self.set_status("Inspecting session details");
            }
            SessionAvailableActionKind::Yank => self.open_share_panel(),
            SessionAvailableActionKind::Archive => {
                self.show_action_menu = false;
                self.toggle_archived_session();
            }
        }
    }

    fn handle_lark_export_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => self.back_or_quit(),
            KeyCode::Enter => self.confirm_lark_export_review(),
            KeyCode::Char('y') => self.copy_lark_export_command(),
            KeyCode::Char('j') | KeyCode::Down => self.scroll_modal(true, 1),
            KeyCode::Char('k') | KeyCode::Up => self.scroll_modal(false, 1),
            _ => {}
        }
    }

    fn start_lark_export_from_action_menu(&mut self) {
        let Some(plan) = self.build_lark_export_plan() else {
            return;
        };
        self.lark_cli_readiness = plan.lark_cli.clone();
        self.lark_export_plan = Some(plan);
        self.confirm_lark_export_review();
    }

    fn build_lark_export_plan(&mut self) -> Option<lark::LarkExportPlan> {
        if !self.current_data_space().is_local() {
            self.set_status("Lark export requires local session context");
            self.pending_g = false;
            return None;
        }
        let Some(session) = self.current_session() else {
            self.set_status("No session selected");
            self.pending_g = false;
            return None;
        };
        let compiler = self
            .data
            .compilers
            .get(self.selected_compiler)
            .cloned()
            .unwrap_or_else(|| self.data.capsule.compiler.clone());
        match workbench::compile_request_for_selection(
            Some(&session.id),
            self.data.target,
            Some(&self.rewind_event_id),
            Some(&compiler),
        ) {
            Ok(Some(request)) => {
                let plan = lark::dry_run_plan(
                    &request,
                    &lark::LarkExportOptions::default(),
                    Some(setup::setup_command_display_for_current_exe(
                        setup::SetupInstallTarget::LarkCli,
                    )),
                );
                Some(plan)
            }
            Ok(None) => {
                self.set_status("No session selected");
                None
            }
            Err(error) => {
                self.set_status(format!("Lark export failed: {error}"));
                None
            }
        }
    }

    fn confirm_lark_export_review(&mut self) {
        let Some(plan) = self.lark_export_plan.clone() else {
            self.set_status("No Lark export plan");
            self.pending_g = false;
            return;
        };
        if plan.lark_cli.state != lark::LarkCliState::Ready {
            self.queue_lark_cli_setup_install();
            return;
        }
        self.refresh_compiler_catalog();
        self.select_lark_handoff_skill_default();
        self.launch_review_lark_export = true;
        self.pending_target = self.data.target;
        self.show_lark_export = false;
        self.confirm_launch_target();
    }

    fn queue_lark_export_from_review(&mut self) {
        if self.lark_cli_readiness.state != lark::LarkCliState::Ready {
            self.queue_lark_cli_setup_install();
            return;
        }
        let Some(markdown) = self.data.capsule.handoff_artifact.clone() else {
            self.set_status("Generate handoff before creating Lark Doc");
            self.pending_g = false;
            return;
        };
        let session_id = self.data.capsule.source_session.clone();
        let target = self.data.capsule.target_cli;
        let rewind = self.rewind_event_id.clone();
        let compiler = self.data.capsule.compiler.clone();
        let command_display =
            "lark-cli docs +create --api-version v2 --as user --doc-format markdown --content <reviewed handoff markdown>"
                .to_string();
        self.pending_lark_export = Some(Box::new(LarkExportTuiPlan {
            session_id,
            target,
            rewind,
            compiler,
            command_display,
            title: self.data.capsule.handoff_label.clone(),
            markdown,
        }));
        self.show_launch = false;
        self.launch_review = false;
        self.launch_review_details = false;
        self.launch_review_lark_export = false;
        self.show_lark_export = false;
        self.lark_export_plan = None;
        self.set_status("Opening Lark Doc");
        self.pending_g = false;
    }

    fn copy_lark_export_command(&mut self) {
        let Some(plan) = self.lark_export_plan.as_ref() else {
            self.set_status("No Lark export command");
            return;
        };
        self.clipboard_text = Some(lark_export_command_display(
            &plan.session,
            &plan.target_cli,
            &plan.rewind,
            &plan.compiler,
        ));
        self.set_status("Lark export command copied");
        self.pending_g = false;
    }

    fn handle_share_panel_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => self.back_or_quit(),
            KeyCode::Char('j') | KeyCode::Down => self.move_share_panel_selection(true),
            KeyCode::Char('k') | KeyCode::Up => self.move_share_panel_selection(false),
            KeyCode::Enter => self.execute_share_panel_selection(),
            _ => {}
        }
    }

    fn move_share_panel_selection(&mut self, forward: bool) {
        let count = SharePanelActionKind::ALL.len();
        if forward {
            self.share_panel_selection = (self.share_panel_selection + 1) % count;
        } else if self.share_panel_selection == 0 {
            self.share_panel_selection = count - 1;
        } else {
            self.share_panel_selection -= 1;
        }
        let kind = SharePanelActionKind::ALL[self.share_panel_selection];
        self.set_status(format!("Yank candidate: {}", share_action_label(kind)));
    }

    fn execute_share_panel_selection(&mut self) {
        let kind = SharePanelActionKind::ALL[self.share_panel_selection];
        let (status, reason) = self.share_action_state(kind);
        if !action_is_runnable(status) {
            self.set_status(format!(
                "{} unavailable: {reason}",
                share_action_label(kind)
            ));
            return;
        }
        match kind {
            SharePanelActionKind::FirstUserInput => self.copy_share_first_user_input(),
            SharePanelActionKind::LastAiOutput => self.copy_share_last_ai_output(),
            SharePanelActionKind::SessionId => self.copy_share_session_id(),
            SharePanelActionKind::HandoffContent => self.copy_share_handoff_content(),
            SharePanelActionKind::PortableJson => self.copy_share_portable_json(),
        }
    }

    fn queue_seed_prompt_session(&mut self) {
        self.show_action_menu = false;
        if !self.current_data_space().is_local() {
            self.set_status("SSH sessions cannot start local target sessions");
            self.pending_g = false;
            return;
        }
        if !self.ensure_session_details_ready("New session") {
            return;
        }
        let Some(session) = self.current_session().cloned() else {
            self.set_status("No session selected");
            self.pending_g = false;
            return;
        };
        let Some((prompt, path_count, missing_path_count)) = self.first_user_seed_prompt() else {
            self.set_status("No first user prompt to start from");
            self.pending_g = false;
            return;
        };
        let target = self.data.target;
        let command = launcher::new_session_command(target, &session, prompt);
        let plan = OriginalSessionPlan {
            version: 1,
            action: SessionAction::NewSession,
            dry_run: true,
            source_session: session.clone(),
            command,
        };
        self.modal_scroll = 0;
        let attachment_note = seed_prompt_attachment_note(path_count, missing_path_count);
        match original_resume_mode_from_env() {
            OriginalResumeMode::Suspend => {
                self.pending_seed_prompt = Some(Box::new(plan));
                self.set_status(format!(
                    "Starting {target} from first prompt{attachment_note}: {} {}",
                    session.cli, session.id
                ));
            }
            OriginalResumeMode::Exec => {
                self.exit_action = Some(TuiExitAction::NewSession(Box::new(plan)));
                self.should_quit = true;
                self.set_status(format!(
                    "Opening {target} from first prompt{attachment_note}: {} {}",
                    session.cli, session.id
                ));
            }
        }
        self.pending_g = false;
    }

    fn queue_native_fork(&mut self) {
        self.show_action_menu = false;
        if !self.current_data_space().is_local() {
            self.set_status("SSH sessions cannot be forked locally");
            self.pending_g = false;
            return;
        }
        if !self.ensure_session_details_ready("Fork") {
            return;
        }
        let Some(session) = self.current_session().cloned() else {
            self.set_status("No session selected");
            return;
        };
        let Some(command) = launcher::native_fork_command(&session) else {
            self.set_status(format!(
                "Fork unavailable: {} does not expose native session fork",
                session.cli
            ));
            self.pending_g = false;
            return;
        };
        let plan = OriginalSessionPlan {
            version: 1,
            action: SessionAction::NativeFork,
            dry_run: true,
            source_session: session.clone(),
            command,
        };
        self.modal_scroll = 0;
        match original_resume_mode_from_env() {
            OriginalResumeMode::Suspend => {
                self.pending_native_fork = Some(Box::new(plan));
                self.set_status(format!(
                    "Suspending to native fork: {} {}",
                    session.cli, session.id
                ));
            }
            OriginalResumeMode::Exec => {
                self.exit_action = Some(TuiExitAction::NativeFork(Box::new(plan)));
                self.should_quit = true;
                self.set_status(format!(
                    "Opening native fork: {} {}",
                    session.cli, session.id
                ));
            }
        }
        self.pending_g = false;
    }

    fn open_original(&mut self) {
        if !self.current_data_space().is_local() {
            self.show_open_original = false;
            self.set_status("SSH sessions cannot be opened locally; use handoff");
            self.pending_g = false;
            return;
        }
        if !self.ensure_session_details_ready("Original") {
            return;
        }
        self.show_open_original = true;
        self.modal_scroll = 0;
        if let Some(session) = self.current_session() {
            self.set_status(format!("Original ready: {} {}", session.cli, session.id));
        } else {
            self.set_status("No session selected");
        }
        self.pending_g = false;
    }

    fn queue_original_resume(&mut self) {
        self.queue_original_resume_with_mode(original_resume_mode_from_env());
    }

    fn queue_original_resume_with_status(&mut self, status: String) {
        self.queue_original_resume_with_mode_and_status(
            original_resume_mode_from_env(),
            Some(status),
        );
    }

    fn queue_original_resume_with_mode(&mut self, mode: OriginalResumeMode) {
        self.queue_original_resume_with_mode_and_status(mode, None);
    }

    fn queue_original_resume_with_mode_and_status(
        &mut self,
        mode: OriginalResumeMode,
        status: Option<String>,
    ) {
        self.show_action_menu = false;
        if !self.current_data_space().is_local() {
            self.show_open_original = false;
            self.set_status("SSH sessions cannot be resumed locally; use handoff");
            self.pending_g = false;
            return;
        }
        if !self.ensure_session_details_ready("Original") {
            return;
        }
        let Some(session) = self.current_session().cloned() else {
            self.set_status("No session selected");
            return;
        };
        let command = launcher::original_command(&session);
        let plan = OriginalSessionPlan {
            version: 1,
            action: SessionAction::OriginalResume,
            dry_run: true,
            source_session: session.clone(),
            command,
        };
        self.show_open_original = false;
        self.modal_scroll = 0;
        match mode {
            OriginalResumeMode::Suspend => {
                self.pending_resume = Some(Box::new(plan));
                self.set_status(status.unwrap_or_else(|| {
                    format!("Suspending to original: {} {}", session.cli, session.id)
                }));
            }
            OriginalResumeMode::Exec => {
                self.exit_action = Some(TuiExitAction::OriginalResume(Box::new(plan)));
                self.should_quit = true;
                self.set_status(status.unwrap_or_else(|| {
                    format!("Opening original: {} {}", session.cli, session.id)
                }));
            }
        }
    }

    pub fn current_session(&self) -> Option<&SessionSummary> {
        self.visible_session_indices
            .contains(&self.selected_session)
            .then(|| self.data.sessions.get(self.selected_session))
            .flatten()
    }

    pub fn visible_session_indices(&self) -> &[usize] {
        &self.visible_session_indices
    }

    pub fn is_session_starred(&self, session: &SessionSummary) -> bool {
        let key = session_overlay_key(session);
        self.starred_sessions.iter().any(|item| item == &key)
    }

    pub fn is_session_archived(&self, session: &SessionSummary) -> bool {
        let key = session_overlay_key(session);
        self.archived_sessions.iter().any(|item| item == &key)
    }

    pub fn archive_feedback_for_session(
        &self,
        session: &SessionSummary,
    ) -> Option<ArchiveFeedbackKind> {
        let feedback = self.pending_archive_feedback.as_ref()?;
        (feedback.session_key == session_overlay_key(session)).then_some(feedback.kind)
    }

    fn refresh_visible_sessions(&mut self) {
        let query = self.search_query.trim().to_ascii_lowercase();
        self.visible_session_indices = self
            .data
            .sessions
            .iter()
            .enumerate()
            .filter(|(_, session)| {
                self.session_filter.matches(
                    session,
                    &self.starred_sessions,
                    &self.archived_sessions,
                )
            })
            .filter(|(_, session)| session_matches_query(session, &query))
            .filter(|(_, session)| {
                !is_moonbox_handoff_worker_session(session)
                    || query_explicitly_requests_moonbox_handoff_workers(&query)
            })
            .map(|(index, _)| index)
            .collect();
    }

    fn move_session(&mut self, forward: bool) {
        if self.visible_session_indices.is_empty() {
            return;
        }
        let current = self
            .visible_session_indices
            .iter()
            .position(|index| *index == self.selected_session)
            .unwrap_or(0);
        let next = if forward {
            (current + 1).min(self.visible_session_indices.len().saturating_sub(1))
        } else {
            current.saturating_sub(1)
        };
        self.select_session_index(self.visible_session_indices[next]);
    }

    fn select_session_index(&mut self, session_index: usize) {
        if self.selected_session == session_index {
            return;
        }
        self.selected_session = session_index;
        self.defer_selected_session_context();
    }

    fn defer_selected_session_context(&mut self) {
        self.session_load_request_id = self.session_load_request_id.wrapping_add(1);
        self.pending_session_load = None;
        self.session_preview_request_id = self.session_preview_request_id.wrapping_add(1);
        self.pending_session_preview = None;
        self.deferred_session_preview = None;
        self.selected_event = 0;
        self.rewind_event_id.clear();
        self.capsule_scroll = 0;
        if self.selected_session_timeline_loaded() {
            return;
        }
        let Some(session) = self.data.sessions.get(self.selected_session).cloned() else {
            self.compile_status = "PENDING";
            self.verify_passed = false;
            return;
        };
        self.compile_status = "PENDING";
        self.verify_passed = false;
        self.schedule_selected_session_preview(session.id);
    }

    fn cycle_session_filter(&mut self, forward: bool) {
        let filter = if forward {
            self.session_filter.next()
        } else {
            self.session_filter.previous()
        };
        self.apply_session_filter(filter);
    }

    pub fn apply_session_filter(&mut self, filter: SessionFilter) {
        self.session_filter = filter;
        self.refresh_visible_sessions();
        self.clamp_selected_session();
        self.defer_selected_session_context();
        self.set_status(format!("Filter: {}", self.session_filter.label()));
        self.pending_g = false;
    }

    fn clear_session_filters(&mut self) {
        self.session_filter = SessionFilter::All;
        self.search_query.clear();
        self.refresh_visible_sessions();
        self.clamp_selected_session();
        self.defer_selected_session_context();
        self.set_status("Filters cleared");
        self.pending_g = false;
    }

    fn cycle_data_space(&mut self, forward: bool) {
        if self.data_spaces.len() <= 1 {
            self.set_status("Data space: Local only");
            self.pending_g = false;
            return;
        }
        let len = self.data_spaces.len();
        let next = if forward {
            (self.selected_data_space + 1) % len
        } else if self.selected_data_space == 0 {
            len - 1
        } else {
            self.selected_data_space - 1
        };
        self.load_data_space(next);
    }

    fn load_data_space(&mut self, index: usize) {
        let Some(space) = self.data_spaces.get(index).cloned() else {
            self.set_status("Data space not found");
            self.pending_g = false;
            return;
        };
        self.data_space_load_request_id = self.data_space_load_request_id.wrapping_add(1);
        let request_id = self.data_space_load_request_id;
        let source = self.data.source;
        let target = self.data.target;
        let worker_space = space.clone();
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let result = workbench::load_workbench_for_data_space(&worker_space, source, target);
            let _ = sender.send(result);
        });
        self.pending_session_load = None;
        self.pending_session_preview = None;
        self.deferred_session_preview = None;
        self.session_preview_request_id = self.session_preview_request_id.wrapping_add(1);
        self.pending_data_space_load = Some(PendingDataSpaceLoad {
            request_id,
            index,
            space: space.clone(),
            started_at: Instant::now(),
            receiver,
        });
        self.data_space_selection = index;
        self.data_space_error = None;
        self.compile_status = "LOADING";
        self.verify_passed = false;
        self.set_status(format!("Loading data space: {}", space.label));
        self.pending_g = false;
    }

    fn toggle_starred_session(&mut self) {
        let Some(session) = self.current_session() else {
            self.set_status("No session selected");
            self.pending_g = false;
            return;
        };
        let key = session_overlay_key(session);
        let starred =
            if let Some(index) = self.starred_sessions.iter().position(|item| item == &key) {
                self.starred_sessions.remove(index);
                false
            } else {
                self.starred_sessions.push(key);
                self.starred_sessions.sort();
                self.starred_sessions.dedup();
                true
            };
        if let Err(error) = config::save_starred_sessions(&self.starred_sessions) {
            self.set_status(format!("Star save failed: {error}"));
        } else if starred {
            self.set_status("Session starred");
        } else {
            self.set_status("Session unstarred");
        }
        self.refresh_visible_sessions();
        self.clamp_selected_session();
        self.pending_g = false;
    }

    fn toggle_archived_session(&mut self) {
        self.commit_pending_archive_feedback();
        let Some(session) = self.current_session() else {
            self.set_status("No session selected");
            self.pending_g = false;
            return;
        };
        let key = session_overlay_key(session);
        let visible_position = self
            .visible_session_indices
            .iter()
            .position(|index| *index == self.selected_session)
            .unwrap_or(0);
        let archived = self.archived_sessions.iter().any(|item| item == &key);
        let kind = if archived {
            ArchiveFeedbackKind::Unarchive
        } else {
            ArchiveFeedbackKind::Archive
        };
        self.pending_archive_feedback = Some(PendingArchiveFeedback {
            session_key: key,
            session_id: session.id.clone(),
            kind,
            visible_position,
            started_tick: self.animation_tick,
        });
        self.set_status(match kind {
            ArchiveFeedbackKind::Archive => "Archiving session...",
            ArchiveFeedbackKind::Unarchive => "Unarchiving session...",
        });
        self.pending_g = false;
    }

    fn advance_archive_feedback(&mut self) {
        let Some(feedback) = self.pending_archive_feedback.as_ref() else {
            return;
        };
        if self.animation_tick.wrapping_sub(feedback.started_tick) >= ARCHIVE_FEEDBACK_FRAMES {
            self.commit_pending_archive_feedback();
        }
    }

    fn commit_pending_archive_feedback(&mut self) {
        let Some(feedback) = self.pending_archive_feedback.take() else {
            return;
        };
        let mut archived_sessions = self.archived_sessions.clone();
        match feedback.kind {
            ArchiveFeedbackKind::Archive => {
                archived_sessions.push(feedback.session_key.clone());
                archived_sessions.sort();
                archived_sessions.dedup();
            }
            ArchiveFeedbackKind::Unarchive => {
                archived_sessions.retain(|key| key != &feedback.session_key);
            }
        }
        if let Err(error) = config::save_archived_sessions(&archived_sessions) {
            self.set_status(format!("Archive save failed: {error}"));
            self.pending_g = false;
            return;
        }
        self.archived_sessions = archived_sessions;
        self.refresh_visible_sessions();
        self.select_visible_session_position(feedback.visible_position);
        self.set_status(match feedback.kind {
            ArchiveFeedbackKind::Archive => format!("Session archived: {}", feedback.session_id),
            ArchiveFeedbackKind::Unarchive => {
                format!("Session unarchived: {}", feedback.session_id)
            }
        });
        self.pending_g = false;
    }

    fn select_visible_session_position(&mut self, position: usize) {
        if self.visible_session_indices.is_empty() {
            self.selected_session = 0;
            self.pending_session_load = None;
            self.pending_session_preview = None;
            self.deferred_session_preview = None;
            return;
        }
        let next_position = position.min(self.visible_session_indices.len().saturating_sub(1));
        self.select_session_index(self.visible_session_indices[next_position]);
    }

    fn clamp_selected_session(&mut self) {
        if self.visible_session_indices.is_empty() {
            self.selected_session = 0;
        } else if !self
            .visible_session_indices
            .contains(&self.selected_session)
        {
            self.selected_session = self.visible_session_indices[0];
        }
    }

    fn has_overlay(&self) -> bool {
        self.show_help
            || self.show_share_panel
            || self.show_action_menu
            || self.show_open_original
            || self.show_doctor
            || self.show_skill_picker
            || self.show_capsules
            || self.show_settings
            || self.show_data_spaces
            || self.show_data_space_config
            || self.show_timeline_detail
    }

    fn cycle_target(&mut self, forward: bool) {
        self.pending_target = if forward {
            self.pending_target.next()
        } else {
            self.pending_target.previous()
        };
        self.launch_review_error = None;
        self.launch_review_details = false;
        self.set_status(format!("Target: {}", self.pending_target));
    }

    fn open_launch_picker(&mut self) {
        self.launch_review_lark_export = false;
        if self.reveal_pending_launch_review() {
            return;
        }
        self.refresh_compiler_catalog();
        if self.launch_review_error.is_some() {
            self.show_launch = true;
            self.launch_review = false;
            self.launch_review_details = false;
            self.target_launch_result = None;
            self.modal_scroll = 0;
            self.set_status(format!("Handoff review failed: {}", self.pending_target));
            self.pending_g = false;
            return;
        }
        if self.launch_review {
            self.show_launch = true;
            self.target_launch_result = None;
            self.launch_review_details = false;
            self.modal_scroll = u16::MAX;
            self.set_status(format!("Handoff review ready: {}", self.pending_target));
            self.pending_g = false;
            return;
        }
        if self.current_session().is_none() {
            self.show_launch = false;
            self.set_status("No session selected");
            self.pending_g = false;
            return;
        }
        self.pending_target = self.data.target;
        self.ensure_launch_handoff_skill_default();
        self.show_launch = true;
        self.launch_review = false;
        self.launch_review_details = false;
        self.target_launch_result = None;
        self.clear_handoff_trail();
        self.modal_scroll = 0;
        self.set_status("Choose target CLI");
        self.pending_g = false;
    }

    fn open_launch_picker_for_remote_session(&mut self) {
        self.open_launch_picker();
        if self.show_launch {
            self.set_status("SSH source is read-only; choose a local target for handoff");
        }
    }

    fn open_timeline_detail(&mut self) {
        if !self.ensure_session_details_ready("Timeline detail") {
            return;
        }
        if self.data.timeline.is_empty() {
            self.set_status("No timeline event selected");
            self.pending_g = false;
            return;
        }
        self.selected_event =
            nearest_visible_timeline_event(&self.data, &self.rewind_event_id, self.selected_event);
        self.timeline_image_previews =
            build_timeline_image_previews(&self.data, self.selected_event);
        self.show_timeline_detail = true;
        self.modal_scroll = 0;
        if let Some(event) = self.data.timeline.get(self.selected_event) {
            self.set_status(format!("Timeline detail: {}", event.id));
        } else {
            self.set_status("No timeline event selected");
        }
        self.pending_g = false;
    }

    fn ensure_session_details_ready(&mut self, action: &str) -> bool {
        if self.is_session_load_pending() {
            self.set_status(format!("{action} waits for selected session to load"));
            self.pending_g = false;
            return false;
        }
        if !self.selected_session_timeline_loaded() {
            self.request_selected_session_details();
            if self.is_session_load_pending() {
                self.set_status(format!("{action} is loading selected session details"));
            } else if self.is_session_preview_pending() {
                self.set_status(format!("{action} is waiting for timeline preview"));
            } else {
                self.set_status(format!("{action} needs a loaded session timeline"));
            }
            self.pending_g = false;
            return false;
        }
        true
    }

    fn reveal_pending_launch_review(&mut self) -> bool {
        let Some(pending) = self.pending_launch_review.as_ref() else {
            return false;
        };
        let target = pending.target;
        let selected_compiler = pending.selected_compiler;
        let stage = pending.stage.label();
        let detail = pending.stage_detail.clone();
        self.pending_target = target;
        self.selected_compiler = selected_compiler;
        self.show_launch = true;
        self.launch_review = false;
        self.launch_review_details = false;
        self.target_launch_result = None;
        self.launch_review_error = None;
        self.modal_scroll = 0;
        self.set_status(format!(
            "Handoff job already running: {target} {stage} - {detail}"
        ));
        self.pending_g = false;
        true
    }

    fn confirm_launch_target(&mut self) {
        if self.reveal_pending_launch_review() {
            return;
        }
        self.refresh_compiler_catalog();
        let target = self.pending_target;
        if let Some(plan) = self.selected_compiler_setup_install_plan() {
            self.queue_setup_install(plan);
            return;
        }
        let validation = self.validate_launch_for_target(target);
        if self.launch_requires_handoff_skill(target) {
            self.open_skill_picker();
            self.show_launch = true;
            self.launch_review_error = None;
            self.launch_review_details = false;
            self.set_status("Choose an AI handoff skill before Handoff Review");
            return;
        }
        if validation.is_blocked() && !launch_validation_can_regenerate_handoff(&validation) {
            self.set_status(format!("Target blocked: {}", validation.summary()));
            self.pending_g = false;
            return;
        }
        let Some(session) = self.current_session().cloned() else {
            self.set_status("No session selected");
            self.pending_g = false;
            return;
        };
        let session_id = session.id.clone();
        let selected_compiler = self.selected_compiler;
        let compiler_id = self
            .data
            .compilers
            .get(selected_compiler)
            .cloned()
            .unwrap_or_else(compiler::default_compiler_id);
        let rewind_event_id = self.rewind_event_id.clone();
        if self.handoff_review_ready_for(&session_id, target, &compiler_id, &rewind_event_id) {
            self.pending_target = target;
            self.show_launch = true;
            self.launch_review = true;
            self.launch_review_details = false;
            self.target_launch_result = None;
            self.launch_review_error = None;
            self.modal_scroll = u16::MAX;
            self.set_status(format!("Handoff review ready: {target}"));
            self.pending_g = false;
            return;
        }
        let space = self.current_data_space().clone();
        let sessions = self.data.sessions.clone();
        let source_adapters = self.data.source_adapters.clone();
        let worker_session_id = session_id.clone();
        let worker_compiler_id = compiler_id.clone();
        let worker_rewind_event_id = rewind_event_id.clone();
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let _ = sender.send(LaunchReviewMessage::Progress(LaunchReviewProgress {
                stage: LaunchReviewStage::PreparingContext,
                detail: if space.is_local() {
                    "reading local timeline snapshot".into()
                } else {
                    "reading SSH timeline snapshot in read-only mode".into()
                },
            }));
            let result = if space.is_local() {
                workbench::load_workbench_from_session_snapshot(
                    session,
                    sessions,
                    source_adapters,
                    target,
                )
            } else {
                workbench::load_remote_workbench_from_session_snapshot(
                    &space, session, sessions, target,
                )
            }
            .and_then(|mut data| {
                let _ = sender.send(LaunchReviewMessage::Progress(LaunchReviewProgress {
                    stage: LaunchReviewStage::StartingRunner,
                    detail: format!("selected compiler {worker_compiler_id}"),
                }));
                let _ = sender.send(LaunchReviewMessage::Progress(LaunchReviewProgress {
                    stage: LaunchReviewStage::RunningSkill,
                    detail: "generating handoff artifact".into(),
                }));
                let Some(capsule) = workbench::compile_capsule_from_workbench_snapshot(
                    &data,
                    &worker_session_id,
                    target,
                    &worker_rewind_event_id,
                    &worker_compiler_id,
                )?
                else {
                    return Err(CoreError::Compiler(
                        compiler::CompilerError::InvalidConfig {
                            name: worker_compiler_id,
                            reason: "session not found in loaded handoff context".into(),
                        },
                    ));
                };
                let _ = sender.send(LaunchReviewMessage::Progress(LaunchReviewProgress {
                    stage: LaunchReviewStage::Verifying,
                    detail: "normalizing and verifying review data".into(),
                }));
                data.capsule = capsule;
                Ok(data)
            });
            let _ = sender.send(LaunchReviewMessage::Finished(Box::new(result)));
        });

        self.launch_review_request_id = self.launch_review_request_id.wrapping_add(1);
        self.pending_launch_review = Some(PendingLaunchReview {
            request_id: self.launch_review_request_id,
            session_id,
            target,
            selected_compiler,
            compiler_id,
            rewind_event_id,
            started_at: Instant::now(),
            timeout_ms: u128::from(handoff::configured_agent_timeout_ms()),
            stage: LaunchReviewStage::Queued,
            stage_detail: "waiting for background handoff worker".into(),
            receiver,
        });
        let _ = config::save_last_target(target);
        self.show_launch = true;
        self.launch_review = false;
        self.launch_review_details = false;
        self.target_launch_result = None;
        self.launch_review_error = None;
        self.start_handoff_trail_for_review();
        self.modal_scroll = 0;
        if launch_validation_can_regenerate_handoff(&validation) {
            self.set_status(format!("Regenerating handoff review: {target}"));
        } else if validation.state == LaunchValidationState::Warning {
            self.set_status(format!(
                "Preparing handoff review: {target} ({})",
                validation.summary()
            ));
        } else {
            self.set_status(format!("Preparing handoff review: {target}"));
        }
        self.pending_g = false;
    }

    fn handoff_review_ready_for(
        &self,
        session_id: &str,
        target: CliTool,
        compiler_id: &str,
        rewind_event_id: &str,
    ) -> bool {
        self.data.capsule.source_session == session_id
            && self.data.capsule.target_cli == target
            && self.data.capsule.compiler == compiler_id
            && self
                .data
                .capsule
                .rewind_point
                .split_whitespace()
                .next()
                .is_some_and(|id| id == rewind_event_id)
            && (self.launch_review || self.data.capsule.handoff_artifact.is_some())
    }

    fn review_capsule(&mut self) {
        if self.compile_capsule_for_review() {
            self.pending_target = self.data.target;
            self.show_launch = true;
            self.launch_review = true;
            self.launch_review_details = false;
            self.modal_scroll = 0;
            self.set_status("Capsule refreshed");
        }
        self.pending_g = false;
    }

    fn compile_capsule_for_review(&mut self) -> bool {
        if !self.ensure_session_details_ready("Review") {
            return false;
        }
        let compiler = self.data.compilers[self.selected_compiler].clone();
        let Some(session_id) = self.current_session().map(|session| session.id.clone()) else {
            self.compile_status = "FAILED";
            self.set_status("Review failed: no session selected");
            return false;
        };
        match workbench::compile_capsule(
            &session_id,
            self.data.target,
            &self.rewind_event_id,
            &compiler,
        ) {
            Ok(Some(capsule)) => {
                self.compile_status = "COMPILED";
                self.data.capsule = capsule;
                true
            }
            Ok(None) => {
                self.compile_status = "FAILED";
                self.set_status("Review failed: session not found");
                false
            }
            Err(error) => {
                self.compile_status = "FAILED";
                self.set_status(format!("Review failed: {error}"));
                false
            }
        }
    }

    fn request_selected_session_preview(&mut self) {
        self.deferred_session_preview = None;
        if self.selected_session_timeline_loaded() || !self.current_data_space().is_local() {
            return;
        }
        let Some(session) = self.data.sessions.get(self.selected_session).cloned() else {
            self.pending_session_preview = None;
            return;
        };
        let target = self.data.target;
        if self
            .pending_session_preview
            .as_ref()
            .is_some_and(|pending| pending.session_id == session.id && pending.target == target)
        {
            return;
        }

        self.session_preview_request_id = self.session_preview_request_id.wrapping_add(1);
        let request_id = self.session_preview_request_id;
        let sessions = self.data.sessions.clone();
        let source_adapters = self.data.source_adapters.clone();
        let worker_session = session.clone();
        let worker_session_id = session.id.clone();
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let result = workbench::load_workbench_from_session_snapshot(
                worker_session,
                sessions,
                source_adapters,
                target,
            );
            let _ = sender.send(result);
        });

        self.pending_session_preview = Some(PendingSessionPreview {
            request_id,
            session_id: worker_session_id,
            target,
            started_at: Instant::now(),
            receiver,
        });
    }

    fn schedule_selected_session_preview(&mut self, session_id: String) {
        if self.selected_session_timeline_loaded()
            || self.is_session_load_pending()
            || !self.current_data_space().is_local()
        {
            self.deferred_session_preview = None;
            return;
        }
        self.deferred_session_preview = Some(DeferredSessionPreview {
            session_id,
            due_at: Instant::now() + Duration::from_millis(SESSION_PREVIEW_DEBOUNCE_MS),
        });
    }

    fn start_deferred_session_preview_if_due(&mut self) -> bool {
        let Some(deferred) = self.deferred_session_preview.as_ref() else {
            return false;
        };
        if Instant::now() < deferred.due_at {
            return false;
        }
        let Some(deferred) = self.deferred_session_preview.take() else {
            return false;
        };
        let Some(session) = self.current_session() else {
            return true;
        };
        if session.id != deferred.session_id || self.selected_session_timeline_loaded() {
            return true;
        }
        self.request_selected_session_preview();
        true
    }

    fn request_selected_session_details(&mut self) {
        let Some(session) = self.data.sessions.get(self.selected_session).cloned() else {
            self.pending_session_load = None;
            return;
        };
        if !self.current_data_space().is_local() {
            self.request_remote_session_details(session);
            return;
        }
        let target = self.data.target;
        if self.selected_session_timeline_loaded() && self.data.target == target {
            return;
        }

        self.session_load_request_id = self.session_load_request_id.wrapping_add(1);
        self.session_preview_request_id = self.session_preview_request_id.wrapping_add(1);
        self.pending_session_preview = None;
        self.deferred_session_preview = None;
        let request_id = self.session_load_request_id;
        let selected_session = self.selected_session;
        let selected_compiler = self.selected_compiler;
        let sessions = self.data.sessions.clone();
        let source_adapters = self.data.source_adapters.clone();
        let worker_session = session.clone();
        let worker_session_id = session.id.clone();
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let result = workbench::load_workbench_from_session_snapshot(
                worker_session,
                sessions,
                source_adapters,
                target,
            );
            let _ = sender.send(result);
        });

        self.pending_session_load = Some(PendingSessionLoad {
            request_id,
            session_id: worker_session_id,
            target,
            started_at: Instant::now(),
            receiver,
        });
        self.selected_session = selected_session.min(self.data.sessions.len().saturating_sub(1));
        self.selected_compiler = selected_compiler.min(self.data.compilers.len().saturating_sub(1));
        self.compile_status = "LOADING";
        self.verify_passed = false;
        self.set_status(format!(
            "Loading timeline: {} {}",
            session.cli, session.title
        ));
    }

    fn request_remote_session_details(&mut self, session: SessionSummary) {
        if self.data.capsule.source_session == session.id
            && !self.data.timeline.is_empty()
            && self.pending_session_load.is_none()
        {
            return;
        }
        let target = self.data.target;
        self.session_load_request_id = self.session_load_request_id.wrapping_add(1);
        let request_id = self.session_load_request_id;
        self.deferred_session_preview = None;
        let selected_compiler = self.selected_compiler;
        let selected_session = self.selected_session;
        let space = self.current_data_space().clone();
        let sessions = self.data.sessions.clone();
        let worker_session = session.clone();
        let worker_session_id = session.id.clone();
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let result = workbench::load_remote_workbench_from_session_snapshot(
                &space,
                worker_session,
                sessions,
                target,
            );
            let _ = sender.send(result);
        });

        self.pending_session_load = Some(PendingSessionLoad {
            request_id,
            session_id: worker_session_id,
            target,
            started_at: Instant::now(),
            receiver,
        });
        self.selected_session = selected_session.min(self.data.sessions.len().saturating_sub(1));
        self.selected_compiler = selected_compiler.min(self.data.compilers.len().saturating_sub(1));
        self.compile_status = "LOADING";
        self.verify_passed = false;
        self.set_status(format!(
            "Loading remote session: {} {}",
            session.cli, session.title
        ));
    }

    fn apply_data_space_load_result(
        &mut self,
        pending: PendingDataSpaceLoad,
        result: DataSpaceLoadResult,
    ) {
        if self.data_space_load_request_id != pending.request_id {
            return;
        }
        let elapsed = pending.started_at.elapsed();
        match result {
            Ok(data) => {
                let selected_compiler = self.selected_compiler;
                let selected_compiler_id = self
                    .data
                    .compilers
                    .get(selected_compiler)
                    .cloned()
                    .unwrap_or_else(|| self.data.capsule.compiler.clone());
                self.data = data;
                self.selected_data_space = pending.index;
                self.data_space_selection = pending.index;
                self.data_space_error = None;
                self.refresh_visible_sessions();
                self.clamp_selected_session();
                self.selected_event =
                    rewind_event_index(&self.data, &initial_rewind_event_id(&self.data));
                self.rewind_event_id = initial_rewind_event_id(&self.data);
                self.selected_compiler =
                    compiler_index_for_id(&self.data, &selected_compiler_id, selected_compiler);
                self.data.capsule.compiler = self.data.compilers[self.selected_compiler].clone();
                self.capsule_scroll = 0;
                self.compile_status = "ACTIVE";
                self.verify_passed = pending.space.is_local();
                self.set_status(format!(
                    "Data space: {} ({} sessions, {} ms)",
                    pending.space.label,
                    self.data.sessions.len(),
                    elapsed.as_millis()
                ));
                if !pending.space.is_local() {
                    self.request_selected_session_details();
                } else {
                    if let Some(session) = self.current_session() {
                        self.schedule_selected_session_preview(session.id.clone());
                    }
                }
            }
            Err(error) => {
                self.compile_status = "FAILED";
                self.verify_passed = false;
                self.show_data_spaces = true;
                self.data_space_selection = pending.index;
                self.data_space_error = Some(error.to_string());
                self.set_status(format!(
                    "Data space failed: {} ({} ms)",
                    error,
                    elapsed.as_millis()
                ));
            }
        }
    }

    fn apply_session_load_result(
        &mut self,
        pending: PendingSessionLoad,
        result: SessionLoadResult,
    ) {
        if self.session_load_request_id != pending.request_id
            || self
                .data
                .sessions
                .get(self.selected_session)
                .is_none_or(|session| session.id != pending.session_id)
            || self.data.target != pending.target
        {
            return;
        }

        let elapsed = pending.started_at.elapsed();
        let selected_compiler = self.selected_compiler;
        let selected_compiler_id = self
            .data
            .compilers
            .get(selected_compiler)
            .cloned()
            .unwrap_or_else(|| self.data.capsule.compiler.clone());
        match result {
            Ok(data) => {
                let launch_was_waiting = self.show_launch
                    && !self.launch_review
                    && self.target_launch_result.is_none()
                    && self.launch_review_error.is_none()
                    && self.pending_launch_review.is_none();
                let rewind_event_id = initial_rewind_event_id(&data);
                let selected_event = rewind_event_index(&data, &rewind_event_id);
                self.data = data;
                self.refresh_visible_sessions();
                self.selected_session = self
                    .data
                    .sessions
                    .iter()
                    .position(|session| session.id == pending.session_id)
                    .unwrap_or(self.selected_session)
                    .min(self.data.sessions.len().saturating_sub(1));
                self.selected_event = selected_event;
                self.selected_compiler =
                    compiler_index_for_id(&self.data, &selected_compiler_id, selected_compiler);
                self.data.capsule.compiler = self.data.compilers[self.selected_compiler].clone();
                self.rewind_event_id = rewind_event_id;
                self.capsule_scroll = 0;
                self.compile_status = "PREVIEW";
                self.verify_passed = false;
                if launch_was_waiting {
                    self.set_status("Choose target CLI");
                } else if let Some(session) = self.current_session() {
                    self.set_status(format!(
                        "Timeline: {} {} ({} events, {} ms)",
                        session.cli,
                        session.title,
                        self.data.timeline.len(),
                        elapsed.as_millis()
                    ));
                } else {
                    self.set_status(format!("Session loaded ({} ms)", elapsed.as_millis()));
                }
            }
            Err(error) => {
                self.compile_status = "FAILED";
                self.set_status(format!(
                    "Session reload failed: {error} ({} ms)",
                    elapsed.as_millis()
                ));
            }
        }
    }

    fn apply_session_preview_result(
        &mut self,
        pending: PendingSessionPreview,
        result: SessionPreviewResult,
    ) {
        if self.session_preview_request_id != pending.request_id
            || self
                .data
                .sessions
                .get(self.selected_session)
                .is_none_or(|session| session.id != pending.session_id)
            || self.data.target != pending.target
            || self.pending_session_load.is_some()
        {
            return;
        }

        let elapsed = pending.started_at.elapsed();
        let selected_compiler = self.selected_compiler;
        let selected_compiler_id = self
            .data
            .compilers
            .get(selected_compiler)
            .cloned()
            .unwrap_or_else(|| self.data.capsule.compiler.clone());
        match result {
            Ok(data) => {
                let rewind_event_id = initial_rewind_event_id(&data);
                let selected_event = rewind_event_index(&data, &rewind_event_id);
                self.data = data;
                self.refresh_visible_sessions();
                self.selected_session = self
                    .data
                    .sessions
                    .iter()
                    .position(|session| session.id == pending.session_id)
                    .unwrap_or(self.selected_session)
                    .min(self.data.sessions.len().saturating_sub(1));
                self.selected_event = selected_event;
                self.selected_compiler =
                    compiler_index_for_id(&self.data, &selected_compiler_id, selected_compiler);
                self.data.capsule.compiler = self.data.compilers[self.selected_compiler].clone();
                self.rewind_event_id = rewind_event_id;
                self.capsule_scroll = 0;
                self.compile_status = "PREVIEW";
                self.verify_passed = false;
                if let Some(session) = self.current_session() {
                    self.set_status(format!(
                        "Timeline preview: {} {} ({} events, {} ms)",
                        session.cli,
                        session.title,
                        self.data.timeline.len(),
                        elapsed.as_millis()
                    ));
                } else {
                    self.set_status(format!(
                        "Timeline preview loaded ({} ms)",
                        elapsed.as_millis()
                    ));
                }
            }
            Err(error) => {
                self.set_status(format!(
                    "Timeline preview failed: {error} ({} ms)",
                    elapsed.as_millis()
                ));
            }
        }
    }

    fn apply_launch_review_result(
        &mut self,
        pending: PendingLaunchReview,
        result: LaunchReviewResult,
    ) {
        if self.launch_review_request_id != pending.request_id {
            return;
        }

        let elapsed = pending.started_at.elapsed();
        let review_was_visible = self.show_launch;
        match result {
            Ok(data) => {
                self.data = data;
                self.launch_review_error = None;
                self.refresh_visible_sessions();
                self.selected_session = self
                    .data
                    .sessions
                    .iter()
                    .position(|session| session.id == pending.session_id)
                    .unwrap_or(self.selected_session)
                    .min(self.data.sessions.len().saturating_sub(1));
                self.selected_event = self
                    .selected_event
                    .min(self.data.timeline.len().saturating_sub(1));
                self.selected_compiler = self
                    .data
                    .compilers
                    .iter()
                    .position(|compiler| compiler == &pending.compiler_id)
                    .unwrap_or(pending.selected_compiler)
                    .min(self.data.compilers.len().saturating_sub(1));
                if let Some(compiler) = self.data.compilers.get(self.selected_compiler).cloned() {
                    self.data.capsule.compiler = compiler;
                }
                if let Some(title) = self.timeline_event_title(&pending.rewind_event_id) {
                    let rewind_event_id = pending.rewind_event_id;
                    self.apply_rewind_event(rewind_event_id.clone(), title);
                    self.selected_event = rewind_event_index(&self.data, &rewind_event_id);
                } else {
                    self.rewind_event_id = initial_rewind_event_id(&self.data);
                    self.selected_event = rewind_event_index(&self.data, &self.rewind_event_id);
                }
                self.pending_target = pending.target;
                self.show_launch = review_was_visible;
                self.launch_review = true;
                self.launch_review_details = false;
                self.target_launch_result = None;
                self.compile_status = "ACTIVE";
                self.verify_passed = true;
                self.modal_scroll = u16::MAX;
                self.pending_g = false;
                if self.pending_share_handoff_copy {
                    self.pending_share_handoff_copy = false;
                    let copied = self.selected_handoff_artifact().map(str::to_string);
                    self.show_launch = false;
                    self.launch_review = false;
                    self.launch_review_details = false;
                    self.show_share_panel = true;
                    self.modal_scroll = 0;
                    if let Some(artifact) = copied {
                        self.clipboard_text = Some(artifact);
                        self.set_status(format!(
                            "Copied generated handoff: {} ({} ms)",
                            pending.target,
                            elapsed.as_millis()
                        ));
                    } else {
                        self.set_status(format!(
                            "Generated handoff has no copyable content: {} ({} ms)",
                            pending.target,
                            elapsed.as_millis()
                        ));
                    }
                } else if review_was_visible {
                    self.set_status(format!(
                        "Handoff review ready: {} ({} ms)",
                        pending.target,
                        elapsed.as_millis()
                    ));
                } else {
                    self.set_status(format!(
                        "Handoff ready in background: {} ({} ms)",
                        pending.target,
                        elapsed.as_millis()
                    ));
                }
            }
            Err(error) => {
                self.pending_share_handoff_copy = false;
                self.compile_status = "FAILED";
                self.verify_passed = false;
                self.pending_target = pending.target;
                self.show_launch = review_was_visible;
                self.launch_review = false;
                self.launch_review_details = false;
                self.target_launch_result = None;
                self.launch_review_error = Some(LaunchReviewErrorState {
                    target: pending.target,
                    compiler_id: pending.compiler_id,
                    message: error.to_string(),
                    elapsed_ms: elapsed.as_millis(),
                });
                self.modal_scroll = 0;
                self.pending_g = false;
                self.clear_handoff_trail();
                self.set_status(format!(
                    "Handoff review failed: {error} ({} ms)",
                    elapsed.as_millis()
                ));
            }
        }
    }

    fn set_status(&mut self, message: impl Into<String>) {
        self.status_message = message.into();
    }

    fn scroll_capsule(&mut self, forward: bool, amount: u16) {
        self.capsule_scroll = if forward {
            self.capsule_scroll.saturating_add(amount)
        } else {
            self.capsule_scroll.saturating_sub(amount)
        };
    }

    fn scroll_modal(&mut self, forward: bool, amount: u16) {
        self.modal_scroll = if forward {
            self.modal_scroll.saturating_add(amount)
        } else {
            self.modal_scroll.saturating_sub(amount)
        };
    }

    fn handle_modal_g(&mut self) {
        if self.pending_g {
            self.modal_scroll = 0;
            self.pending_g = false;
            self.set_status("Review top");
        } else {
            self.pending_g = true;
        }
    }

    fn copy_text(&mut self, label: &str, text: String) {
        self.clipboard_text = Some(text);
        self.set_status(format!("Copied {label} command"));
    }

    fn copy_focused_command(&mut self) {
        if self.show_launch {
            self.copy_launch_command();
        } else if self.show_open_original {
            self.copy_original_command();
        } else {
            self.set_status("No command to copy");
        }
        self.pending_g = false;
    }

    fn copy_launch_command(&mut self) {
        if self.launch_requires_handoff_skill(self.pending_target) {
            self.set_status("Choose an AI handoff skill before copying");
            return;
        }
        let validation = self.validate_launch_for_target(self.pending_target);
        if validation.is_blocked() {
            if launch_validation_can_regenerate_handoff(&validation) {
                self.set_status("Regenerate handoff with Enter before copying");
            } else {
                self.set_status(format!("Target blocked: {}", validation.summary()));
            }
            return;
        }
        if !self.launch_review {
            self.set_status("Confirm target first with enter");
            return;
        }
        self.copy_text("launch", self.launch_copy_command());
    }

    fn skill_handoff_review_ready(&self) -> bool {
        let capsule = self.launch_capsule_for_target(self.pending_target);
        capsule.handoff_artifact.is_some() && !compiler::compiler_is_builtin(&capsule.compiler)
    }

    fn copy_handoff_artifact(&mut self) {
        let capsule = self.launch_capsule_for_target(self.pending_target);
        let Some(artifact) = capsule.handoff_artifact else {
            self.set_status("No handoff content to copy");
            return;
        };
        self.clipboard_text = Some(artifact);
        self.set_status("Copied handoff text");
    }

    fn copy_handoff_artifact_path(&mut self) {
        let capsule = self.launch_capsule_for_target(self.pending_target);
        let Some(path) = capsule.handoff_artifact_path else {
            self.set_status("No handoff file path to copy");
            return;
        };
        self.clipboard_text = Some(path);
        self.set_status("Copied handoff path");
    }

    fn copy_share_first_user_input(&mut self) {
        if let Some(input) = self.first_user_input().map(str::to_string) {
            self.clipboard_text = Some(input);
            self.set_status("Copied first user input");
            self.pending_g = false;
            return;
        }
        if self.ensure_session_details_ready("Yank first user input") {
            self.set_status("No user input to copy");
        }
        self.pending_g = false;
    }

    fn copy_share_last_ai_output(&mut self) {
        if let Some(output) = self.last_ai_output().map(str::to_string) {
            self.clipboard_text = Some(output);
            self.set_status("Copied last AI output");
            self.pending_g = false;
            return;
        }
        if self.ensure_session_details_ready("Yank last AI output") {
            self.set_status("No assistant output to copy");
        }
        self.pending_g = false;
    }

    fn copy_share_session_id(&mut self) {
        let Some(session_id) = self.current_session().map(|session| session.id.clone()) else {
            self.set_status("No session selected");
            self.pending_g = false;
            return;
        };
        self.clipboard_text = Some(session_id);
        self.set_status("Copied Session ID");
        self.pending_g = false;
    }

    fn copy_share_handoff_content(&mut self) {
        if let Some(artifact) = self.selected_handoff_artifact().map(str::to_string) {
            self.clipboard_text = Some(artifact);
            self.set_status("Copied handoff text");
            self.pending_g = false;
            return;
        }
        if !self.ensure_session_details_ready("Yank handoff") {
            return;
        }
        self.ensure_launch_handoff_skill_default();
        self.pending_share_handoff_copy = true;
        self.show_share_panel = false;
        self.pending_target = self.data.target;
        self.confirm_launch_target();
    }

    fn copy_share_portable_json(&mut self) {
        if !self.ensure_session_details_ready("Yank portable JSON") {
            return;
        }
        let Some(session) = self.current_session() else {
            self.set_status("No session selected");
            self.pending_g = false;
            return;
        };
        let payload = self.compact_portable_json(session);
        let Ok(text) = serde_json::to_string(&payload) else {
            self.set_status("Portable JSON failed to serialize");
            self.pending_g = false;
            return;
        };
        if text.len() > SHARE_PORTABLE_JSON_CLIPBOARD_LIMIT_BYTES {
            self.set_status(format!(
                "Portable JSON is {}KB; use file export when M113 lands",
                text.len() / 1024
            ));
            self.pending_g = false;
            return;
        }
        self.clipboard_text = Some(text);
        self.set_status("Copied portable JSON");
        self.pending_g = false;
    }

    fn first_user_input(&self) -> Option<&str> {
        self.first_user_event()
            .and_then(|event| (!event.detail.trim().is_empty()).then_some(event.detail.trim()))
    }

    fn first_user_event(&self) -> Option<&TimelineEvent> {
        if !self.selected_session_timeline_loaded() {
            return None;
        }
        self.data.timeline.iter().find(|event| {
            event.kind == TimelineKind::User
                && (!event.detail.trim().is_empty() || !event.metadata.attachments.is_empty())
        })
    }

    fn first_user_seed_prompt(&self) -> Option<(String, usize, usize)> {
        let event = self.first_user_event()?;
        let mut prompt = event.detail.trim().to_string();
        let (attachment_lines, path_count, missing_path_count) =
            seed_prompt_attachment_lines(&event.metadata.attachments);
        if !attachment_lines.is_empty() {
            if !prompt.is_empty() {
                prompt.push_str("\n\n");
            }
            prompt.push_str("Original first user message attachment path references:\n");
            prompt.push_str(
                "Use these local paths only if they are accessible in this target workspace.\n",
            );
            prompt.push_str(&attachment_lines.join("\n"));
        }
        (!prompt.trim().is_empty()).then_some((prompt, path_count, missing_path_count))
    }

    fn last_ai_output(&self) -> Option<&str> {
        if !self.selected_session_timeline_loaded() {
            return None;
        }
        self.data
            .timeline
            .iter()
            .rev()
            .find(|event| event.kind == TimelineKind::Assistant && !event.detail.trim().is_empty())
            .map(|event| event.detail.trim())
    }

    fn selected_handoff_artifact(&self) -> Option<&str> {
        let session = self.current_session()?;
        if self.data.capsule.source_session != session.id
            || self.data.capsule.source_cli != session.cli
        {
            return None;
        }
        self.data.capsule.handoff_artifact.as_deref()
    }

    fn compact_portable_json(&self, session: &SessionSummary) -> serde_json::Value {
        let exported_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or(0);
        let timeline_events = self
            .data
            .timeline
            .iter()
            .map(|event| {
                serde_json::json!({
                    "id": event.id,
                    "time": event.time,
                    "kind": event.kind,
                    "title": event.title,
                    "detail": event.detail,
                })
            })
            .collect::<Vec<_>>();
        serde_json::json!({
            "schema": "moonbox.portable_session.v1",
            "moonbox_version": env!("CARGO_PKG_VERSION"),
            "exported_at_unix": exported_at,
            "source": {
                "cli": session.cli,
                "session_id": session.id,
                "title": session.title,
                "cwd": session.cwd,
                "branch": session.branch,
                "updated_at": session.updated_at,
            },
            "handoff": {
                "target_cli": self.pending_target,
                "compiler": self.data.capsule.compiler,
                "runner": self.data.capsule.handoff_runner,
                "skill": self.data.capsule.handoff_skill,
                "artifact": self.selected_handoff_artifact(),
            },
            "last_ai_output": self.last_ai_output(),
            "timeline": {
                "loaded": self.selected_session_timeline_loaded(),
                "event_count": timeline_events.len(),
                "events": timeline_events,
            },
        })
    }

    fn copy_target_launch_result_command(&mut self) {
        let Some(result) = &self.target_launch_result else {
            self.set_status("No launch result");
            return;
        };
        self.copy_text("launch", result.command.clone());
    }

    fn rerun_target_handoff(&mut self) {
        let Some(result) = &self.target_launch_result else {
            self.set_status("No launch result");
            return;
        };
        self.pending_launch = Some(result.plan.clone());
        self.set_status(format!("Launching target: {}", result.target));
    }

    fn copy_original_command(&mut self) {
        if !self.current_data_space().is_local() {
            self.set_status("SSH sessions cannot be opened locally; use handoff");
            return;
        }
        if let Some(command) = self.original_open_command() {
            self.copy_text("original", command);
        } else {
            self.set_status("No session selected");
        }
    }

    fn queue_target_handoff(&mut self) {
        if self.launch_requires_handoff_skill(self.pending_target) {
            self.set_status("Choose an AI handoff skill before running");
            return;
        }
        let validation = self.validate_launch_for_target(self.pending_target);
        if validation.is_blocked() {
            if launch_validation_can_regenerate_handoff(&validation) {
                self.set_status("Regenerate handoff with Enter before running");
            } else {
                self.set_status(format!("Target blocked: {}", validation.summary()));
            }
            return;
        }
        let Some(session) = self.current_session().cloned() else {
            self.set_status("No session selected");
            return;
        };
        let capsule = self.launch_capsule_for_target(self.pending_target);
        if compiler::compiler_is_builtin(&capsule.compiler)
            && session.source_provenance != SourceProvenance::Fixture
        {
            self.set_status("Draft handoff cannot run; press y to copy or choose an AI skill");
            return;
        }
        let continuation = continuation::build_continuation_protocol(
            &session,
            self.pending_target,
            &capsule,
            None,
            ContinuationOptions::default(),
        );
        let target_command = match launcher::target_command_with_continuation(
            self.pending_target,
            &session,
            &capsule,
            &continuation,
        ) {
            Ok(command) => command,
            Err(error) => {
                self.set_status(format!("Target failed: {error}"));
                return;
            }
        };
        let verification = verifier::verify_capsule_with_continuation(
            &capsule,
            &session,
            &self.data.timeline,
            self.pending_target,
            &continuation,
        );
        let command = target_command.display.clone();
        let compiler = capsule.compiler.clone();
        let handoff_label = capsule.handoff_label.clone();
        let rewind_point = capsule.rewind_point.clone();
        self.pending_launch = Some(Box::new(LaunchPlan {
            version: 1,
            action: SessionAction::TargetHandoff,
            dry_run: true,
            source_session: session,
            target_cli: self.pending_target,
            compiler,
            handoff_label,
            rewind_point,
            capsule_path: None,
            command,
            target_command,
            verification,
            continuation,
        }));
        self.set_status(format!("Launching target: {}", self.pending_target));
    }

    fn copy_doctor_report(&mut self) {
        match serde_json::to_string_pretty(&self.doctor_report) {
            Ok(report) => {
                self.clipboard_text = Some(report);
                self.set_status("Copied pre-flight doctor JSON");
            }
            Err(error) => self.set_status(format!("Doctor copy failed: {error}")),
        }
    }

    pub fn launch_command(&self) -> String {
        let session = self
            .current_session()
            .map(|session| session.id.as_str())
            .unwrap_or("no-session");
        workbench::moonbox_execute_command(self.pending_target, session, None)
    }

    pub fn launch_copy_command(&self) -> String {
        self.target_launch_command_display()
            .unwrap_or_else(|| self.launch_command())
    }

    pub fn launch_handoff_label(&self) -> String {
        format!(
            "moonbox/{}-rewind-{}",
            self.pending_target.id(),
            self.rewind_event_id
        )
    }

    pub fn original_open_command(&self) -> Option<String> {
        if !self.current_data_space().is_local() {
            return None;
        }
        self.current_session()
            .map(|session| workbench::moonbox_open_execute_command(&session.id))
    }

    pub fn original_resume_display_command(&self) -> Option<String> {
        if !self.current_data_space().is_local() {
            return None;
        }
        self.current_session()
            .map(|session| launcher::original_command(session).display)
    }

    pub fn target_launch_command_display(&self) -> Option<String> {
        let session = self.current_session()?;
        let capsule = self.launch_capsule_for_target(self.pending_target);
        let continuation = continuation::build_continuation_protocol(
            session,
            self.pending_target,
            &capsule,
            None,
            ContinuationOptions::default(),
        );
        launcher::target_command_with_continuation(
            self.pending_target,
            session,
            &capsule,
            &continuation,
        )
        .ok()
        .map(|command| command.display)
    }

    pub fn target_launch_command_summary(&self) -> Option<String> {
        let session = self.current_session()?;
        let capsule = self.launch_capsule_for_target(self.pending_target);
        let continuation = continuation::build_continuation_protocol(
            session,
            self.pending_target,
            &capsule,
            None,
            ContinuationOptions::default(),
        );
        launcher::target_command_with_continuation(
            self.pending_target,
            session,
            &capsule,
            &continuation,
        )
        .ok()
        .map(|command| launcher::concise_command_display(&command))
    }

    pub fn target_command_preview(&self) -> Option<launcher::TargetInputPreview> {
        let session = self.current_session()?;
        let capsule = self.launch_capsule_for_target(self.pending_target);
        let continuation = continuation::build_continuation_protocol(
            session,
            self.pending_target,
            &capsule,
            None,
            ContinuationOptions::default(),
        );
        let command = launcher::target_command_with_continuation(
            self.pending_target,
            session,
            &capsule,
            &continuation,
        )
        .ok()?;
        Some(launcher::TargetInputPreview {
            program: command.program,
            args: command.args,
            cwd: command.cwd,
            prompt: launcher::target_prompt_preview_with_continuation(
                session,
                &capsule,
                &continuation,
            ),
        })
    }

    pub fn validate_launch_for_target(&self, target: CliTool) -> LaunchValidation {
        if self.is_session_load_pending() || !self.selected_session_context_loaded() {
            return LaunchValidation::warning(vec![
                "selected session context loads when review starts".into(),
            ]);
        }
        let Some(report) = self.launch_verification_for_target(target) else {
            return LaunchValidation::blocked(vec!["No session selected".into()]);
        };
        verifier::validation_from_report(&report)
    }

    pub fn launch_verification_for_target(&self, target: CliTool) -> Option<VerificationReport> {
        if !self.selected_session_context_loaded() {
            return None;
        }
        let session = self.current_session()?;
        let capsule = self.launch_capsule_for_target(target);
        let continuation = continuation::build_continuation_protocol(
            session,
            target,
            &capsule,
            None,
            ContinuationOptions::default(),
        );
        Some(verifier::verify_capsule_with_continuation(
            &capsule,
            session,
            &self.data.timeline,
            target,
            &continuation,
        ))
    }

    pub fn launch_requires_handoff_skill(&self, target: CliTool) -> bool {
        let Some(session) = self.current_session() else {
            return false;
        };
        session.source_provenance != SourceProvenance::Fixture
            && compiler::compiler_is_builtin(&self.launch_capsule_for_target(target).compiler)
    }

    pub(crate) fn launch_capsule_for_target(&self, target: CliTool) -> WorkCapsule {
        let mut capsule = self.data.capsule.clone();
        if let Some(selected_compiler) = self.data.compilers.get(self.selected_compiler) {
            capsule.compiler = selected_compiler.clone();
        }
        capsule.target_cli = target;
        capsule.handoff_label = format!("moonbox/{}-rewind-{}", target.id(), self.rewind_event_id);
        capsule
    }

    fn apply_rewind_event(&mut self, id: String, title: String) {
        self.rewind_event_id = id.clone();
        self.data.capsule.rewind_point = format!("{id} / {title}");
        self.data.capsule.handoff_label = format!("moonbox/{}-rewind-{id}", self.data.target.id());
    }

    fn timeline_event_title(&self, id: &str) -> Option<String> {
        self.data
            .timeline
            .iter()
            .find(|event| event.id == id)
            .map(|event| event.title.clone())
    }
}

fn initial_rewind_event_id(data: &WorkbenchData) -> String {
    data.capsule
        .rewind_point
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_string()
}

fn rewind_event_index(data: &WorkbenchData, rewind_event_id: &str) -> usize {
    data.timeline
        .iter()
        .position(|event| event.id == rewind_event_id)
        .unwrap_or_else(|| data.timeline.len().saturating_sub(1))
}

fn compiler_index_for_id(data: &WorkbenchData, compiler_id: &str, fallback: usize) -> usize {
    data.compilers
        .iter()
        .position(|candidate| candidate == compiler_id)
        .unwrap_or(fallback)
        .min(data.compilers.len().saturating_sub(1))
}

fn compiler_index_for_equivalent_id(
    data: &WorkbenchData,
    compiler_id: &str,
    target: CliTool,
    fallback: usize,
) -> usize {
    if let Some(index) = data
        .compilers
        .iter()
        .position(|candidate| candidate == compiler_id)
    {
        return index;
    }
    let Some(selected_spec) = handoff::parse_compiler_id(compiler_id) else {
        return fallback.min(data.compilers.len().saturating_sub(1));
    };
    if let Some(target_runner_id) = target_runner_id(target)
        && let Some(index) = data.compilers.iter().position(|candidate| {
            handoff::parse_compiler_id(candidate).is_some_and(|candidate_spec| {
                candidate_spec.skill_id == selected_spec.skill_id
                    && candidate_spec.runner.id() == target_runner_id
            })
        })
    {
        return index;
    }
    data.compilers
        .iter()
        .position(|candidate| {
            handoff::parse_compiler_id(candidate)
                .is_some_and(|candidate_spec| candidate_spec.skill_id == selected_spec.skill_id)
        })
        .unwrap_or(fallback)
        .min(data.compilers.len().saturating_sub(1))
}

fn launch_validation_can_regenerate_handoff(validation: &LaunchValidation) -> bool {
    validation.is_blocked()
        && !validation.reasons.is_empty()
        && validation
            .reasons
            .iter()
            .all(|reason| is_stale_handoff_compiler_mismatch(reason))
}

fn seed_prompt_attachment_lines(attachments: &[TimelineAttachment]) -> (Vec<String>, usize, usize) {
    let mut path_count = 0usize;
    let mut missing_path_count = 0usize;
    let lines = attachments
        .iter()
        .enumerate()
        .map(|(index, attachment)| {
            let label = attachment
                .name
                .as_deref()
                .or(attachment.id.as_deref())
                .unwrap_or("attachment");
            match attachment
                .path
                .as_deref()
                .map(str::trim)
                .filter(|path| !path.is_empty())
            {
                Some(path) => {
                    path_count += 1;
                    format!("- {label}: {path}")
                }
                None => {
                    missing_path_count += 1;
                    if label == "attachment" {
                        format!(
                            "- Attachment #{}: path unavailable in source metadata",
                            index + 1
                        )
                    } else {
                        format!("- {label}: path unavailable in source metadata")
                    }
                }
            }
        })
        .collect::<Vec<_>>();
    (lines, path_count, missing_path_count)
}

fn seed_prompt_attachment_note(path_count: usize, missing_path_count: usize) -> String {
    match (path_count, missing_path_count) {
        (0, 0) => String::new(),
        (paths, 0) => format!(" ({paths} attachment path{})", plural_suffix(paths)),
        (0, missing) => format!(
            " ({missing} attachment{} without path)",
            plural_suffix(missing)
        ),
        (paths, missing) => format!(
            " ({paths} path{}, {missing} attachment{} without path)",
            plural_suffix(paths),
            plural_suffix(missing)
        ),
    }
}

fn plural_suffix(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

fn setup_install_plan_for_compiler_info(
    info: &crate::core::model::CompilerPresetInfo,
    scope: SetupInstallScope,
) -> Option<SetupInstallPlan> {
    setup_install_plan_for_compiler_error_with_scope(&info.id, &info.reason, scope)
}

fn setup_install_plan_for_compiler_error(
    compiler_id: &str,
    message: &str,
) -> Option<SetupInstallPlan> {
    setup_install_plan_for_compiler_error_with_scope(
        compiler_id,
        message,
        SetupInstallScope::Launch,
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SetupInstallScope {
    Skill,
    Launch,
}

fn setup_install_plan_for_compiler_error_with_scope(
    compiler_id: &str,
    message: &str,
    scope: SetupInstallScope,
) -> Option<SetupInstallPlan> {
    let spec = handoff::parse_compiler_id(compiler_id)?;
    let target = if message.contains("skill_not_installed") {
        setup::SetupInstallTarget::MattHandoff
    } else if scope == SetupInstallScope::Launch && message.contains("sdk_not_found:") {
        match spec.runner {
            handoff::AgentRunner::Codex => setup::SetupInstallTarget::CodexSdk,
            handoff::AgentRunner::Claude => setup::SetupInstallTarget::ClaudeSdk,
        }
    } else {
        return None;
    };
    Some(SetupInstallPlan {
        target,
        label: match target {
            setup::SetupInstallTarget::CodexSdk => "Install Codex SDK".into(),
            setup::SetupInstallTarget::ClaudeSdk => "Install Claude SDK".into(),
            setup::SetupInstallTarget::MattHandoff => "Install matt-handoff".into(),
            setup::SetupInstallTarget::LarkCli => "Install lark-cli".into(),
        },
        command_display: setup::setup_command_display_for_current_exe(target),
        compiler_id: Some(compiler_id.to_string()),
    })
}

fn is_stale_handoff_compiler_mismatch(reason: &str) -> bool {
    reason.contains("raw source map mismatch")
        && reason.contains("generated_by ")
        && reason.contains(" vs compiler ")
}

fn timeline_event_is_visible(data: &WorkbenchData, rewind_event_id: &str, index: usize) -> bool {
    data.timeline
        .get(index)
        .is_some_and(|event| event.id == rewind_event_id || event.kind != TimelineKind::Tool)
}

fn timeline_event_is_rewind_anchor(event: &crate::core::model::TimelineEvent) -> bool {
    matches!(event.kind, TimelineKind::User | TimelineKind::RewindPoint)
}

fn session_overlay_key(session: &SessionSummary) -> String {
    format!("{}:{}", session.cli.id(), session.id)
}

fn first_visible_timeline_event(data: &WorkbenchData, rewind_event_id: &str) -> usize {
    visible_timeline_group_heads(data, rewind_event_id)
        .first()
        .copied()
        .unwrap_or(0)
}

fn last_visible_timeline_event(data: &WorkbenchData, rewind_event_id: &str) -> usize {
    visible_timeline_group_heads(data, rewind_event_id)
        .last()
        .copied()
        .unwrap_or_else(|| data.timeline.len().saturating_sub(1))
}

fn nearest_visible_timeline_event(
    data: &WorkbenchData,
    rewind_event_id: &str,
    selected_event: usize,
) -> usize {
    if timeline_event_is_visible(data, rewind_event_id, selected_event) {
        return selected_event;
    }
    (selected_event.saturating_add(1)..data.timeline.len())
        .find(|index| timeline_event_is_visible(data, rewind_event_id, *index))
        .or_else(|| {
            (0..selected_event)
                .rev()
                .find(|index| timeline_event_is_visible(data, rewind_event_id, *index))
        })
        .unwrap_or_else(|| selected_event.min(data.timeline.len().saturating_sub(1)))
}

fn next_visible_timeline_event(
    data: &WorkbenchData,
    rewind_event_id: &str,
    selected_event: usize,
) -> usize {
    let group_heads = visible_timeline_group_heads(data, rewind_event_id);
    let current = selected_visible_timeline_group_position(
        data,
        rewind_event_id,
        selected_event,
        &group_heads,
    );
    group_heads
        .get(current.saturating_add(1))
        .copied()
        .unwrap_or_else(|| nearest_visible_timeline_event(data, rewind_event_id, selected_event))
}

fn previous_visible_timeline_event(
    data: &WorkbenchData,
    rewind_event_id: &str,
    selected_event: usize,
) -> usize {
    let group_heads = visible_timeline_group_heads(data, rewind_event_id);
    let current = selected_visible_timeline_group_position(
        data,
        rewind_event_id,
        selected_event,
        &group_heads,
    );
    current
        .checked_sub(1)
        .and_then(|index| group_heads.get(index))
        .copied()
        .unwrap_or_else(|| nearest_visible_timeline_event(data, rewind_event_id, selected_event))
}

fn visible_timeline_group_heads(data: &WorkbenchData, rewind_event_id: &str) -> Vec<usize> {
    let mut heads = Vec::new();
    let mut previous_kind = None;
    for (index, event) in data.timeline.iter().enumerate() {
        if !timeline_event_is_visible(data, rewind_event_id, index) {
            continue;
        }
        let continues_ai_group =
            event.kind == TimelineKind::Assistant && previous_kind == Some(TimelineKind::Assistant);
        if !continues_ai_group {
            heads.push(index);
        }
        previous_kind = Some(event.kind);
    }
    heads
}

fn selected_visible_timeline_group_position(
    data: &WorkbenchData,
    rewind_event_id: &str,
    selected_event: usize,
    group_heads: &[usize],
) -> usize {
    if group_heads.is_empty() {
        return 0;
    }
    let visible_event = nearest_visible_timeline_event(data, rewind_event_id, selected_event);
    group_heads
        .iter()
        .enumerate()
        .rev()
        .find(|(_, head)| **head <= visible_event)
        .map(|(position, _)| position)
        .unwrap_or(0)
}

fn session_matches_query(session: &SessionSummary, query: &str) -> bool {
    query.is_empty()
        || session.id.to_ascii_lowercase().contains(query)
        || session.title.to_ascii_lowercase().contains(query)
        || session.cwd.to_ascii_lowercase().contains(query)
        || session
            .source_path
            .as_ref()
            .is_some_and(|path| path.to_ascii_lowercase().contains(query))
        || session.cli.id().contains(query)
        || session
            .branch
            .as_ref()
            .is_some_and(|branch| branch.to_ascii_lowercase().contains(query))
        || session
            .health_reason
            .as_ref()
            .is_some_and(|reason| reason.to_ascii_lowercase().contains(query))
}

fn is_moonbox_handoff_worker_session(session: &SessionSummary) -> bool {
    let title = session.title.to_ascii_lowercase();
    crate::core::local_jsonl::is_provider_context_text(&session.title)
        || crate::core::local_jsonl::is_moonbox_handoff_control_text(&session.title)
        || title.starts_with("$handoff ")
        || title.contains("you are running a moonbox continuation handoff job")
        || title.contains("moonbox continuation handoff job")
        || title.contains("the following is the codex agent history whose request action")
        || title.contains("<selected_skill")
        || title.contains("transcript start")
}

fn query_explicitly_requests_moonbox_handoff_workers(query: &str) -> bool {
    !query.is_empty()
        && (query.contains("$handoff")
            || query.contains("moonbox continuation handoff")
            || query.contains("moonbox handoff worker"))
}

fn original_resume_mode_from_env() -> OriginalResumeMode {
    parse_original_resume_mode(env::var("MOONBOX_RESUME_MODE").ok().as_deref())
}

fn share_action_label(kind: SharePanelActionKind) -> &'static str {
    match kind {
        SharePanelActionKind::FirstUserInput => "First user input",
        SharePanelActionKind::LastAiOutput => "Last AI output",
        SharePanelActionKind::SessionId => "Session ID",
        SharePanelActionKind::HandoffContent => "Handoff content",
        SharePanelActionKind::PortableJson => "Portable JSON",
    }
}

fn lark_export_command_display(
    session_id: &str,
    target: &str,
    rewind: &str,
    compiler: &str,
) -> String {
    format!(
        "moon export --session {} --target {} --rewind {} --compiler {} --to lark --mode handoff --execute",
        shellish_quote(session_id),
        shellish_quote(target),
        shellish_quote(rewind),
        shellish_quote(compiler)
    )
}

fn shellish_quote(value: &str) -> String {
    if value.bytes().all(|byte| {
        byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b'/' | b':' | b'=')
    }) {
        return value.into();
    }
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn parse_original_resume_mode(value: Option<&str>) -> OriginalResumeMode {
    if value.is_some_and(|value| value.trim().eq_ignore_ascii_case("exec")) {
        OriginalResumeMode::Exec
    } else {
        OriginalResumeMode::Suspend
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{data, image_preview::ImagePreviewStatus};

    fn key(ch: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(ch), KeyModifiers::empty())
    }

    fn new_app(source: CliTool, target: CliTool) -> App {
        let mut app = App::new(source, target).expect("app");
        app.starred_sessions.clear();
        app.archived_sessions.clear();
        app.refresh_visible_sessions();
        app
    }

    fn readonly_startup_app() -> App {
        let fixture = App::new(CliTool::Codex, CliTool::Hermes).expect("fixture app");
        let mut session = fixture
            .data
            .sessions
            .iter()
            .find(|session| session.cli == CliTool::Codex)
            .cloned()
            .expect("codex fixture session");
        session.source_provenance = SourceProvenance::Real;
        session.source_path = Some("/tmp/moonbox-readonly-startup.jsonl".into());
        let workbench = data::workbench_data_from_readonly_inventory(
            session.clone(),
            vec![session],
            fixture.data.source_adapters,
            CliTool::Hermes,
        );
        App::from_data(workbench, CliTool::Hermes)
    }

    fn enable_hook_config(app: &mut App, smart_enter_tmux: bool) {
        app.set_hooks_config_for_test(config::HooksConfig {
            enabled: true,
            smart_enter_tmux,
            ..config::HooksConfig::default()
        });
    }

    fn hook_event_for_session(
        session: &SessionSummary,
        kind: hooks::HookEventKind,
        tmux: Option<&str>,
        tmux_pane: Option<&str>,
    ) -> hooks::HookSpoolEvent {
        hooks::HookSpoolEvent {
            cli: session.cli,
            session_id: session.id.clone(),
            transcript_path: session.source_path.clone(),
            cwd: Some("/repo".into()),
            tmux: tmux.map(str::to_owned),
            tmux_pane: tmux_pane.map(str::to_owned),
            captured_at_ms: hooks::current_millis(),
            event_name: format!("{kind:?}"),
            kind,
            summary: "Edit src/app.rs".into(),
            wait_reason: None,
        }
    }

    fn wait_for_background(app: &mut App, mut done: impl FnMut(&App) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline {
            app.poll_background();
            if done(app) {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        app.poll_background();
    }

    fn settle_session_load(app: &mut App) {
        wait_for_background(app, |app| !app.is_session_load_pending());
        assert!(
            !app.is_session_load_pending(),
            "session load did not finish"
        );
    }

    fn settle_session_preview(app: &mut App) {
        wait_for_background(app, |app| !app.is_session_preview_pending());
        assert!(
            !app.is_session_preview_pending(),
            "session preview did not finish"
        );
    }

    fn settle_data_space_load(app: &mut App) {
        wait_for_background(app, |app| app.pending_data_space_load.is_none());
        assert!(
            app.pending_data_space_load.is_none(),
            "data space load did not finish"
        );
    }

    fn settle_launch_review(app: &mut App) {
        wait_for_background(app, |app| !app.is_launch_review_pending());
        assert!(
            !app.is_launch_review_pending(),
            "launch review did not finish"
        );
    }

    fn seed_ready_handoff_artifact(app: &mut App, artifact: &str) {
        let session = app.current_session().expect("session").clone();
        let compiler_id = "agent:codex:moonbox-handoff".to_string();
        if !app
            .data
            .compilers
            .iter()
            .any(|compiler| compiler == &compiler_id)
        {
            app.data.compilers.insert(0, compiler_id.clone());
        }
        app.selected_compiler = compiler_index_for_id(&app.data, &compiler_id, 0);
        app.data.capsule.source_cli = session.cli;
        app.data.capsule.source_session = session.id;
        app.data.capsule.target_cli = app.data.target;
        app.data.capsule.compiler = compiler_id;
        app.data.capsule.rewind_point = format!("{} / reviewed", app.rewind_event_id);
        app.data.capsule.handoff_label = format!(
            "moonbox/{}-rewind-{}",
            app.data.target.id(),
            app.rewind_event_id
        );
        app.data.capsule.handoff_artifact = Some(artifact.into());
        app.data.capsule.handoff_artifact_path = None;
        app.data.capsule.handoff_runner = Some("Codex".into());
        app.data.capsule.handoff_skill = Some("handoff".into());
    }

    #[test]
    fn readonly_startup_app_does_not_preload_session_timeline() {
        let app = readonly_startup_app();

        assert!(app.data.timeline.is_empty());
        assert_eq!(app.data.capsule.state, "pending_rewind");
        assert_eq!(app.compile_status, "PENDING");
        assert!(!app.verify_passed);
        assert_eq!(app.status_message, "Ready: session details load on demand");
        assert!(!app.is_session_load_pending());
        assert!(app.is_session_preview_pending());
    }

    #[test]
    fn readonly_startup_details_load_on_first_timeline_action() {
        let mut app = readonly_startup_app();

        assert!(!app.ensure_session_details_ready("Review"));
        assert!(app.is_session_load_pending());
        assert_eq!(
            app.status_message,
            "Review is loading selected session details"
        );

        settle_session_load(&mut app);

        assert!(!app.data.timeline.is_empty());
        assert_eq!(app.compile_status, "PREVIEW");
        assert!(!app.verify_passed);
        assert!(app.selected_session_timeline_loaded());
        assert!(!app.selected_session_context_loaded());
    }

    #[test]
    fn readonly_startup_preview_loads_timeline_without_compiling_handoff() {
        let mut app = readonly_startup_app();

        settle_session_preview(&mut app);

        assert!(app.selected_session_timeline_loaded());
        assert!(!app.selected_session_context_loaded());
        assert_eq!(app.compile_status, "PREVIEW");
        assert!(!app.verify_passed);
        assert!(
            app.status_message.starts_with("Timeline preview: "),
            "{}",
            app.status_message
        );
    }

    #[test]
    fn space_updates_rewind_point_from_selected_event() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        app.selected_event = 0;
        app.handle_key(key(' '));
        assert_eq!(app.rewind_event_id, "evt-001");
        assert!(app.data.capsule.rewind_point.contains("evt-001"));
        assert!(app.data.capsule.handoff_label.contains("evt-001"));
    }

    #[test]
    fn timeline_navigation_skips_hidden_tool_events() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        app.focus = Focus::Timeline;
        app.selected_event = 0;

        app.handle_key(key('j'));
        assert_eq!(app.selected_event, 2);

        app.handle_key(key('k'));
        assert_eq!(app.selected_event, 0);
    }

    #[test]
    fn timeline_navigation_moves_by_visible_ai_groups() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        app.focus = Focus::Timeline;
        app.data.timeline = vec![
            crate::core::model::TimelineEvent {
                id: "evt-001".into(),
                time: "10:00".into(),
                kind: TimelineKind::User,
                title: "User".into(),
                detail: "分析下 cxcp".into(),
                metadata: Default::default(),
            },
            crate::core::model::TimelineEvent {
                id: "evt-002".into(),
                time: "10:01".into(),
                kind: TimelineKind::Assistant,
                title: "Assistant".into(),
                detail: "先定位项目。".into(),
                metadata: Default::default(),
            },
            crate::core::model::TimelineEvent {
                id: "evt-003".into(),
                time: "10:02".into(),
                kind: TimelineKind::Assistant,
                title: "Assistant".into(),
                detail: "继续分析缓存。".into(),
                metadata: Default::default(),
            },
            crate::core::model::TimelineEvent {
                id: "evt-004".into(),
                time: "10:03".into(),
                kind: TimelineKind::User,
                title: "User".into(),
                detail: "下一步".into(),
                metadata: Default::default(),
            },
        ];
        app.selected_event = 0;
        app.rewind_event_id = "evt-001".into();

        app.handle_key(key('j'));
        assert_eq!(app.selected_event, 1);

        app.handle_key(key('j'));
        assert_eq!(app.selected_event, 3);

        app.selected_event = 2;
        app.handle_key(key('j'));
        assert_eq!(app.selected_event, 3);

        app.handle_key(key('k'));
        assert_eq!(app.selected_event, 1);
    }

    #[test]
    fn rewind_selection_from_non_user_event_is_rejected() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        let original_rewind = app.rewind_event_id.clone();
        app.selected_event = 2;

        app.handle_key(key(' '));

        assert_eq!(app.rewind_event_id, original_rewind);
        assert_eq!(app.status_message, "Rewind anchor must be a User turn");
    }

    #[test]
    fn skill_picker_applies_selected_compiler() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        app.handle_key(key('S'));
        app.data.compilers = vec!["custom-handoff-a".into(), "custom-handoff-b".into()];
        app.selected_compiler = 0;
        app.pending_compiler = 0;
        app.data.capsule.compiler = "custom-handoff-a".into();
        let first = app.data.capsule.compiler.clone();
        assert!(app.show_skill_picker);
        assert_eq!(app.data.capsule.compiler, first);
        app.pending_compiler = compiler_index_for_id(&app.data, "custom-handoff-b", 0);
        app.handle_key(KeyEvent::from(KeyCode::Enter));
        assert_ne!(app.data.capsule.compiler, first);
        assert_eq!(app.data.capsule.compiler, "custom-handoff-b");
        assert!(!app.show_skill_picker);
    }

    #[test]
    fn skill_picker_collapses_agent_runners_into_one_skill_choice() {
        let mut app = new_app(CliTool::Codex, CliTool::Claude);
        app.data.target = CliTool::Claude;
        app.data.compilers = vec![
            "agent:codex:handoff".into(),
            "agent:claude:handoff".into(),
            "engineering-handoff".into(),
        ];
        app.compiler_catalog.clear();
        app.selected_compiler = 0;
        app.pending_compiler = 1;
        app.data.capsule.compiler = "agent:codex:handoff".into();

        let candidates = app.skill_picker_candidate_indices();

        assert_eq!(candidates, vec![1]);
        assert!(app.compiler_selection_matches(candidates[0], app.selected_compiler));

        app.show_skill_picker = true;
        assert_eq!(
            app.data.compilers[app.pending_compiler],
            "agent:claude:handoff"
        );
        app.handle_key(KeyEvent::from(KeyCode::Enter));

        assert_eq!(app.data.capsule.compiler, "agent:claude:handoff");
        assert_eq!(app.status_message, "Handoff skill: handoff");
    }

    #[test]
    fn zoom_shortcuts_expand_restore_and_follow_focus() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        app.focus = Focus::Timeline;
        app.selected_event = 2;

        app.handle_key(key('+'));

        assert_eq!(app.zoomed_focus, Some(Focus::Timeline));
        assert_eq!(app.selected_event, 2);
        assert_eq!(app.status_message, "Zoomed Timeline");

        app.handle_key(KeyEvent::from(KeyCode::Tab));

        assert_eq!(app.focus, Focus::Capsule);
        assert_eq!(app.zoomed_focus, Some(Focus::Capsule));

        app.handle_key(key('-'));

        assert_eq!(app.zoomed_focus, None);
        assert_eq!(app.status_message, "Zoom restored");
    }

    #[test]
    fn data_space_shortcut_loads_selected_inventory() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        app.data_spaces = vec![
            dataspace::DataSpaceEntry::local(),
            dataspace::DataSpaceEntry {
                id: "local-devbox".into(),
                label: "Devbox".into(),
                kind: dataspace::DataSpaceKind::Local,
                detail: "fixture local data space".into(),
                ssh_host: None,
                ssh_user: None,
                ssh_port: None,
                ssh_identity_file: None,
                config_source: Some("test".into()),
                config_path: None,
            },
        ];

        app.handle_key(key('}'));
        settle_data_space_load(&mut app);

        assert_eq!(app.selected_data_space, 1);
        assert!(app.status_message.contains("Data space: Devbox"));
        assert!(!app.should_quit());
    }

    #[test]
    fn data_space_picker_opens_and_switches_selected_inventory() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        app.data_spaces = vec![
            dataspace::DataSpaceEntry::local(),
            dataspace::DataSpaceEntry {
                id: "local-review".into(),
                label: "Review".into(),
                kind: dataspace::DataSpaceKind::Local,
                detail: "fixture local review space".into(),
                ssh_host: None,
                ssh_user: None,
                ssh_port: None,
                ssh_identity_file: None,
                config_source: Some("test".into()),
                config_path: None,
            },
        ];

        app.handle_key(key('d'));
        assert!(app.show_data_spaces);
        assert_eq!(app.data_space_selection, 0);

        app.handle_key(key('j'));
        assert_eq!(app.data_space_selection, 1);

        app.handle_key(KeyEvent::from(KeyCode::Enter));
        assert!(!app.show_data_spaces);
        settle_data_space_load(&mut app);

        assert_eq!(app.selected_data_space, 1);
        assert!(app.status_message.contains("Data space: Review"));
    }

    #[test]
    fn data_space_picker_opens_add_ssh_config_form() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);

        app.handle_key(key('d'));
        app.handle_key(key('n'));

        assert!(app.show_data_spaces);
        assert!(app.show_data_space_config);
        assert_eq!(app.status_message, "Add SSH data space");
    }

    #[test]
    fn data_space_config_form_validates_and_builds_host() {
        let mut form = DataSpaceConfigForm {
            quick: String::new(),
            name: "devbox".into(),
            host: "10.37.218.31".into(),
            user: "yangyang.1205".into(),
            port: "22".into(),
            identity_file: "~/.ssh/id_ed25519".into(),
        };

        let host = form.to_config().expect("host config");
        assert_eq!(host.name, "devbox");
        assert_eq!(host.host, "10.37.218.31");
        assert_eq!(host.user.as_deref(), Some("yangyang.1205"));
        assert_eq!(host.port, Some(22));
        assert_eq!(host.identity_file.as_deref(), Some("~/.ssh/id_ed25519"));

        form.port = "nope".into();
        assert_eq!(
            form.to_config().expect_err("bad port"),
            "port must be 1-65535"
        );
    }

    #[test]
    fn data_space_config_quick_input_parses_ssh_command() {
        let mut form = DataSpaceConfigForm {
            quick: "ssh -i ~/.ssh/id_ed25519 -p 2222 yangyang.1205@10.37.218.31".into(),
            ..DataSpaceConfigForm::default()
        };

        assert!(form.parse_quick_into_fields().expect("parse quick ssh"));
        let host = form.to_config().expect("host config");

        assert_eq!(host.name, "10.37.218.31");
        assert_eq!(host.host, "10.37.218.31");
        assert_eq!(host.user.as_deref(), Some("yangyang.1205"));
        assert_eq!(host.port, Some(2222));
        assert_eq!(host.identity_file.as_deref(), Some("~/.ssh/id_ed25519"));
    }

    #[test]
    fn data_space_config_quick_input_parses_openssh_host_block() {
        let mut form = DataSpaceConfigForm {
            quick: r#"
Host devbox
  HostName 10.37.218.31
  User yangyang.1205
  Port 22
  IdentityFile ~/.ssh/id_ed25519
"#
            .into(),
            ..DataSpaceConfigForm::default()
        };

        assert!(form.parse_quick_into_fields().expect("parse openssh"));
        let host = form.to_config().expect("host config");

        assert_eq!(host.name, "devbox");
        assert_eq!(host.host, "10.37.218.31");
        assert_eq!(host.user.as_deref(), Some("yangyang.1205"));
        assert_eq!(host.port, Some(22));
        assert_eq!(host.identity_file.as_deref(), Some("~/.ssh/id_ed25519"));
    }

    #[test]
    fn data_space_picker_delete_requires_second_keypress() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        app.data_spaces = vec![
            dataspace::DataSpaceEntry::local(),
            dataspace::DataSpaceEntry {
                id: "ssh:devbox".into(),
                label: "devbox".into(),
                kind: dataspace::DataSpaceKind::Ssh,
                detail: "yangyang.1205@10.37.218.31".into(),
                ssh_host: Some("10.37.218.31".into()),
                ssh_user: Some("yangyang.1205".into()),
                ssh_port: None,
                ssh_identity_file: None,
                config_source: Some("Moonbox config".into()),
                config_path: None,
            },
        ];
        app.show_data_spaces = true;
        app.data_space_selection = 1;

        app.handle_key(key('x'));

        assert_eq!(
            app.data_space_delete_confirmation.as_deref(),
            Some("devbox")
        );
        assert!(app.status_message.contains("Press x again"));

        app.handle_key(KeyEvent::from(KeyCode::Esc));

        assert_eq!(app.data_space_delete_confirmation, None);
        assert_eq!(app.status_message, "Data space delete cancelled");
    }

    #[test]
    fn command_palette_data_opens_visual_picker() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);

        app.handle_key(key(':'));
        for ch in "data".chars() {
            app.handle_key(key(ch));
        }
        app.handle_key(KeyEvent::from(KeyCode::Enter));

        assert!(app.show_data_spaces);
        assert_eq!(app.status_message, "Data spaces opened");
    }

    #[test]
    fn review_key_refreshes_capsule_and_opens_handoff_review() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        app.data.compilers = vec!["engineering-handoff".into()];
        app.selected_compiler = 0;
        app.pending_compiler = 0;
        app.data.capsule.compiler = "engineering-handoff".into();
        let compiler = app.data.capsule.compiler.clone();

        app.handle_key(key('c'));

        assert_eq!(app.compile_status, "COMPILED");
        assert_eq!(app.data.capsule.compiler, compiler);
        assert!(app.show_launch);
        assert!(app.launch_review);
        assert_eq!(app.pending_target, app.data.target);
        assert_eq!(app.status_message, "Capsule refreshed");
    }

    #[test]
    fn new_selects_requested_source_session() {
        let app = new_app(CliTool::Hermes, CliTool::Codex);

        assert_eq!(
            app.current_session().map(|session| session.id.as_str()),
            Some("hermes-cxcp-502")
        );
        assert_eq!(app.data.source, CliTool::Hermes);
        assert_eq!(app.data.target, CliTool::Codex);
        assert_eq!(app.rewind_event_id, "evt-052");
    }

    #[test]
    fn session_filter_limits_visible_sessions() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        app.handle_key(key('f'));
        assert!(
            app.visible_session_indices()
                .iter()
                .all(|index| app.data.sessions[*index].cli == CliTool::Codex)
        );
    }

    #[test]
    fn moonbox_handoff_worker_sessions_are_hidden_unless_explicitly_searched() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        let mut worker = app.data.sessions[0].clone();
        worker.id = "moonbox-worker-session".into();
        worker.title =
            "You are running a Moonbox continuation handoff job. Use the selected handoff skill."
                .into();
        app.data.sessions.insert(0, worker);
        let mut prompt_worker = app.data.sessions[0].clone();
        prompt_worker.id = "moonbox-worker-prompt-session".into();
        prompt_worker.title =
            "The following is the Codex agent history whose request action you are assessing."
                .into();
        app.data.sessions.insert(0, prompt_worker);
        let mut skill_worker = app.data.sessions[0].clone();
        skill_worker.id = "moonbox-worker-skill-session".into();
        skill_worker.title =
            "<skill><name>handoff</name><path>/Users/me/.codex/skills/handoff/SKILL.md</path></skill>"
                .into();
        app.data.sessions.insert(0, skill_worker);
        let mut aborted_worker = app.data.sessions[0].clone();
        aborted_worker.id = "moonbox-worker-aborted-session".into();
        aborted_worker.title =
            "<turn_aborted>The user interrupted the previous turn on purpose.</turn_aborted>"
                .into();
        app.data.sessions.insert(0, aborted_worker);

        app.refresh_visible_sessions();

        assert!(
            !app.visible_session_indices()
                .iter()
                .any(|index| app.data.sessions[*index].id == "moonbox-worker-session")
        );
        assert!(
            !app.visible_session_indices()
                .iter()
                .any(|index| app.data.sessions[*index].id == "moonbox-worker-prompt-session")
        );
        assert!(
            !app.visible_session_indices()
                .iter()
                .any(|index| app.data.sessions[*index].id == "moonbox-worker-skill-session")
        );
        assert!(
            !app.visible_session_indices()
                .iter()
                .any(|index| app.data.sessions[*index].id == "moonbox-worker-aborted-session")
        );

        app.search_query = "moonbox continuation handoff".into();
        app.refresh_visible_sessions();

        assert!(
            app.visible_session_indices()
                .iter()
                .any(|index| app.data.sessions[*index].id == "moonbox-worker-session")
        );
    }

    #[test]
    fn session_filter_cycles_archived_between_starred_and_all() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);

        app.handle_key(key('['));

        assert_eq!(app.session_filter, SessionFilter::Archived);
        assert_eq!(app.status_message, "Filter: Archived");

        app.handle_key(key('['));

        assert_eq!(app.session_filter, SessionFilter::Starred);
        assert_eq!(app.status_message, "Filter: Star");

        app.handle_key(key(']'));

        assert_eq!(app.session_filter, SessionFilter::Archived);

        app.handle_key(key(']'));

        assert_eq!(app.session_filter, SessionFilter::All);
    }

    #[test]
    fn star_shortcut_toggles_current_session_and_filter() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        let session_id = app.current_session().expect("session").id.clone();

        app.handle_key(key('s'));

        assert_eq!(app.status_message, "Session starred");
        assert!(
            app.starred_sessions
                .iter()
                .any(|key| key.ends_with(session_id.as_str()))
        );

        app.apply_session_filter(SessionFilter::Starred);

        assert_eq!(app.visible_session_indices().len(), 1);
        assert_eq!(
            app.current_session().map(|session| session.id.as_str()),
            Some(session_id.as_str())
        );

        app.handle_key(key('*'));

        assert_eq!(app.status_message, "Session unstarred");
        assert!(app.visible_session_indices().is_empty());
    }

    #[test]
    fn current_session_respects_empty_filter_results() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        app.search_query = "no-match".into();
        app.refresh_visible_sessions();
        app.clamp_selected_session();

        assert!(app.visible_session_indices().is_empty());
        assert!(app.current_session().is_none());
    }

    #[test]
    fn slash_search_filters_while_typing() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);

        app.handle_key(key('/'));
        app.handle_key(key('5'));

        assert!(app.command_mode);
        assert_eq!(app.search_query, "5");
        assert_eq!(app.visible_session_indices().len(), 1);
        assert_eq!(
            app.current_session().map(|session| session.id.as_str()),
            Some("hermes-cxcp-502")
        );
        assert!(!app.is_session_load_pending());
        assert!(app.is_session_preview_pending());
        assert!(!app.selected_session_timeline_loaded());
        assert_eq!(app.data.source, CliTool::Codex);
        assert_eq!(app.data.capsule.source_session, "codex-cxcp-design");
        assert!(!app.data.timeline.is_empty());
    }

    #[test]
    fn slash_search_matches_source_path() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        let session = app.data.sessions.get_mut(1).expect("fixture session");
        session.source_path = Some("/tmp/moonbox/raw-title-source.jsonl".into());

        app.handle_key(key('/'));
        for ch in "raw-title-source".chars() {
            app.handle_key(key(ch));
        }

        assert_eq!(app.visible_session_indices().len(), 1);
        assert_eq!(
            app.current_session()
                .and_then(|session| session.source_path.as_deref()),
            Some("/tmp/moonbox/raw-title-source.jsonl")
        );
    }

    #[test]
    fn slash_search_escape_keeps_filter_result() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);

        app.handle_key(key('/'));
        app.handle_key(key('5'));
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::empty()));

        assert!(!app.command_mode);
        assert_eq!(app.search_query, "5");
        assert_eq!(app.visible_session_indices().len(), 1);
    }

    #[test]
    fn main_escape_does_not_quit() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);

        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::empty()));

        assert!(!app.should_quit());
        assert_eq!(app.status_message, "Press q or Ctrl-C to quit");
    }

    #[test]
    fn q_and_ctrl_c_quit_from_main_screen() {
        let mut q_app = new_app(CliTool::Codex, CliTool::Hermes);
        q_app.handle_key(key('q'));
        assert!(q_app.should_quit());

        let mut ctrl_c_app = new_app(CliTool::Codex, CliTool::Hermes);
        ctrl_c_app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert!(ctrl_c_app.should_quit());
    }

    #[test]
    fn clear_filter_resets_source_and_search() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);

        app.apply_session_filter(SessionFilter::Tool(CliTool::Hermes));
        app.handle_key(key('/'));
        app.handle_key(key('5'));
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::empty()));
        app.run_palette_command("clear");

        assert_eq!(app.session_filter, SessionFilter::All);
        assert!(app.search_query.is_empty());
        assert_eq!(app.visible_session_indices().len(), 3);
        assert_eq!(app.status_message, "Filters cleared");
        assert!(!app.is_session_load_pending());
    }

    #[test]
    fn archive_shortcut_hides_after_feedback_and_unarchive_restores() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        let first_id = app.current_session().expect("session").id.clone();
        let second_id = app.data.sessions[app.visible_session_indices()[1]]
            .id
            .clone();

        app.handle_key(key('a'));

        assert_eq!(app.status_message, "Archiving session...");
        assert!(
            app.visible_session_indices()
                .iter()
                .any(|index| app.data.sessions[*index].id == first_id)
        );
        assert_eq!(
            app.current_session()
                .and_then(|session| app.archive_feedback_for_session(session)),
            Some(ArchiveFeedbackKind::Archive)
        );

        for _ in 0..ARCHIVE_FEEDBACK_FRAMES {
            app.advance_animation();
        }

        assert!(app.status_message.starts_with("Session archived: "));
        assert!(
            !app.visible_session_indices()
                .iter()
                .any(|index| app.data.sessions[*index].id == first_id)
        );
        assert_eq!(
            app.current_session().map(|session| session.id.as_str()),
            Some(second_id.as_str())
        );

        app.apply_session_filter(SessionFilter::Archived);
        app.search_query = first_id.clone();
        app.refresh_visible_sessions();
        app.clamp_selected_session();

        assert_eq!(
            app.current_session().map(|session| session.id.as_str()),
            Some(first_id.as_str())
        );

        app.handle_key(key('a'));
        assert_eq!(app.status_message, "Unarchiving session...");

        for _ in 0..ARCHIVE_FEEDBACK_FRAMES {
            app.advance_animation();
        }

        assert!(app.visible_session_indices().is_empty());
        assert!(app.status_message.starts_with("Session unarchived: "));

        app.search_query.clear();
        app.apply_session_filter(SessionFilter::All);

        assert!(
            app.visible_session_indices()
                .iter()
                .any(|index| app.data.sessions[*index].id == first_id)
        );
    }

    #[test]
    fn source_filter_cycles_in_tui() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        app.handle_key(key(']'));
        assert_eq!(app.session_filter, SessionFilter::Tool(CliTool::Codex));

        app.handle_key(key(']'));
        assert_eq!(app.session_filter, SessionFilter::Tool(CliTool::Claude));
        assert!(!app.is_session_load_pending());
        assert!(app.is_session_preview_pending());
        assert_eq!(
            app.current_session().map(|session| session.id.as_str()),
            Some("claude-qc-platform")
        );
        assert_eq!(app.data.source, CliTool::Codex);
        assert_eq!(app.data.capsule.source_session, "codex-cxcp-design");
        assert!(!app.selected_session_timeline_loaded());
        assert!(
            app.visible_session_indices()
                .iter()
                .all(|index| app.data.sessions[*index].cli == CliTool::Claude)
        );

        app.handle_key(key('['));
        assert_eq!(app.session_filter, SessionFilter::Tool(CliTool::Codex));
    }

    #[test]
    fn moving_session_defers_timeline_context_until_action_needs_it() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);

        app.handle_key(key('j'));

        assert_eq!(
            app.current_session().map(|session| session.id.as_str()),
            Some("claude-qc-platform")
        );
        assert!(!app.is_session_load_pending());
        assert!(app.is_session_preview_pending());
        assert!(app.pending_session_preview.is_none());
        assert!(app.deferred_session_preview.is_some());
        assert_eq!(app.data.source, CliTool::Codex);
        assert_eq!(app.data.capsule.source_cli, CliTool::Codex);
        assert_eq!(app.data.capsule.source_session, "codex-cxcp-design");
        assert!(!app.data.timeline.is_empty());
        assert!(!app.selected_session_timeline_loaded());
        assert_eq!(app.compile_status, "PENDING");
        assert!(!app.verify_passed);

        assert!(!app.ensure_session_details_ready("Timeline detail"));
        assert!(app.is_session_load_pending());
        settle_session_load(&mut app);

        assert_eq!(app.data.capsule.source_session, "claude-qc-platform");
        assert_eq!(app.rewind_event_id, "evt-074");
        assert_eq!(app.selected_event, 4);
        assert!(app.data.timeline[0].detail.contains("QC platform"));
        assert!(app.data.branches[1].label.contains("evt-074"));
        assert_eq!(app.compile_status, "PREVIEW");
        assert!(!app.verify_passed);
        assert!(
            app.status_message
                .starts_with("Timeline: Claude QC platform trace repair (5 events, ")
        );
    }

    #[test]
    fn rapid_session_moves_ignore_stale_background_loads() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);

        app.handle_key(key('j'));
        app.handle_key(key('k'));

        assert!(!app.is_session_load_pending());
        assert_eq!(
            app.current_session().map(|session| session.id.as_str()),
            Some("codex-cxcp-design")
        );
        assert_eq!(app.data.source, CliTool::Codex);
        assert_eq!(app.data.capsule.source_session, "codex-cxcp-design");
        assert!(app.selected_session_timeline_loaded());
        assert!(!app.is_session_preview_pending());
        assert!(!app.data.timeline.is_empty());
    }

    #[test]
    fn rapid_session_scanning_debounces_timeline_preview_worker() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);

        for _ in 0..16 {
            app.handle_key(key('j'));
            app.handle_key(key('k'));
        }
        app.handle_key(key('j'));

        assert_eq!(
            app.current_session().map(|session| session.id.as_str()),
            Some("claude-qc-platform")
        );
        assert!(!app.is_session_load_pending());
        assert!(app.is_session_preview_pending());
        assert!(
            app.pending_session_preview.is_none(),
            "navigation should not start IO worker before debounce expires"
        );
        assert!(app.deferred_session_preview.is_some());
    }

    #[test]
    fn launch_picker_starts_review_with_deferred_selected_session_context() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);

        app.handle_key(key('j'));
        app.handle_key(key('H'));

        assert!(!app.is_session_load_pending());
        assert!(app.show_launch);
        assert_eq!(app.status_message, "Choose target CLI");
        let validation = app.validate_launch_for_target(app.pending_target);
        assert_eq!(validation.state, LaunchValidationState::Warning);
        assert_eq!(
            validation.summary(),
            "selected session context loads when review starts"
        );
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
        assert!(app.show_launch);
        assert!(app.is_launch_review_pending());
        assert!(
            app.status_message
                .starts_with("Preparing handoff review: Hermes"),
            "{}",
            app.status_message
        );
    }

    #[test]
    fn launch_defaults_real_builtin_draft_to_handoff_skill() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        app.data.compilers = vec![
            "engineering-handoff".into(),
            "agent:codex:handoff".into(),
            "agent:claude:handoff".into(),
        ];
        app.selected_compiler = 0;
        app.data.capsule.compiler = "engineering-handoff".into();
        app.data.sessions[app.selected_session].source_provenance = SourceProvenance::Real;

        app.ensure_launch_handoff_skill_default();

        assert_eq!(app.data.capsule.compiler, "agent:codex:handoff");
        assert!(!app.launch_requires_handoff_skill(CliTool::Hermes));
    }

    #[test]
    fn launch_picker_cycles_runner_for_selected_handoff_skill() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        app.data.compilers = vec![
            "agent:codex:handoff".into(),
            "agent:claude:handoff".into(),
            "agent:codex:moonbox-handoff".into(),
        ];
        app.selected_compiler = 0;
        app.pending_compiler = 0;
        app.data.capsule.compiler = "agent:codex:handoff".into();

        app.cycle_handoff_runner();

        assert_eq!(app.data.capsule.compiler, "agent:claude:handoff");
        assert_eq!(app.status_message, "Runner: Claude / handoff");
    }

    #[test]
    fn target_cycles_inside_launch_picker() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        app.handle_key(key('t'));
        assert!(app.show_launch);
        assert_eq!(app.status_message, "Choose target CLI");

        app.handle_key(key('j'));
        assert_eq!(app.pending_target, CliTool::Codex);
        assert_eq!(app.data.target, CliTool::Hermes);
        assert_eq!(app.status_message, "Target: Codex");

        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
        assert!(app.show_launch);
        assert!(app.is_launch_review_pending());
        assert!(!app.launch_review);
        assert_eq!(app.data.target, CliTool::Hermes);
        assert!(
            app.status_message
                .starts_with("Preparing handoff review: Codex")
        );
        assert!(app.handoff_trail_frame().is_some());
        settle_launch_review(&mut app);
        assert!(app.launch_review);
        assert_eq!(app.data.target, CliTool::Codex);
        assert!(
            app.status_message
                .starts_with("Handoff review ready: Codex")
        );
        assert_eq!(app.data.source, CliTool::Codex);
        assert_eq!(app.data.capsule.source_cli, CliTool::Codex);
        assert_eq!(app.data.capsule.source_session, "codex-cxcp-design");
        assert!(app.data.capsule.handoff_label.contains("codex"));
        assert!(app.data.branches[2].label.contains("codex"));
    }

    #[test]
    fn launch_review_worker_compiles_selected_compiler() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        let compiler_index = app
            .data
            .compilers
            .iter()
            .position(|compiler| compiler == "design-review")
            .expect("design-review compiler");
        app.selected_compiler = compiler_index;

        app.handle_key(key('H'));
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
        settle_launch_review(&mut app);

        assert!(app.launch_review);
        assert_eq!(app.data.capsule.compiler, "design-review");
        assert_eq!(app.data.capsule.state, "draft_from_builtin_compiler");
        assert!(
            app.data
                .capsule
                .decisions
                .iter()
                .any(|decision| { decision.contains("built-in deterministic draft compiler") })
        );
        assert!(
            app.status_message
                .starts_with("Handoff review ready: Hermes")
        );
    }

    #[test]
    fn stale_handoff_after_skill_change_regenerates_on_launch_enter() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        let compiler_index = app
            .data
            .compilers
            .iter()
            .position(|compiler| compiler == "design-review")
            .expect("design-review compiler");
        app.selected_compiler = compiler_index;
        app.data.capsule.compiler = "design-review".into();

        let validation = app.validate_launch_for_target(CliTool::Hermes);
        assert!(validation.is_blocked());
        assert!(validation.summary().contains("generated_by"));

        app.handle_key(key('H'));
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));

        assert!(app.show_launch);
        assert!(app.is_launch_review_pending());
        assert_eq!(app.status_message, "Regenerating handoff review: Hermes");

        settle_launch_review(&mut app);
        assert!(app.launch_review);
        assert_eq!(app.data.capsule.compiler, "design-review");
        assert!(!app.validate_launch_for_target(CliTool::Hermes).is_blocked());
    }

    #[test]
    fn stale_launch_review_enter_regenerates_with_selected_skill() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        let compiler_index = app
            .data
            .compilers
            .iter()
            .position(|compiler| compiler == "design-review")
            .expect("design-review compiler");
        app.selected_compiler = compiler_index;
        app.show_launch = true;
        app.launch_review = true;
        app.pending_target = CliTool::Hermes;

        assert!(app.validate_launch_for_target(CliTool::Hermes).is_blocked());

        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));

        assert!(app.show_launch);
        assert!(!app.launch_review);
        assert!(app.is_launch_review_pending());
        assert_eq!(app.status_message, "Regenerating handoff review: Hermes");

        settle_launch_review(&mut app);
        assert!(app.launch_review);
        assert_eq!(app.data.capsule.compiler, "design-review");
        assert!(!app.validate_launch_for_target(CliTool::Hermes).is_blocked());
    }

    #[test]
    fn failed_launch_review_stays_visible_with_retry_actions() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        app.data
            .compilers
            .insert(0, "agent:codex:missing-skill".into());
        app.selected_compiler = 0;

        app.handle_key(key('H'));
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));

        assert!(app.show_launch);
        assert!(app.is_launch_review_pending());
        assert!(!app.launch_review);

        settle_launch_review(&mut app);

        let error = app.launch_review_error().expect("launch review error");
        assert_eq!(error.target, CliTool::Hermes);
        assert_eq!(error.compiler_id, "agent:codex:missing-skill");
        assert!(error.message.contains("skill_not_found"));
        assert!(app.show_launch);
        assert!(!app.launch_review);
        assert!(app.target_launch_result.is_none());
        assert_eq!(app.compile_status, "FAILED");
        assert!(!app.verify_passed);
        assert!(
            app.status_message
                .starts_with("Handoff review failed: invalid compiler config")
        );

        app.handle_key(key('y'));
        assert_eq!(
            app.status_message,
            "Handoff review failed; press r to retry or S to choose skill"
        );

        let request_id = app.launch_review_request_id;
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
        assert_eq!(app.launch_review_request_id, request_id);
        assert!(app.launch_review_error().is_some());
        assert_eq!(
            app.status_message,
            "Handoff review failed; press r to retry or S to choose skill"
        );

        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::empty()));
        assert!(!app.show_launch);
        assert!(app.launch_review_error().is_none());
    }

    #[test]
    fn handoff_trail_starts_for_review_and_expires_under_800ms() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);

        app.handle_key(key('x'));
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));

        let frame = app.handoff_trail_frame().expect("handoff trail frame");
        assert_eq!(frame.phase, HandoffTrailPhase::Review);
        assert!(frame.duration_ms <= 800);

        app.set_handoff_trail_elapsed_for_test(Duration::from_millis(
            HANDOFF_TRAIL_DURATION_MS + 1,
        ));
        assert!(app.handoff_trail_frame().is_none());
        assert!(app.poll_background());
        assert!(app.handoff_trail.is_none());
    }

    #[test]
    fn closing_launch_review_clears_handoff_trail() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);

        app.handle_key(key('x'));
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
        assert!(app.handoff_trail_frame().is_some());
        settle_launch_review(&mut app);

        app.handle_key(key('q'));

        assert!(!app.show_launch);
        assert!(!app.launch_review);
        assert!(app.handoff_trail_frame().is_none());
    }

    #[test]
    fn hiding_pending_launch_review_keeps_background_job() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);

        app.handle_key(key('x'));
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
        assert!(app.is_launch_review_pending());

        app.handle_key(key('q'));

        assert!(!app.show_launch);
        assert!(app.is_launch_review_pending());
        assert_eq!(app.status_message, "Handoff job continues in background");

        settle_launch_review(&mut app);

        assert!(!app.show_launch);
        assert!(app.launch_review);
        assert!(
            app.status_message
                .starts_with("Handoff ready in background: Hermes")
        );

        app.handle_key(key('x'));

        assert!(app.show_launch);
        assert!(app.launch_review);
        assert_eq!(app.status_message, "Handoff review ready: Hermes");
    }

    #[test]
    fn pending_launch_review_reuses_existing_worker() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);

        app.handle_key(key('x'));
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
        assert!(app.is_launch_review_pending());
        let request_id = app.launch_review_request_id;

        app.handle_key(key('q'));
        assert!(!app.show_launch);
        assert!(app.is_launch_review_pending());

        app.handle_key(key('x'));
        assert!(app.show_launch);
        assert!(app.is_launch_review_pending());
        assert_eq!(app.launch_review_request_id, request_id);
        assert!(
            app.status_message
                .starts_with("Handoff job already running: Hermes")
        );

        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
        assert_eq!(app.launch_review_request_id, request_id);
        assert!(app.is_launch_review_pending());
    }

    #[test]
    fn ready_handoff_review_reuses_existing_artifact_without_new_worker() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        let session_id = app.current_session().expect("session").id.clone();
        let compiler_id = app.data.compilers[app.selected_compiler].clone();
        let rewind_event_id = app.rewind_event_id.clone();
        app.data.capsule.source_session = session_id;
        app.data.capsule.target_cli = CliTool::Hermes;
        app.data.capsule.compiler = compiler_id;
        app.data.capsule.rewind_point = format!("{rewind_event_id} / reviewed");
        app.data.capsule.handoff_artifact = Some("existing reviewed artifact".into());
        app.show_launch = true;
        app.launch_review = false;
        app.pending_target = CliTool::Hermes;
        let request_id = app.launch_review_request_id;

        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));

        assert_eq!(app.launch_review_request_id, request_id);
        assert!(!app.is_launch_review_pending());
        assert!(app.launch_review);
        assert_eq!(app.status_message, "Handoff review ready: Hermes");
    }

    #[test]
    fn skill_handoff_review_copies_exact_artifact_and_details() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        app.data.compilers.insert(0, "agent:codex:handoff".into());
        app.selected_compiler = 0;
        app.data.capsule.compiler = "agent:codex:handoff".into();
        if let Some(raw_source_map) = &mut app.data.capsule.raw_source_map {
            raw_source_map.generated_by = "agent:codex:handoff".into();
        }
        app.data.capsule.handoff_artifact = Some("# Handoff\n\nContinue exactly.".into());
        app.data.capsule.handoff_artifact_path =
            Some("/tmp/moonbox-continuation-handoff-demo.md".into());
        app.data.capsule.handoff_runner = Some("Codex".into());
        app.data.capsule.handoff_skill = Some("handoff".into());
        app.show_launch = true;
        app.launch_review = true;
        app.pending_target = CliTool::Hermes;

        app.handle_key(key('y'));

        assert_eq!(
            app.take_clipboard_text().as_deref(),
            Some("# Handoff\n\nContinue exactly.")
        );
        assert_eq!(app.status_message, "Copied handoff text");

        app.handle_key(key('p'));

        assert_eq!(
            app.take_clipboard_text().as_deref(),
            Some("/tmp/moonbox-continuation-handoff-demo.md")
        );
        assert_eq!(app.status_message, "Copied handoff path");

        app.handle_key(key('d'));

        assert!(app.launch_review_details);
        assert_eq!(app.status_message, "Handoff details opened");

        app.handle_key(key('q'));

        assert!(app.show_launch);
        assert!(app.launch_review);
        assert!(!app.launch_review_details);
        assert_eq!(app.status_message, "Handoff details closed");
    }

    #[test]
    fn uppercase_h_remains_launch_picker_compatibility_alias() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);

        app.handle_key(key('H'));

        assert!(app.show_launch);
        assert_eq!(app.status_message, "Choose target CLI");
    }

    #[test]
    fn target_change_preserves_selected_rewind_point() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        app.selected_event = 0;
        app.handle_key(key(' '));
        app.handle_key(key('H'));
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
        settle_launch_review(&mut app);

        assert_eq!(app.rewind_event_id, "evt-001");
        assert!(app.data.capsule.rewind_point.contains("evt-001"));
        assert!(app.data.capsule.handoff_label.contains("evt-001"));
    }

    #[test]
    fn launch_picker_cancel_discards_pending_target() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        app.handle_key(key('H'));
        app.handle_key(key('j'));
        app.handle_key(key('q'));

        assert!(!app.show_launch);
        assert_eq!(app.pending_target, CliTool::Codex);
        assert_eq!(app.data.target, CliTool::Hermes);
        assert_eq!(app.status_message, "Launch cancelled");
    }

    #[test]
    fn launch_validation_warns_for_same_cli_handoff() {
        let app = new_app(CliTool::Codex, CliTool::Codex);

        let validation = app.validate_launch_for_target(CliTool::Codex);
        let report = app
            .launch_verification_for_target(CliTool::Codex)
            .expect("launch verification");

        assert_eq!(validation.state, LaunchValidationState::Warning);
        assert!(validation.summary().contains("Same-CLI handoff"));
        assert!(report.checks.iter().any(|check| {
            check.name == "target_support" && check.detail.contains("Same-CLI handoff")
        }));
    }

    #[test]
    fn target_picker_blocks_failed_same_cli_resume_path() {
        let mut app = new_app(CliTool::Hermes, CliTool::Hermes);

        app.handle_key(key('H'));
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));

        assert!(app.show_launch);
        assert!(!app.launch_review);
        assert_eq!(app.data.target, CliTool::Hermes);
        assert!(app.status_message.starts_with("Target blocked:"));
        assert!(app.status_message.contains("raw resume is known failed"));
    }

    #[test]
    fn blocked_target_cannot_copy_launch_command() {
        let mut app = new_app(CliTool::Hermes, CliTool::Hermes);

        app.handle_key(key('H'));
        app.handle_key(key('y'));

        assert!(app.take_clipboard_text().is_none());
        assert!(app.status_message.starts_with("Target blocked:"));
    }

    #[test]
    fn launch_picker_requires_visible_session() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        app.search_query = "no-match".into();
        app.refresh_visible_sessions();
        app.clamp_selected_session();

        app.handle_key(key('H'));

        assert!(!app.show_launch);
        assert_eq!(app.status_message, "No session selected");
    }

    #[test]
    fn verify_toggle_reports_status() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);

        app.handle_key(key('v'));

        assert!(app.verify_passed);
        assert!(app.status_message.starts_with("Verify: WARN ("));
        assert!(app.status_message.ends_with(" checks)"));
    }

    #[test]
    fn launch_copy_queues_clipboard_text() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        app.handle_key(key('H'));

        app.handle_key(key('y'));
        assert!(app.take_clipboard_text().is_none());
        assert_eq!(app.status_message, "Confirm target first with enter");

        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
        assert!(app.is_launch_review_pending());
        settle_launch_review(&mut app);
        assert!(app.launch_review);
        app.handle_key(key('y'));

        let copied = app.take_clipboard_text().expect("clipboard text");
        assert!(copied.starts_with("hermes chat "));
        assert!(copied.contains("Moonbox cross-CLI handoff"));
        assert_eq!(app.status_message, "Copied launch command");
    }

    #[test]
    fn launch_review_enter_queues_target_handoff_without_executing_in_tests() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        app.handle_key(key('H'));
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
        settle_launch_review(&mut app);
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));

        assert!(!app.should_quit());
        assert!(app.take_exit_action().is_none());
        let Some(plan) = app.take_pending_launch() else {
            panic!("expected pending target handoff");
        };
        assert_eq!(plan.source_session.id, "codex-cxcp-design");
        assert_eq!(plan.target_cli, CliTool::Hermes);
        assert!(plan.dry_run);
        assert!(app.launch_review);
        assert_eq!(app.status_message, "Launching target: Hermes");
    }

    #[test]
    fn launch_review_r_queues_target_handoff_without_executing_in_tests() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        app.handle_key(key('H'));
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
        settle_launch_review(&mut app);
        app.handle_key(key('r'));

        assert!(!app.should_quit());
        assert!(app.take_exit_action().is_none());
        let Some(plan) = app.take_pending_launch() else {
            panic!("expected pending target handoff");
        };
        assert_eq!(plan.source_session.id, "codex-cxcp-design");
        assert_eq!(plan.target_cli, CliTool::Hermes);
        assert!(plan.dry_run);
    }

    #[test]
    fn launch_review_supports_vim_jump_keys() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        app.handle_key(key('H'));
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
        settle_launch_review(&mut app);

        app.handle_key(key('G'));
        assert_eq!(app.modal_scroll, u16::MAX);

        app.handle_key(key('g'));
        assert!(app.pending_g);
        app.handle_key(key('g'));
        assert_eq!(app.modal_scroll, 0);
        assert_eq!(app.status_message, "Review top");
    }

    #[test]
    fn launch_review_blocks_real_draft_run_before_spawn() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        app.handle_key(key('H'));
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
        settle_launch_review(&mut app);
        app.data.sessions[app.selected_session].source_provenance = SourceProvenance::Real;

        app.handle_key(key('r'));

        assert!(app.take_pending_launch().is_none());
        assert_eq!(
            app.status_message,
            "Choose an AI handoff skill before running"
        );
    }

    #[test]
    fn real_builtin_draft_enter_uses_default_handoff_skill_for_real_session() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        app.data.compilers = vec![
            "engineering-handoff".into(),
            "agent:codex:moonbox-handoff".into(),
        ];
        app.data.sessions[app.selected_session].source_provenance = SourceProvenance::Real;
        app.selected_compiler = compiler_index_for_id(&app.data, "engineering-handoff", 0);
        app.data.capsule.compiler = "engineering-handoff".into();

        app.handle_key(key('H'));
        assert!(app.show_launch);
        assert!(!app.show_skill_picker);
        assert!(!compiler::compiler_is_builtin(&app.data.capsule.compiler));
        assert!(!app.launch_requires_handoff_skill(CliTool::Hermes));

        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));

        assert!(app.show_launch);
        assert!(!app.launch_review);
        if let Some(plan) = app.take_pending_setup_install() {
            assert_eq!(plan.target, setup::SetupInstallTarget::CodexSdk);
            assert!(app.status_message.contains("Install Codex SDK"));
        } else {
            assert!(app.is_launch_review_pending());
            assert_eq!(app.status_message, "Regenerating handoff review: Hermes");
        }
    }

    #[test]
    fn launch_entry_refreshes_agent_skill_catalog_for_real_sessions() {
        let mut app = new_app(CliTool::Codex, CliTool::Claude);
        app.data.sessions[app.selected_session].source_provenance = SourceProvenance::Real;
        app.data.compilers = vec!["engineering-handoff".into()];
        app.selected_compiler = 0;
        app.pending_compiler = 0;
        app.data.capsule.compiler = "engineering-handoff".into();

        app.handle_key(key('H'));

        assert!(app.show_launch);
        assert!(!app.show_skill_picker);
        assert!(
            app.data
                .compilers
                .iter()
                .any(|compiler| compiler == "agent:claude:moonbox-handoff")
        );
        assert_eq!(
            app.data.compilers[app.selected_compiler],
            "agent:claude:moonbox-handoff"
        );
        assert_eq!(app.status_message, "Choose target CLI");
    }

    #[test]
    fn skill_picker_enter_from_launch_applies_skill_without_starting_review() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        app.data.compilers = vec![
            "engineering-handoff".into(),
            "agent:codex:moonbox-handoff".into(),
        ];
        app.data.sessions[app.selected_session].source_provenance = SourceProvenance::Real;
        app.selected_compiler = compiler_index_for_id(&app.data, "engineering-handoff", 0);
        app.data.capsule.compiler = "engineering-handoff".into();

        app.handle_key(key('H'));
        app.handle_key(key('S'));

        assert!(app.show_launch);
        assert!(app.show_skill_picker);
        let pending_compiler = app.pending_compiler;
        let pending_id = app.data.compilers[pending_compiler].clone();
        assert!(app.pending_skill_setup_install_plan().is_none());

        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));

        assert!(app.take_pending_setup_install().is_none());

        assert!(!app.show_skill_picker);
        assert!(app.show_launch);
        assert_eq!(app.selected_compiler, pending_compiler);
        assert_eq!(app.data.capsule.compiler, pending_id);
        assert!(!app.is_launch_review_pending());
        assert!(!app.launch_review);
        assert_eq!(
            app.status_message,
            "Handoff skill: moonbox-handoff; press Enter to generate Review"
        );

        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));

        if let Some(plan) = app.take_pending_setup_install() {
            assert_eq!(plan.target, setup::SetupInstallTarget::CodexSdk);
            assert!(app.status_message.contains("Install Codex SDK"));
        } else {
            assert!(app.is_launch_review_pending());
        }
        assert!(!app.launch_review);
    }

    #[test]
    fn completed_target_handoff_returns_to_visible_result_actions() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        app.handle_key(key('H'));
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
        settle_launch_review(&mut app);
        app.handle_key(key('r'));
        let plan = app.take_pending_launch().expect("pending target handoff");

        app.complete_target_handoff(
            plan,
            Err(CoreError::LaunchStart {
                command: "hermes chat".into(),
                reason: "fixture target missing".into(),
            }),
        );

        let result = app
            .target_launch_result
            .as_ref()
            .expect("launch result panel");
        assert!(app.show_launch);
        assert!(!app.launch_review);
        assert!(!result.success);
        assert!(result.outcome.contains("failed to start"));

        app.handle_key(key('y'));
        assert!(
            app.take_clipboard_text()
                .expect("copied target command")
                .contains("Moonbox cross-CLI handoff")
        );

        app.handle_key(key('r'));
        assert!(app.take_pending_launch().is_some());
    }

    #[test]
    fn x_shortcut_opens_target_handoff_picker() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);

        app.handle_key(key('x'));

        assert!(app.show_launch);
        assert!(!app.launch_review);
        assert_eq!(app.status_message, "Choose target CLI");
    }

    #[test]
    fn main_enter_queues_original_resume_without_opening_handoff_picker() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);

        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));

        assert!(!app.should_quit());
        assert!(!app.show_launch);
        assert!(app.take_exit_action().is_none());
        let Some(plan) = app.take_pending_resume() else {
            panic!("expected pending original resume");
        };
        assert_eq!(plan.source_session.id, "codex-cxcp-design");
        assert_eq!(plan.command.display, "codex resume codex-cxcp-design");
        assert_eq!(
            app.status_message,
            "Suspending to original: Codex codex-cxcp-design"
        );
    }

    #[test]
    fn main_enter_keeps_resume_when_hooks_enabled_but_smart_enter_off() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        enable_hook_config(&mut app, false);
        let session = app.current_session().expect("session").clone();
        app.set_hook_live_events_for_test(vec![hook_event_for_session(
            &session,
            hooks::HookEventKind::PreToolUse,
            Some("/tmp/tmux-501/default,1,0"),
            Some("%42"),
        )]);

        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));

        assert!(app.take_pending_tmux_jump().is_none());
        assert!(app.take_pending_resume().is_some());
        assert_eq!(app.enter_key_hint(), "Resume");
    }

    #[test]
    fn main_enter_queues_tmux_jump_when_smart_enter_has_live_pane_metadata() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        enable_hook_config(&mut app, true);
        let session = app.current_session().expect("session").clone();
        app.set_hook_live_events_for_test(vec![hook_event_for_session(
            &session,
            hooks::HookEventKind::PreToolUse,
            Some("/tmp/tmux-501/default,1,0"),
            Some("%42"),
        )]);

        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));

        let Some(plan) = app.take_pending_tmux_jump() else {
            panic!("expected tmux jump plan");
        };
        assert!(app.take_pending_resume().is_none());
        assert_eq!(plan.command.socket_path, "/tmp/tmux-501/default");
        assert_eq!(plan.command.pane_id, "%42");
        assert_eq!(plan.source_session.id, session.id);
        assert_eq!(app.enter_key_hint(), "Jump");
        assert!(app.status_message.contains("Jumping to pane %42"));
    }

    #[test]
    fn action_model_and_enter_hint_use_same_live_tmux_state() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        enable_hook_config(&mut app, true);
        let session = app.current_session().expect("session").clone();
        app.set_hook_live_events_for_test(vec![hook_event_for_session(
            &session,
            hooks::HookEventKind::PreToolUse,
            Some("/tmp/tmux-501/default,1,0"),
            Some("%42"),
        )]);

        let actions = app.session_actions(&session);

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
        assert_eq!(app.enter_key_hint(), "Jump");
    }

    #[test]
    fn failed_tmux_jump_queues_resume_with_visible_reason() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        let session = app.current_session().expect("session").clone();
        let plan = TmuxJumpPlan {
            source_session: session.clone(),
            command: tmux::TmuxJumpCommand {
                program: "tmux".into(),
                socket_path: "/tmp/tmux-501/default".into(),
                pane_id: "%42".into(),
                display: "tmux -S /tmp/tmux-501/default select-pane -t %42".into(),
            },
        };

        app.complete_tmux_jump(Box::new(plan), Err("tmux pane %42 is not live".into()));

        let Some(resume) = app.take_pending_resume() else {
            panic!("expected fallback resume");
        };
        assert_eq!(resume.source_session.id, session.id);
        assert!(app.status_message.contains("Tmux jump unavailable"));
        assert!(app.status_message.contains("tmux pane %42 is not live"));
    }

    #[test]
    fn main_enter_falls_back_to_resume_when_smart_enter_lacks_tmux_metadata() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        enable_hook_config(&mut app, true);
        let session = app.current_session().expect("session").clone();
        app.set_hook_live_events_for_test(vec![hook_event_for_session(
            &session,
            hooks::HookEventKind::PreToolUse,
            None,
            Some("%42"),
        )]);

        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));

        assert!(app.take_pending_tmux_jump().is_none());
        assert!(app.take_pending_resume().is_some());
        assert_eq!(app.enter_key_hint(), "Resume");
    }

    #[test]
    fn remote_enter_opens_handoff_picker_instead_of_local_original_resume() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        app.data_spaces = vec![
            dataspace::DataSpaceEntry::local(),
            dataspace::DataSpaceEntry {
                id: "ssh:devbox".into(),
                label: "devbox".into(),
                kind: dataspace::DataSpaceKind::Ssh,
                detail: "yangyang.1205@10.37.218.31".into(),
                ssh_host: Some("10.37.218.31".into()),
                ssh_user: Some("yangyang.1205".into()),
                ssh_port: None,
                ssh_identity_file: None,
                config_source: Some("Moonbox config".into()),
                config_path: None,
            },
        ];
        app.selected_data_space = 1;

        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));

        assert!(app.show_launch);
        assert!(!app.launch_review);
        assert!(app.take_pending_resume().is_none());
        assert_eq!(
            app.status_message,
            "SSH source is read-only; choose a local target for handoff"
        );
    }

    #[test]
    fn remote_original_open_is_blocked() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        app.data_spaces = vec![
            dataspace::DataSpaceEntry::local(),
            dataspace::DataSpaceEntry {
                id: "ssh:devbox".into(),
                label: "devbox".into(),
                kind: dataspace::DataSpaceKind::Ssh,
                detail: "yangyang.1205@10.37.218.31".into(),
                ssh_host: Some("10.37.218.31".into()),
                ssh_user: Some("yangyang.1205".into()),
                ssh_port: None,
                ssh_identity_file: None,
                config_source: Some("Moonbox config".into()),
                config_path: None,
            },
        ];
        app.selected_data_space = 1;

        app.open_original();

        assert!(!app.show_open_original);
        assert_eq!(
            app.status_message,
            "SSH sessions cannot be opened locally; use handoff"
        );
    }

    #[test]
    fn remote_action_menu_blocks_resume_but_allows_handoff() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        app.data_spaces = vec![
            dataspace::DataSpaceEntry::local(),
            dataspace::DataSpaceEntry {
                id: "ssh:devbox".into(),
                label: "devbox".into(),
                kind: dataspace::DataSpaceKind::Ssh,
                detail: "yangyang.1205@10.37.218.31".into(),
                ssh_host: Some("10.37.218.31".into()),
                ssh_user: Some("yangyang.1205".into()),
                ssh_port: None,
                ssh_identity_file: None,
                config_source: Some("Moonbox config".into()),
                config_path: None,
            },
        ];
        app.selected_data_space = 1;

        app.handle_key(key('o'));
        let entries = app.action_menu_entries();

        assert!(app.show_action_menu);
        assert_eq!(app.action_menu_selection, 1);
        assert_eq!(app.status_message, "Choose session action: Handoff");
        assert_eq!(entries[0].action.kind, SessionAvailableActionKind::Resume);
        assert_eq!(entries[0].action.status, SessionActionAvailability::Blocked);
        assert_eq!(entries[1].action.kind, SessionAvailableActionKind::Handoff);
        assert_eq!(
            entries[1].action.status,
            SessionActionAvailability::Available
        );
        assert_eq!(
            entries[2].action.kind,
            SessionAvailableActionKind::LarkExport
        );
        assert_eq!(entries[2].action.status, SessionActionAvailability::Blocked);
        assert_eq!(
            entries[3].action.kind,
            SessionAvailableActionKind::NewSession
        );
        assert_eq!(entries[3].action.status, SessionActionAvailability::Blocked);
    }

    #[test]
    fn exec_resume_mode_preserves_single_ticket_original_resume() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);

        app.queue_original_resume_with_mode(OriginalResumeMode::Exec);

        assert!(app.should_quit());
        assert!(app.take_pending_resume().is_none());
        let Some(TuiExitAction::OriginalResume(plan)) = app.take_exit_action() else {
            panic!("expected original resume action");
        };
        assert_eq!(plan.source_session.id, "codex-cxcp-design");
        assert_eq!(plan.command.display, "codex resume codex-cxcp-design");
        assert_eq!(
            app.status_message,
            "Opening original: Codex codex-cxcp-design"
        );
    }

    #[test]
    fn timeline_e_opens_detail_overlay_without_original_resume() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        app.focus = Focus::Timeline;
        app.selected_event = 2;
        let event_id = app.data.timeline[2].id.clone();

        app.handle_key(key('e'));

        assert!(app.show_timeline_detail);
        assert!(!app.should_quit());
        assert!(app.take_exit_action().is_none());
        assert_eq!(app.selected_event, 2);
        assert_eq!(app.modal_scroll, 0);
        assert_eq!(app.status_message, format!("Timeline detail: {event_id}"));
    }

    #[test]
    fn timeline_detail_builds_and_clears_image_preview_cache() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        app.focus = Focus::Timeline;
        app.selected_event = 0;
        app.data.timeline[0].metadata.attachments = vec![crate::core::model::TimelineAttachment {
            id: Some("img-1".into()),
            name: Some("Image #1".into()),
            mime_type: Some("image/png".into()),
            ..Default::default()
        }];

        app.handle_key(key('e'));

        assert!(app.show_timeline_detail);
        assert_eq!(app.timeline_image_previews.len(), 1);
        assert_eq!(
            app.timeline_image_previews[0].status,
            ImagePreviewStatus::MissingPath
        );

        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::empty()));

        assert!(!app.show_timeline_detail);
        assert!(app.timeline_image_previews.is_empty());
    }

    #[test]
    fn timeline_enter_still_queues_original_resume() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        app.focus = Focus::Timeline;

        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));

        assert!(!app.show_timeline_detail);
        assert!(!app.should_quit());
        assert!(app.take_exit_action().is_none());
        let Some(plan) = app.take_pending_resume() else {
            panic!("expected pending original resume");
        };
        assert_eq!(plan.source_session.id, "codex-cxcp-design");
        assert_eq!(plan.command.display, "codex resume codex-cxcp-design");
    }

    #[test]
    fn timeline_detail_overlay_scrolls_and_closes_without_moving_selection() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        app.focus = Focus::Timeline;
        app.selected_event = 2;
        app.handle_key(key('e'));

        app.handle_key(key('j'));
        assert_eq!(app.selected_event, 2);
        assert_eq!(app.modal_scroll, 1);

        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::empty()));
        assert!(!app.show_timeline_detail);
        assert_eq!(app.modal_scroll, 0);
        assert_eq!(app.status_message, "Timeline detail closed");
    }

    #[test]
    fn action_menu_defaults_to_lark_doc_not_original_resume() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        seed_ready_handoff_artifact(&mut app, "# Generated Handoff\n\nContinue from preview.");
        app.handle_key(key('o'));

        assert!(app.show_action_menu);
        let entries = app.action_menu_entries();
        assert_eq!(entries[0].action.kind, SessionAvailableActionKind::Resume);
        assert_eq!(entries[1].action.kind, SessionAvailableActionKind::Handoff);
        assert_eq!(
            entries[2].action.kind,
            SessionAvailableActionKind::LarkExport
        );
        assert_eq!(
            entries[2].action.status,
            SessionActionAvailability::Available
        );
        assert_eq!(
            entries[3].action.kind,
            SessionAvailableActionKind::NewSession
        );
        assert_eq!(
            entries[3].action.status,
            SessionActionAvailability::Available
        );
        assert_eq!(entries[4].action.kind, SessionAvailableActionKind::Fork);
        assert_eq!(entries[7].action.kind, SessionAvailableActionKind::Yank);
        assert_eq!(
            entries[7].action.status,
            SessionActionAvailability::Available
        );
        assert_eq!(entries[8].action.kind, SessionAvailableActionKind::Archive);
        assert_eq!(
            entries[8].action.status,
            SessionActionAvailability::Available
        );
        assert_eq!(app.action_menu_selection, 2);
        assert_eq!(app.status_message, "Choose session action: Lark Doc");

        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));

        assert!(!app.show_action_menu);
        assert!(!app.should_quit());
        assert!(app.take_exit_action().is_none());
        assert!(app.take_pending_resume().is_none());
        assert!(!app.show_lark_export);
        assert!(app.take_pending_lark_export().is_none());
        if let Some(plan) = app.take_pending_setup_install() {
            assert_eq!(plan.target, setup::SetupInstallTarget::LarkCli);
            assert!(app.status_message.starts_with("Installing "));
            assert!(app.status_message.contains("lark-cli"));
            return;
        }

        assert!(app.show_launch);
        assert!(app.launch_review_lark_export);
        assert!(!app.is_launch_review_pending());
        settle_launch_review(&mut app);
        assert!(app.launch_review);
        assert!(app.launch_review_lark_export);
        assert!(app.data.capsule.handoff_artifact.is_some());
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));

        let Some(plan) = app.take_pending_lark_export() else {
            if let Some(plan) = app.take_pending_setup_install() {
                assert_eq!(plan.target, setup::SetupInstallTarget::LarkCli);
                assert!(app.status_message.starts_with("Installing "));
                assert!(app.status_message.contains("lark-cli"));
                return;
            }
            panic!(
                "expected pending Lark export after reviewed handoff; status={}",
                app.status_message
            );
        };
        assert_eq!(plan.session_id, "codex-cxcp-design");
        assert_eq!(plan.target, CliTool::Hermes);
        assert_eq!(plan.rewind, "evt-091");
        assert_eq!(plan.compiler, "agent:codex:moonbox-handoff");
        assert!(plan.command_display.contains("lark-cli docs +create"));
        assert!(!plan.markdown.trim().is_empty());
        assert!(!plan.markdown.contains("/tmp/"));
        assert_eq!(app.status_message, "Opening Lark Doc");
    }

    #[test]
    fn action_menu_explicit_resume_queues_original_resume() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        app.handle_key(key('o'));
        app.handle_key(key('k'));
        app.handle_key(key('k'));
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));

        assert!(app.show_action_menu);
        assert!(!app.should_quit());
        assert!(app.take_exit_action().is_none());
        assert!(app.take_pending_resume().is_none());
        assert_eq!(
            app.status_message,
            "Resume requires explicit r from Action Menu"
        );

        app.handle_key(key('r'));

        assert!(!app.show_action_menu);
        assert!(!app.should_quit());
        assert!(app.take_exit_action().is_none());
        let Some(plan) = app.take_pending_resume() else {
            panic!("expected pending original resume");
        };
        assert_eq!(plan.source_session.id, "codex-cxcp-design");
        assert_eq!(plan.command.display, "codex resume codex-cxcp-design");
        assert!(plan.dry_run);
    }

    #[test]
    fn action_menu_can_open_handoff_new_session_and_native_fork() {
        let mut handoff = new_app(CliTool::Codex, CliTool::Hermes);
        handoff.handle_key(key('o'));
        handoff.handle_key(key('k'));
        handoff.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));

        assert!(!handoff.show_action_menu);
        assert!(handoff.show_launch);
        assert_eq!(handoff.status_message, "Choose target CLI");
        assert!(handoff.take_pending_resume().is_none());

        let mut lark_export = new_app(CliTool::Codex, CliTool::Hermes);
        seed_ready_handoff_artifact(
            &mut lark_export,
            "# Generated Handoff\n\nWrite this exact preview to Lark.",
        );
        lark_export.handle_key(key('o'));
        lark_export.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));

        assert!(!lark_export.show_action_menu);
        assert!(!lark_export.show_lark_export);
        assert!(lark_export.take_pending_lark_export().is_none());
        assert!(lark_export.take_pending_resume().is_none());
        if let Some(plan) = lark_export.take_pending_setup_install() {
            assert_eq!(plan.target, setup::SetupInstallTarget::LarkCli);
            assert!(lark_export.status_message.starts_with("Installing "));
            assert!(lark_export.status_message.contains("lark-cli"));
        } else {
            assert!(lark_export.show_launch);
            assert!(lark_export.launch_review_lark_export);
            assert!(!lark_export.is_launch_review_pending());
            settle_launch_review(&mut lark_export);
            assert!(lark_export.launch_review);
            assert!(lark_export.launch_review_lark_export);
            lark_export.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
            let Some(plan) = lark_export.take_pending_lark_export() else {
                panic!("expected pending Lark export after reviewed handoff");
            };
            assert_eq!(plan.session_id, "codex-cxcp-design");
            assert_eq!(plan.target, CliTool::Hermes);
            assert!(!plan.markdown.trim().is_empty());
            assert!(!plan.markdown.contains("/tmp/"));
            assert_eq!(lark_export.status_message, "Opening Lark Doc");
        }

        let mut seeded = new_app(CliTool::Codex, CliTool::Hermes);
        seeded.data.timeline[0].metadata.attachments = vec![
            TimelineAttachment {
                id: Some("img-1".into()),
                name: Some("Image #1".into()),
                path: Some("/tmp/moonbox-first.png".into()),
                mime_type: Some("image/png".into()),
                ..Default::default()
            },
            TimelineAttachment {
                id: Some("img-2".into()),
                name: Some("Image #2".into()),
                mime_type: Some("image/png".into()),
                ..Default::default()
            },
        ];
        seeded.handle_key(key('o'));
        seeded.handle_key(key('j'));
        seeded.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));

        assert!(!seeded.show_action_menu);
        assert!(!seeded.show_launch);
        assert!(seeded.take_pending_resume().is_none());
        let Some(plan) = seeded.take_pending_seed_prompt() else {
            panic!("expected pending new session");
        };
        assert_eq!(plan.action, SessionAction::NewSession);
        assert_eq!(plan.source_session.id, "codex-cxcp-design");
        assert_eq!(plan.command.program, "hermes");
        assert_eq!(plan.command.args[0], "chat");
        assert_eq!(plan.command.args[1], "--source");
        assert_eq!(plan.command.args[2], "moonbox");
        assert_eq!(plan.command.args[3], "--query");
        assert!(
            plan.command.args[4]
                .contains("Original first user message attachment path references:")
        );
        assert!(plan.command.args[4].contains("- Image #1: /tmp/moonbox-first.png"));
        assert!(plan.command.args[4].contains("- Image #2: path unavailable in source metadata"));
        assert_eq!(
            seeded.status_message,
            "Starting Hermes from first prompt (1 path, 1 attachment without path): Codex codex-cxcp-design"
        );

        let mut fork = new_app(CliTool::Codex, CliTool::Hermes);
        fork.handle_key(key('o'));
        fork.handle_key(key('j'));
        fork.handle_key(key('j'));
        fork.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));

        assert!(!fork.show_action_menu);
        assert!(!fork.show_launch);
        assert!(fork.take_pending_resume().is_none());
        let Some(plan) = fork.take_pending_native_fork() else {
            panic!("expected pending native fork");
        };
        assert_eq!(plan.action, SessionAction::NativeFork);
        assert_eq!(plan.source_session.id, "codex-cxcp-design");
        assert_eq!(plan.command.args, ["fork", "codex-cxcp-design"]);
        assert_eq!(
            fork.status_message,
            "Suspending to native fork: Codex codex-cxcp-design"
        );
    }

    #[test]
    fn action_menu_opens_share_panel() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        app.handle_key(key('o'));
        for _ in 0..5 {
            app.handle_key(key('j'));
        }
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));

        assert!(!app.show_action_menu);
        assert!(app.show_share_panel);
        assert!(!app.show_launch);
        assert!(app.take_pending_resume().is_none());
        assert_eq!(app.status_message, "Choose yank action");
    }

    #[test]
    fn share_panel_copies_first_user_last_ai_and_session_id() {
        let mut user = new_app(CliTool::Codex, CliTool::Hermes);
        let expected = user.first_user_input().expect("user input").to_string();
        user.handle_key(key('y'));
        user.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));

        assert_eq!(
            user.take_clipboard_text().as_deref(),
            Some(expected.as_str())
        );
        assert_eq!(user.status_message, "Copied first user input");

        let mut ai = new_app(CliTool::Codex, CliTool::Hermes);
        let expected = ai.last_ai_output().expect("assistant output").to_string();
        ai.handle_key(key('y'));
        ai.handle_key(key('j'));
        ai.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));

        assert_eq!(ai.take_clipboard_text().as_deref(), Some(expected.as_str()));
        assert_eq!(ai.status_message, "Copied last AI output");

        let mut session_id = new_app(CliTool::Codex, CliTool::Hermes);
        let expected = session_id.current_session().expect("session").id.clone();
        session_id.handle_key(key('y'));
        session_id.handle_key(key('j'));
        session_id.handle_key(key('j'));
        session_id.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));

        assert_eq!(
            session_id.take_clipboard_text().as_deref(),
            Some(expected.as_str())
        );
        assert_eq!(session_id.status_message, "Copied Session ID");
    }

    #[test]
    fn share_panel_copies_ready_handoff_and_portable_json() {
        let mut handoff = new_app(CliTool::Codex, CliTool::Hermes);
        handoff.data.capsule.handoff_artifact = Some("# Handoff\n\nContinue here.".into());
        handoff.handle_key(key('y'));
        handoff.handle_key(key('j'));
        handoff.handle_key(key('j'));
        handoff.handle_key(key('j'));
        handoff.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));

        assert_eq!(
            handoff.take_clipboard_text().as_deref(),
            Some("# Handoff\n\nContinue here.")
        );
        assert_eq!(handoff.status_message, "Copied handoff text");

        let mut portable = new_app(CliTool::Codex, CliTool::Hermes);
        let session_id = portable.current_session().expect("session").id.clone();
        portable.handle_key(key('y'));
        for _ in 0..4 {
            portable.handle_key(key('j'));
        }
        portable.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));

        let copied = portable.take_clipboard_text().expect("portable json");
        let json: serde_json::Value = serde_json::from_str(&copied).expect("valid json");
        assert_eq!(json["schema"], "moonbox.portable_session.v1");
        assert_eq!(json["source"]["session_id"], session_id);
        assert_eq!(json["timeline"]["loaded"], true);
        assert_eq!(portable.status_message, "Copied portable JSON");
    }

    #[test]
    fn action_menu_archive_uses_tui_overlay_action() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        let session_id = app.current_session().expect("session").id.clone();
        app.handle_key(key('o'));
        for _ in 0..6 {
            app.handle_key(key('j'));
        }
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));

        assert!(!app.show_action_menu);
        assert_eq!(app.status_message, "Archiving session...");
        assert_eq!(
            app.current_session()
                .and_then(|session| app.archive_feedback_for_session(session)),
            Some(ArchiveFeedbackKind::Archive)
        );
        for _ in 0..ARCHIVE_FEEDBACK_FRAMES {
            app.advance_animation();
        }
        assert!(
            !app.visible_session_indices()
                .iter()
                .any(|index| app.data.sessions[*index].id == session_id)
        );
    }

    #[test]
    fn original_copy_and_enter_queue_distinct_actions() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        app.open_original();
        app.handle_key(key('y'));

        assert_eq!(
            app.take_clipboard_text().as_deref(),
            Some("moonbox open --execute --session codex-cxcp-design")
        );
        assert_eq!(app.status_message, "Copied original command");

        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
        assert!(!app.should_quit());
        assert!(!app.show_open_original);
        assert!(app.take_exit_action().is_none());
        let Some(plan) = app.take_pending_resume() else {
            panic!("expected pending original resume");
        };
        assert_eq!(plan.source_session.id, "codex-cxcp-design");
        assert_eq!(plan.command.display, "codex resume codex-cxcp-design");
        assert!(plan.dry_run);
    }

    #[test]
    fn resume_mode_parser_defaults_to_suspend_unless_explicit_exec() {
        assert_eq!(
            parse_original_resume_mode(None),
            OriginalResumeMode::Suspend
        );
        assert_eq!(
            parse_original_resume_mode(Some("")),
            OriginalResumeMode::Suspend
        );
        assert_eq!(
            parse_original_resume_mode(Some("suspend")),
            OriginalResumeMode::Suspend
        );
        assert_eq!(
            parse_original_resume_mode(Some(" exec ")),
            OriginalResumeMode::Exec
        );
        assert_eq!(
            parse_original_resume_mode(Some("EXEC")),
            OriginalResumeMode::Exec
        );
    }

    #[test]
    fn overlay_navigation_scrolls_modal_without_moving_timeline() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        app.focus = Focus::Timeline;
        app.selected_event = 3;
        app.handle_key(key('?'));
        app.handle_key(key('j'));

        assert_eq!(app.selected_event, 3);
        assert_eq!(app.modal_scroll, 1);
    }

    #[test]
    fn doctor_overlay_reports_and_copies_json_without_moving_selection() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        app.focus = Focus::Timeline;
        app.selected_event = 3;

        app.handle_key(key('D'));

        assert!(app.show_doctor);
        assert_eq!(app.selected_event, 3);
        assert!(app.status_message.starts_with("Pre-flight: "));
        assert!(
            app.doctor_report
                .checks
                .iter()
                .any(|check| check.name == "session_discovery")
        );

        app.handle_key(key('y'));
        let copied = app.take_clipboard_text().expect("doctor json");
        let json: serde_json::Value = serde_json::from_str(&copied).expect("valid json");
        assert_eq!(json["version"], 1);
        assert!(json["checks"].as_array().is_some_and(|checks| {
            checks
                .iter()
                .any(|check| check["name"] == "session_discovery")
        }));
        assert_eq!(app.status_message, "Copied pre-flight doctor JSON");
    }

    #[test]
    fn command_mode_opens_doctor_overlay() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);

        app.handle_key(key(':'));
        for ch in "doctor".chars() {
            app.handle_key(key(ch));
        }
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));

        assert!(app.show_doctor);
        assert!(!app.command_mode);
        assert!(app.status_message.starts_with("Pre-flight: "));
    }

    #[test]
    fn command_palette_fuzzy_runs_capsule_review() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);

        app.handle_key(key(':'));
        for ch in "cap".chars() {
            app.handle_key(key(ch));
        }
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));

        assert!(!app.command_mode);
        assert!(app.show_launch);
        assert!(app.launch_review);
        assert_eq!(app.status_message, "Capsule refreshed");
    }

    #[test]
    fn command_palette_resolves_saved_capsule_inventory() {
        let entry = resolve_command_palette_entry("capsule list").expect("capsules command");

        assert_eq!(entry.command, "capsules");
        assert_eq!(entry.badge, "PICKER");
        assert!(entry.description.contains("saved local Capsule"));
    }

    #[test]
    fn command_palette_tab_completes_selected_command() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);

        app.handle_key(key(':'));
        for ch in "sk".chars() {
            app.handle_key(key(ch));
        }
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::empty()));

        assert_eq!(app.command_input, "skill");

        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));

        assert!(!app.command_mode);
        assert!(app.show_skill_picker);
        assert_eq!(app.status_message, "Choose handoff skill");
    }

    #[test]
    fn command_palette_empty_enter_cancels_without_running_first_item() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);

        app.handle_key(key(':'));
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));

        assert!(!app.command_mode);
        assert!(!app.show_open_original);
        assert_eq!(app.status_message, "Command cancelled");
    }

    #[test]
    fn command_palette_vim_selection_runs_selected_command() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);

        app.handle_key(key(':'));
        app.handle_key(key('j'));
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));

        assert!(!app.command_mode);
        assert!(app.show_launch);
        assert!(!app.launch_review);
        assert_eq!(app.status_message, "Choose target CLI");
    }

    #[test]
    fn capsule_panel_scrolls_with_vim_navigation() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        app.focus = Focus::Capsule;

        app.handle_key(key('j'));
        app.handle_key(key('j'));
        app.handle_key(key('k'));

        assert_eq!(app.capsule_scroll, 1);
    }

    #[test]
    fn setup_install_plan_prefers_missing_handoff_skill() {
        let plan = setup_install_plan_for_compiler_error(
            "agent:claude:handoff",
            "skill_not_installed: install a generic handoff skill; runner preflight: sdk_not_found: runner=Claude",
        )
        .expect("setup plan");

        assert_eq!(plan.target, setup::SetupInstallTarget::MattHandoff);
        assert_eq!(plan.label, "Install matt-handoff");
        assert!(plan.command_display.contains("setup install matt-handoff"));
    }

    #[test]
    fn setup_install_plan_maps_runner_sdk_missing_reason() {
        let plan = setup_install_plan_for_compiler_error(
            "agent:claude:handoff",
            "sdk_not_found: runner=Claude; cli=/opt/homebrew/bin/claude; module=claude_agent_sdk",
        )
        .expect("setup plan");

        assert_eq!(plan.target, setup::SetupInstallTarget::ClaudeSdk);
        assert_eq!(plan.label, "Install Claude SDK");
        assert!(plan.command_display.contains("setup install claude-sdk"));
    }

    #[test]
    fn skill_picker_setup_plan_ignores_runner_sdk_missing_reason() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        app.data.compilers = vec!["agent:codex:moonbox-handoff".into()];
        app.pending_compiler = 0;

        assert!(app.pending_skill_setup_install_plan().is_none());
    }

    #[test]
    fn launch_review_error_enter_queues_setup_install() {
        let mut app = new_app(CliTool::Codex, CliTool::Hermes);
        app.show_launch = true;
        app.pending_target = CliTool::Hermes;
        app.set_launch_review_error_for_test(LaunchReviewErrorState {
            target: CliTool::Hermes,
            compiler_id: "agent:claude:handoff".into(),
            message: "invalid compiler config agent:claude:handoff: sdk_not_found: runner=Claude"
                .into(),
            elapsed_ms: 12,
        });

        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));

        let plan = app.take_pending_setup_install().expect("setup install");
        assert_eq!(plan.target, setup::SetupInstallTarget::ClaudeSdk);
        assert!(app.status_message.contains("Install Claude SDK"));
    }
}
