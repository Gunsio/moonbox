use std::{
    collections::BTreeMap,
    env, fs,
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use super::{
    config::{self, HooksConfig},
    error::CoreError,
    model::CliTool,
};

const CLAUDE_EVENTS: &[&str] = &[
    "SessionStart",
    "UserPromptSubmit",
    "PreToolUse",
    "PostToolUse",
    "PermissionRequest",
    "Notification",
    "Stop",
    "SessionEnd",
];
const CODEX_EVENTS: &[&str] = &[
    "session_start",
    "user_prompt_submit",
    "pre_tool_use",
    "permission_request",
    "post_tool_use",
    "stop",
];
const CODEX_CLEANUP_EVENTS: &[&str] = &[
    "session_start",
    "user_prompt_submit",
    "pre_tool_use",
    "permission_request",
    "post_tool_use",
    "stop",
    "SessionStart",
    "UserPromptSubmit",
    "PreToolUse",
    "PermissionRequest",
    "PostToolUse",
    "Stop",
];
const MOONBOX_HOOK_MARKER: &str = "hook-event --cli";
const HOOK_LIVE_STALE_AFTER_MS: u128 = 5 * 60 * 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookProvider {
    Claude,
    Codex,
}

impl HookProvider {
    pub fn id(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }

    pub fn display(self) -> &'static str {
        match self {
            Self::Claude => "Claude",
            Self::Codex => "Codex",
        }
    }

    fn config_path(self) -> Option<PathBuf> {
        match self {
            Self::Claude => claude_home().map(|home| home.join("settings.json")),
            Self::Codex => codex_home().map(|home| home.join("hooks.json")),
        }
    }

    fn events(self) -> &'static [&'static str] {
        match self {
            Self::Claude => CLAUDE_EVENTS,
            Self::Codex => CODEX_EVENTS,
        }
    }

    fn cleanup_events(self) -> &'static [&'static str] {
        match self {
            Self::Claude => CLAUDE_EVENTS,
            Self::Codex => CODEX_CLEANUP_EVENTS,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookAction {
    Install,
    Uninstall,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookFileAction {
    Create,
    Update,
    Noop,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookSpoolReport {
    pub path: String,
    pub exists: bool,
    pub bytes: u64,
    pub max_bytes: u64,
    pub max_files: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookProviderReport {
    pub provider: HookProvider,
    pub config_path: Option<String>,
    pub config_exists: bool,
    pub config_valid: bool,
    pub installed: bool,
    pub moonbox_entry_count: usize,
    pub feature_enabled: Option<bool>,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HooksStatusReport {
    pub version: u8,
    pub moonbox_enabled: bool,
    pub smart_enter_tmux_enabled: bool,
    pub moonbox_config_path: Option<String>,
    pub spool: HookSpoolReport,
    pub providers: Vec<HookProviderReport>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookProviderChange {
    pub provider: HookProvider,
    pub config_path: Option<String>,
    pub action: HookFileAction,
    pub changed: bool,
    pub before_entries: usize,
    pub after_entries: usize,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HooksApplyReport {
    pub version: u8,
    pub action: HookAction,
    pub dry_run: bool,
    pub moonbox_enabled_before: bool,
    pub moonbox_enabled_after: bool,
    pub smart_enter_tmux_enabled: bool,
    pub moonbox_config_path: Option<String>,
    pub spool: HookSpoolReport,
    pub providers: Vec<HookProviderChange>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookEventKind {
    SessionStart,
    UserPromptSubmit,
    PreToolUse,
    PostToolUse,
    Stop,
    PermissionRequest,
    Notification,
    SessionEnd,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookSessionStatus {
    Running,
    Waiting,
    Idle,
    Dead,
}

impl HookSessionStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Running => "RUN",
            Self::Waiting => "WAIT",
            Self::Idle => "IDLE",
            Self::Dead => "END",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookSpoolEvent {
    pub cli: CliTool,
    pub session_id: String,
    pub transcript_path: Option<String>,
    pub cwd: Option<String>,
    pub tmux: Option<String>,
    pub tmux_pane: Option<String>,
    pub captured_at_ms: u128,
    pub event_name: String,
    pub kind: HookEventKind,
    pub summary: String,
    pub wait_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookSessionLiveInfo {
    pub cli: CliTool,
    pub session_id: String,
    pub transcript_path: Option<String>,
    pub cwd: Option<String>,
    pub tmux: Option<String>,
    pub tmux_pane: Option<String>,
    pub status: HookSessionStatus,
    pub summary: String,
    pub wait_reason: Option<String>,
    pub status_since_ms: u128,
    pub updated_at_ms: u128,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookSpoolRead {
    pub next_offset: u64,
    pub events: Vec<HookSpoolEvent>,
    pub skipped_lines: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookLiveIndicator {
    pub label: String,
    pub is_error: bool,
    pub is_stale: bool,
}

#[derive(Debug, Clone)]
pub struct HookLiveState {
    spool_path: PathBuf,
    offset: u64,
    sessions: BTreeMap<String, HookSessionLiveInfo>,
    last_event_at_ms: Option<u128>,
    last_error: Option<String>,
    skipped_lines: usize,
}

pub fn status_report() -> HooksStatusReport {
    let hooks_config = config::load_hooks_config();
    let spool = spool_report(&hooks_config);
    let providers = [HookProvider::Claude, HookProvider::Codex]
        .into_iter()
        .map(provider_report)
        .collect();

    HooksStatusReport {
        version: 1,
        moonbox_enabled: hooks_config.enabled,
        smart_enter_tmux_enabled: hooks_config.smart_enter_tmux,
        moonbox_config_path: config::config_path().map(|path| path.display().to_string()),
        spool,
        providers,
        notes: status_notes(),
    }
}

pub fn apply(
    action: HookAction,
    providers: &[HookProvider],
    apply: bool,
) -> Result<HooksApplyReport, CoreError> {
    let before_config = config::load_hooks_config();
    let mut after_config = before_config.clone();
    after_config.enabled = action == HookAction::Install;
    let preview_changes = providers
        .iter()
        .copied()
        .map(|provider| provider_change(action, provider, false))
        .collect::<Result<Vec<_>, _>>()?;
    let provider_changes = if apply {
        config::save_hooks_config(after_config.clone()).map_err(|error| CoreError::Hooks {
            reason: format!("cannot save Moonbox hooks config: {error}"),
        })?;
        providers
            .iter()
            .copied()
            .map(|provider| provider_change(action, provider, true))
            .collect::<Result<Vec<_>, _>>()?
    } else {
        preview_changes
    };

    Ok(HooksApplyReport {
        version: 1,
        action,
        dry_run: !apply,
        moonbox_enabled_before: before_config.enabled,
        moonbox_enabled_after: after_config.enabled,
        smart_enter_tmux_enabled: after_config.smart_enter_tmux,
        moonbox_config_path: config::config_path().map(|path| path.display().to_string()),
        spool: spool_report(&after_config),
        providers: provider_changes,
        notes: apply_notes(action),
    })
}

pub fn capture_event(provider: HookProvider) {
    let hooks_config = config::load_hooks_config();
    if !hooks_config.enabled {
        return;
    }
    let mut input = String::new();
    if io::stdin().read_to_string(&mut input).is_err() {
        input.clear();
    }
    let event = serde_json::from_str::<Value>(&input).unwrap_or(Value::Null);
    let cwd = event
        .get("cwd")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| {
            env::current_dir()
                .ok()
                .map(|path| path.display().to_string())
        });
    let captured = json!({
        "version": 1,
        "cli": provider.id(),
        "captured_at_ms": now_millis(),
        "hook_event_name": event.get("hook_event_name").and_then(Value::as_str),
        "session_id": event.get("session_id").and_then(Value::as_str),
        "transcript_path": event.get("transcript_path").and_then(Value::as_str),
        "cwd": cwd,
        "tmux": env::var("TMUX").ok(),
        "tmux_pane": env::var("TMUX_PANE").ok(),
        "event": event,
    });
    if let Ok(line) = serde_json::to_string(&captured) {
        let spool = spool_path(&hooks_config);
        let _ = append_spool_line(
            &spool,
            &line,
            hooks_config.spool_max_bytes,
            hooks_config.spool_max_files,
        );
    }
}

pub fn default_providers() -> Vec<HookProvider> {
    vec![HookProvider::Claude, HookProvider::Codex]
}

#[cfg_attr(test, allow(dead_code))]
pub fn live_state_from_config(config: &HooksConfig) -> Option<HookLiveState> {
    config
        .enabled
        .then(|| HookLiveState::new(spool_path(config)))
}

pub fn current_millis() -> u128 {
    now_millis()
}

pub fn read_spool_events(path: &Path, offset: u64) -> io::Result<HookSpoolRead> {
    let mut file = fs::OpenOptions::new().read(true).open(path)?;
    let len = file.metadata()?.len();
    let start = offset.min(len);
    file.seek(SeekFrom::Start(start))?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    let mut events = Vec::new();
    let mut skipped_lines = 0;
    for line in contents.lines().filter(|line| !line.trim().is_empty()) {
        match parse_spool_event(line) {
            Ok(event) => events.push(event),
            Err(_) => skipped_lines += 1,
        }
    }
    Ok(HookSpoolRead {
        next_offset: start.saturating_add(contents.len() as u64),
        events,
        skipped_lines,
    })
}

pub fn parse_spool_event(line: &str) -> Result<HookSpoolEvent, String> {
    let value = serde_json::from_str::<Value>(line).map_err(|error| error.to_string())?;
    event_from_value(&value)
}

impl HookLiveState {
    pub fn new(spool_path: PathBuf) -> Self {
        Self {
            spool_path,
            offset: 0,
            sessions: BTreeMap::new(),
            last_event_at_ms: None,
            last_error: None,
            skipped_lines: 0,
        }
    }

    pub fn replay_existing(&mut self) -> bool {
        self.offset = 0;
        self.poll()
    }

    pub fn poll(&mut self) -> bool {
        match read_spool_events(&self.spool_path, self.offset) {
            Ok(read) => {
                let changed =
                    !read.events.is_empty() || read.skipped_lines > 0 || self.last_error.is_some();
                self.offset = read.next_offset;
                self.skipped_lines = self.skipped_lines.saturating_add(read.skipped_lines);
                self.last_error = None;
                for event in read.events {
                    self.apply_event(event);
                }
                changed
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let changed = self.last_error.is_some();
                self.last_error = None;
                changed
            }
            Err(error) => {
                let reason = error.to_string();
                let changed = self.last_error.as_deref() != Some(reason.as_str());
                self.last_error = Some(reason);
                changed
            }
        }
    }

    pub fn indicator(&self, now_ms: u128) -> HookLiveIndicator {
        if let Some(error) = &self.last_error {
            return HookLiveIndicator {
                label: format!("Live error: {error}"),
                is_error: true,
                is_stale: false,
            };
        }
        match self.last_event_at_ms {
            Some(last) if now_ms.saturating_sub(last) > HOOK_LIVE_STALE_AFTER_MS => {
                HookLiveIndicator {
                    label: format!("Live stale: {}", age_label_ms(now_ms.saturating_sub(last))),
                    is_error: false,
                    is_stale: true,
                }
            }
            Some(_) => HookLiveIndicator {
                label: "Live on".into(),
                is_error: false,
                is_stale: false,
            },
            None => HookLiveIndicator {
                label: "Live on: no events".into(),
                is_error: false,
                is_stale: false,
            },
        }
    }

    pub fn session_for(
        &self,
        cli: CliTool,
        session_id: &str,
        source_path: Option<&str>,
    ) -> Option<&HookSessionLiveInfo> {
        self.sessions
            .get(&session_key(cli, session_id))
            .or_else(|| {
                source_path.and_then(|path| {
                    self.sessions.values().find(|session| {
                        session.cli == cli && session.transcript_path.as_deref() == Some(path)
                    })
                })
            })
    }

    pub fn waiting_sessions(&self) -> Vec<&HookSessionLiveInfo> {
        let mut sessions = self
            .sessions
            .values()
            .filter(|session| session.status == HookSessionStatus::Waiting)
            .collect::<Vec<_>>();
        sessions.sort_by(|left, right| {
            left.status_since_ms
                .cmp(&right.status_since_ms)
                .then_with(|| left.session_id.cmp(&right.session_id))
        });
        sessions
    }

    fn apply_event(&mut self, event: HookSpoolEvent) {
        self.last_event_at_ms = Some(event.captured_at_ms);
        let key = session_key(event.cli, &event.session_id);
        let status = status_for_event(event.kind);
        if let Some(existing) = self.sessions.get_mut(&key) {
            let status_changed = existing.status != status;
            existing.transcript_path = event.transcript_path.or(existing.transcript_path.take());
            existing.cwd = event.cwd.or(existing.cwd.take());
            existing.tmux = event.tmux.or(existing.tmux.take());
            existing.tmux_pane = event.tmux_pane.or(existing.tmux_pane.take());
            existing.status = status;
            existing.summary = event.summary;
            existing.wait_reason = event.wait_reason;
            if status_changed {
                existing.status_since_ms = event.captured_at_ms;
            }
            existing.updated_at_ms = event.captured_at_ms;
        } else {
            self.sessions.insert(
                key,
                HookSessionLiveInfo {
                    cli: event.cli,
                    session_id: event.session_id,
                    transcript_path: event.transcript_path,
                    cwd: event.cwd,
                    tmux: event.tmux,
                    tmux_pane: event.tmux_pane,
                    status,
                    summary: event.summary,
                    wait_reason: event.wait_reason,
                    status_since_ms: event.captured_at_ms,
                    updated_at_ms: event.captured_at_ms,
                },
            );
        }
    }

    #[cfg(test)]
    pub(crate) fn apply_event_for_test(&mut self, event: HookSpoolEvent) {
        self.apply_event(event);
    }
}

pub fn age_label_ms(age_ms: u128) -> String {
    let seconds = age_ms / 1000;
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 60 * 60 {
        format!("{}m", seconds / 60)
    } else {
        format!("{}h", seconds / 3600)
    }
}

fn provider_report(provider: HookProvider) -> HookProviderReport {
    let Some(path) = provider.config_path() else {
        return HookProviderReport {
            provider,
            config_path: None,
            config_exists: false,
            config_valid: false,
            installed: false,
            moonbox_entry_count: 0,
            feature_enabled: (provider == HookProvider::Codex).then_some(true),
            reason: "HOME is unavailable".into(),
        };
    };
    let feature_enabled = (provider == HookProvider::Codex).then(codex_hooks_feature_enabled);
    let path_display = path.display().to_string();
    if !path.exists() {
        return HookProviderReport {
            provider,
            config_path: Some(path_display),
            config_exists: false,
            config_valid: true,
            installed: false,
            moonbox_entry_count: 0,
            feature_enabled,
            reason: "not installed".into(),
        };
    }
    match read_json_config(&path) {
        Ok(value) => {
            let count = count_moonbox_entries(provider, &value);
            let mut reason = if count == 0 {
                "not installed".to_string()
            } else {
                format!("{count} Moonbox hook entries installed")
            };
            if provider == HookProvider::Codex && feature_enabled == Some(false) {
                reason.push_str("; Codex [features].hooks=false disables hook execution");
            }
            HookProviderReport {
                provider,
                config_path: Some(path_display),
                config_exists: true,
                config_valid: true,
                installed: count > 0,
                moonbox_entry_count: count,
                feature_enabled,
                reason,
            }
        }
        Err(error) => HookProviderReport {
            provider,
            config_path: Some(path_display),
            config_exists: true,
            config_valid: false,
            installed: false,
            moonbox_entry_count: 0,
            feature_enabled,
            reason: error,
        },
    }
}

fn event_from_value(value: &Value) -> Result<HookSpoolEvent, String> {
    let event = value.get("event").unwrap_or(value);
    let cli = text_field(value, "cli")
        .or_else(|| text_field(event, "cli"))
        .and_then(|name| parse_cli(&name))
        .ok_or_else(|| "missing supported cli".to_string())?;
    let event_name = text_field(value, "hook_event_name")
        .or_else(|| text_field(event, "hook_event_name"))
        .or_else(|| text_field(event, "subtype"))
        .or_else(|| text_field(event, "type"))
        .unwrap_or_else(|| "unknown".into());
    let kind = normalize_event_kind(&event_name);
    let session_id = text_field(value, "session_id")
        .or_else(|| text_field(event, "session_id"))
        .or_else(|| nested_text(event, &["session", "id"]))
        .ok_or_else(|| "missing session_id".to_string())?;
    let captured_at_ms = value
        .get("captured_at_ms")
        .and_then(Value::as_u64)
        .map(u128::from)
        .or_else(|| {
            value
                .get("captured_at_ms")
                .and_then(Value::as_str)
                .and_then(|value| value.parse::<u128>().ok())
        })
        .unwrap_or_else(now_millis);
    let wait_reason = wait_reason_for_event(kind, event);
    let summary = summary_for_event(kind, event, wait_reason.as_deref());

    Ok(HookSpoolEvent {
        cli,
        session_id,
        transcript_path: text_field(value, "transcript_path")
            .or_else(|| text_field(event, "transcript_path")),
        cwd: text_field(value, "cwd").or_else(|| text_field(event, "cwd")),
        tmux: text_field(value, "tmux"),
        tmux_pane: text_field(value, "tmux_pane"),
        captured_at_ms,
        event_name,
        kind,
        summary,
        wait_reason,
    })
}

fn parse_cli(value: &str) -> Option<CliTool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "claude" | "claude_code" | "claude-code" => Some(CliTool::Claude),
        "codex" => Some(CliTool::Codex),
        _ => None,
    }
}

fn normalize_event_kind(name: &str) -> HookEventKind {
    match name
        .chars()
        .filter(|ch| *ch != '_' && *ch != '-')
        .collect::<String>()
        .to_ascii_lowercase()
        .as_str()
    {
        "sessionstart" => HookEventKind::SessionStart,
        "userpromptsubmit" => HookEventKind::UserPromptSubmit,
        "pretooluse" => HookEventKind::PreToolUse,
        "posttooluse" => HookEventKind::PostToolUse,
        "stop" => HookEventKind::Stop,
        "permissionrequest" => HookEventKind::PermissionRequest,
        "notification" => HookEventKind::Notification,
        "sessionend" => HookEventKind::SessionEnd,
        _ => HookEventKind::Unknown,
    }
}

fn status_for_event(kind: HookEventKind) -> HookSessionStatus {
    match kind {
        HookEventKind::PermissionRequest | HookEventKind::Notification => {
            HookSessionStatus::Waiting
        }
        HookEventKind::Stop => HookSessionStatus::Idle,
        HookEventKind::SessionEnd => HookSessionStatus::Dead,
        HookEventKind::SessionStart
        | HookEventKind::UserPromptSubmit
        | HookEventKind::PreToolUse
        | HookEventKind::PostToolUse
        | HookEventKind::Unknown => HookSessionStatus::Running,
    }
}

fn wait_reason_for_event(kind: HookEventKind, event: &Value) -> Option<String> {
    match kind {
        HookEventKind::PermissionRequest => {
            let detail = event_detail(event)
                .or_else(|| tool_name(event))
                .unwrap_or_else(|| "approval required".into());
            Some(format!("Approval: {}", compact_text(&detail, 56)))
        }
        HookEventKind::Notification => {
            let detail = event_detail(event).unwrap_or_else(|| "agent notification".into());
            Some(format!("Notification: {}", compact_text(&detail, 56)))
        }
        _ => None,
    }
}

fn summary_for_event(kind: HookEventKind, event: &Value, wait_reason: Option<&str>) -> String {
    match kind {
        HookEventKind::SessionStart => "Session started".into(),
        HookEventKind::UserPromptSubmit => "User prompt submitted".into(),
        HookEventKind::PreToolUse => tool_action_summary("Running", event),
        HookEventKind::PostToolUse => tool_action_summary("Finished", event),
        HookEventKind::Stop => "Idle after stop".into(),
        HookEventKind::PermissionRequest | HookEventKind::Notification => {
            wait_reason.unwrap_or("Waiting on you").into()
        }
        HookEventKind::SessionEnd => "Session ended".into(),
        HookEventKind::Unknown => {
            let detail = event_detail(event).unwrap_or_else(|| "Hook event".into());
            compact_text(&detail, 64)
        }
    }
}

fn tool_action_summary(prefix: &str, event: &Value) -> String {
    let tool = tool_name(event).unwrap_or_else(|| "tool".into());
    let mut summary = format!("{prefix} {tool}");
    if let Some(target) = tool_target(event) {
        summary.push(' ');
        summary.push_str(&compact_text(&target, 40));
    }
    compact_text(&summary, 64)
}

fn tool_name(event: &Value) -> Option<String> {
    text_field(event, "tool_name")
        .or_else(|| text_field(event, "toolName"))
        .or_else(|| match event.get("tool") {
            Some(Value::String(value)) => non_empty(value),
            Some(value) => text_field(value, "name"),
            None => None,
        })
        .or_else(|| nested_text(event, &["tool_use", "name"]))
        .or_else(|| nested_text(event, &["tool_input", "name"]))
}

fn tool_target(event: &Value) -> Option<String> {
    nested_text(event, &["tool_input", "file_path"])
        .or_else(|| nested_text(event, &["tool_input", "path"]))
        .or_else(|| nested_text(event, &["tool_input", "command"]))
        .or_else(|| text_field(event, "file_path"))
        .or_else(|| text_field(event, "command"))
}

fn event_detail(event: &Value) -> Option<String> {
    nested_text(event, &["message", "content"])
        .or_else(|| text_field(event, "message"))
        .or_else(|| text_field(event, "reason"))
        .or_else(|| text_field(event, "detail"))
        .or_else(|| nested_text(event, &["notification", "message"]))
        .or_else(|| nested_text(event, &["permission", "reason"]))
}

fn text_field(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).and_then(non_empty)
}

fn nested_text(value: &Value, path: &[&str]) -> Option<String> {
    let mut cursor = value;
    for key in path {
        cursor = cursor.get(*key)?;
    }
    cursor.as_str().and_then(non_empty)
}

fn non_empty(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn compact_text(value: &str, max_chars: usize) -> String {
    let collapsed = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= max_chars {
        return collapsed;
    }
    let keep = max_chars.saturating_sub(3);
    format!("{}...", collapsed.chars().take(keep).collect::<String>())
}

fn session_key(cli: CliTool, session_id: &str) -> String {
    format!("{}:{session_id}", cli.id())
}

fn provider_change(
    action: HookAction,
    provider: HookProvider,
    apply: bool,
) -> Result<HookProviderChange, CoreError> {
    let Some(path) = provider.config_path() else {
        return Ok(HookProviderChange {
            provider,
            config_path: None,
            action: HookFileAction::Error,
            changed: false,
            before_entries: 0,
            after_entries: 0,
            error: Some("HOME is unavailable".into()),
        });
    };
    let existed = path.exists();
    let mut config = if existed {
        read_json_config(&path).map_err(|error| CoreError::Hooks {
            reason: format!("{} cannot be read as JSON: {error}", path.display()),
        })?
    } else {
        Value::Object(Map::new())
    };
    let before_entries = count_moonbox_entries(provider, &config);
    let changed = match action {
        HookAction::Install => install_provider_entries(provider, &mut config)?,
        HookAction::Uninstall => uninstall_provider_entries(provider, &mut config)?,
    };
    let after_entries = count_moonbox_entries(provider, &config);
    let file_action = if !changed {
        HookFileAction::Noop
    } else if existed {
        HookFileAction::Update
    } else {
        HookFileAction::Create
    };
    if changed && apply {
        write_json_config(&path, &config)?;
    }
    Ok(HookProviderChange {
        provider,
        config_path: Some(path.display().to_string()),
        action: file_action,
        changed,
        before_entries,
        after_entries,
        error: None,
    })
}

fn install_provider_entries(provider: HookProvider, config: &mut Value) -> Result<bool, CoreError> {
    let command = hook_command(provider);
    let object = config.as_object_mut().ok_or_else(|| CoreError::Hooks {
        reason: format!(
            "{} hooks config root must be a JSON object",
            provider.display()
        ),
    })?;
    let hooks = object
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()));
    let hooks = hooks.as_object_mut().ok_or_else(|| CoreError::Hooks {
        reason: format!("{} hooks field must be a JSON object", provider.display()),
    })?;
    let mut changed = false;
    for event in provider.events() {
        let groups = hooks
            .entry((*event).to_string())
            .or_insert_with(|| Value::Array(Vec::new()));
        let groups = groups.as_array_mut().ok_or_else(|| CoreError::Hooks {
            reason: format!("{} hooks.{event} must be a JSON array", provider.display()),
        })?;
        if groups
            .iter()
            .any(|group| group_has_moonbox_handler(provider, group))
        {
            continue;
        }
        groups.push(json!({
            "hooks": [
                {
                    "type": "command",
                    "command": command,
                    "timeout": 5
                }
            ]
        }));
        changed = true;
    }
    Ok(changed)
}

fn uninstall_provider_entries(
    provider: HookProvider,
    config: &mut Value,
) -> Result<bool, CoreError> {
    let Some(object) = config.as_object_mut() else {
        return Err(CoreError::Hooks {
            reason: format!(
                "{} hooks config root must be a JSON object",
                provider.display()
            ),
        });
    };
    let Some(hooks) = object.get_mut("hooks").and_then(Value::as_object_mut) else {
        return Ok(false);
    };
    let mut changed = false;
    for event in provider.cleanup_events() {
        let Some(groups) = hooks.get_mut(*event).and_then(Value::as_array_mut) else {
            continue;
        };
        let before_group_count = groups.len();
        for group in groups.iter_mut() {
            if let Some(handlers) = group.get_mut("hooks").and_then(Value::as_array_mut) {
                let before_handler_count = handlers.len();
                handlers.retain(|handler| !is_moonbox_handler(provider, handler));
                changed |= handlers.len() != before_handler_count;
            }
        }
        groups.retain(|group| {
            group
                .get("hooks")
                .and_then(Value::as_array)
                .is_none_or(|handlers| !handlers.is_empty())
        });
        changed |= groups.len() != before_group_count;
    }
    let empty_events = hooks
        .iter()
        .filter(|(_, value)| value.as_array().is_some_and(|groups| groups.is_empty()))
        .map(|(event, _)| event.clone())
        .collect::<Vec<_>>();
    for event in empty_events {
        hooks.remove(&event);
    }
    Ok(changed)
}

fn hook_command(provider: HookProvider) -> String {
    let binary = env::current_exe()
        .ok()
        .map(|path| path.display().to_string())
        .filter(|path| !path.trim().is_empty())
        .unwrap_or_else(|| "moonbox".into());
    format!(
        "{} hook-event --cli {}",
        shell_escape(&binary),
        provider.id()
    )
}

fn shell_escape(value: &str) -> String {
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '_' | '-' | ':'))
    {
        return value.into();
    }
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn read_json_config(path: &Path) -> Result<Value, String> {
    let contents = fs::read_to_string(path).map_err(|error| error.to_string())?;
    if contents.trim().is_empty() {
        return Ok(Value::Object(Map::new()));
    }
    serde_json::from_str(&contents).map_err(|error| error.to_string())
}

fn write_json_config(path: &Path, value: &Value) -> Result<(), CoreError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| CoreError::Hooks {
            reason: format!("cannot create {}: {error}", parent.display()),
        })?;
    }
    fs::write(
        path,
        serde_json::to_string_pretty(value).map_err(|error| CoreError::Hooks {
            reason: format!("cannot serialize {}: {error}", path.display()),
        })?,
    )
    .map_err(|error| CoreError::Hooks {
        reason: format!("cannot write {}: {error}", path.display()),
    })
}

fn count_moonbox_entries(provider: HookProvider, config: &Value) -> usize {
    config
        .get("hooks")
        .and_then(Value::as_object)
        .map(|hooks| {
            provider
                .cleanup_events()
                .iter()
                .filter_map(|event| hooks.get(*event).and_then(Value::as_array))
                .flat_map(|groups| groups.iter())
                .filter_map(|group| group.get("hooks").and_then(Value::as_array))
                .flat_map(|handlers| handlers.iter())
                .filter(|handler| is_moonbox_handler(provider, handler))
                .count()
        })
        .unwrap_or(0)
}

fn group_has_moonbox_handler(provider: HookProvider, group: &Value) -> bool {
    group
        .get("hooks")
        .and_then(Value::as_array)
        .is_some_and(|handlers| {
            handlers
                .iter()
                .any(|handler| is_moonbox_handler(provider, handler))
        })
}

fn is_moonbox_handler(provider: HookProvider, handler: &Value) -> bool {
    handler
        .get("type")
        .and_then(Value::as_str)
        .is_some_and(|kind| kind == "command")
        && handler
            .get("command")
            .and_then(Value::as_str)
            .is_some_and(|command| {
                command.contains(MOONBOX_HOOK_MARKER)
                    && (command.contains(&format!("--cli {}", provider.id()))
                        || command.contains(&format!("--cli={}", provider.id())))
            })
}

fn spool_report(config: &HooksConfig) -> HookSpoolReport {
    let path = spool_path(config);
    let bytes = fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
    HookSpoolReport {
        path: path.display().to_string(),
        exists: path.exists(),
        bytes,
        max_bytes: config.spool_max_bytes,
        max_files: config.spool_max_files,
    }
}

fn spool_path(config: &HooksConfig) -> PathBuf {
    if let Some(path) = env::var_os("MOONBOX_HOOK_SPOOL") {
        return PathBuf::from(path);
    }
    if let Some(path) = config
        .spool_path
        .as_deref()
        .filter(|path| !path.trim().is_empty())
    {
        return expand_home(path);
    }
    moonbox_home()
        .unwrap_or_else(|| env::temp_dir().join("moonbox"))
        .join("spool")
        .join("events.jsonl")
}

fn moonbox_home() -> Option<PathBuf> {
    env::var_os("MOONBOX_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".moonbox")))
}

fn claude_home() -> Option<PathBuf> {
    env::var_os("MOONBOX_CLAUDE_HOME")
        .or_else(|| env::var_os("CLAUDE_HOME"))
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".claude")))
}

fn codex_home() -> Option<PathBuf> {
    env::var_os("MOONBOX_CODEX_HOME")
        .or_else(|| env::var_os("CODEX_HOME"))
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".codex")))
}

fn expand_home(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/")
        && let Some(home) = env::var_os("HOME")
    {
        return PathBuf::from(home).join(rest);
    }
    PathBuf::from(path)
}

fn codex_hooks_feature_enabled() -> bool {
    let Some(path) = codex_home().map(|home| home.join("config.toml")) else {
        return true;
    };
    let Ok(contents) = fs::read_to_string(path) else {
        return true;
    };
    let mut in_features = false;
    for line in contents.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.starts_with('[') && line.ends_with(']') {
            in_features = line == "[features]";
            continue;
        }
        if !in_features {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if key != "hooks" && key != "codex_hooks" {
            continue;
        }
        return !value.trim().starts_with("false");
    }
    true
}

fn append_spool_line(path: &Path, line: &str, max_bytes: u64, max_files: usize) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let next_len = line.len() as u64 + 1;
    if fs::metadata(path)
        .map(|meta| meta.len() > 0 && meta.len().saturating_add(next_len) > max_bytes)
        .unwrap_or(false)
    {
        rotate_spool(path, max_files)?;
    }
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(file, "{line}")?;
    Ok(())
}

fn rotate_spool(path: &Path, max_files: usize) -> io::Result<()> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("events");
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("jsonl");
    let rotated = parent.join(format!("{stem}.{}.{}", now_millis(), extension));
    fs::rename(path, rotated)?;
    prune_rotations(parent, stem, extension, max_files)
}

fn prune_rotations(parent: &Path, stem: &str, extension: &str, max_files: usize) -> io::Result<()> {
    let mut rotations = fs::read_dir(parent)?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            let file_name = path.file_name()?.to_str()?;
            (file_name.starts_with(&format!("{stem}."))
                && file_name.ends_with(&format!(".{extension}")))
            .then_some(path)
        })
        .collect::<Vec<_>>();
    rotations.sort();
    while rotations.len() > max_files {
        if let Some(path) = rotations.first() {
            let _ = fs::remove_file(path);
        }
        rotations.remove(0);
    }
    Ok(())
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

fn status_notes() -> Vec<String> {
    vec![
        "Hooks are opt-in and disabled until `moonbox hooks install --apply` writes Moonbox and provider config.".into(),
        "Provider hooks affect only new Claude/Codex sessions started after installation.".into(),
        "Codex command hooks must still be reviewed and trusted from Codex `/hooks`; Moonbox never writes Codex trust state.".into(),
        "When hooks are enabled, the TUI can replay/tail the Moonbox spool for live badges and the waiting queue; Smart Enter / tmux jump is still a separate Settings opt-in.".into(),
    ]
}

fn apply_notes(action: HookAction) -> Vec<String> {
    let verb = match action {
        HookAction::Install => "installed",
        HookAction::Uninstall => "removed",
    };
    vec![
        format!("Dry-run is the default; this report is only applied when dry_run=false. Moonbox hooks {verb} only Moonbox-owned entries."),
        "Restart or open new Claude/Codex sessions after changing hooks; already running sessions keep their startup snapshot.".into(),
        "Codex may require `/hooks` review before newly configured command hooks run.".into(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_is_idempotent_and_preserves_existing_hooks() {
        let mut value = json!({
            "hooks": {
                "PreToolUse": [
                    {"matcher": "Bash", "hooks": [{"type": "command", "command": "/bin/echo existing"}]}
                ]
            },
            "theme": "keep"
        });

        assert!(install_provider_entries(HookProvider::Claude, &mut value).expect("install"));
        assert!(!install_provider_entries(HookProvider::Claude, &mut value).expect("idempotent"));

        assert_eq!(value["theme"], "keep");
        assert_eq!(
            count_moonbox_entries(HookProvider::Claude, &value),
            CLAUDE_EVENTS.len()
        );
        let pre_tool_hooks = value["hooks"]["PreToolUse"]
            .as_array()
            .expect("pre tool groups");
        assert!(
            pre_tool_hooks
                .iter()
                .any(|group| group["hooks"][0]["command"] == "/bin/echo existing")
        );
    }

    #[test]
    fn uninstall_removes_only_moonbox_handlers() {
        let mut value = json!({"hooks": {}});
        install_provider_entries(HookProvider::Codex, &mut value).expect("install");
        value["hooks"]["stop"]
            .as_array_mut()
            .expect("stop groups")
            .push(json!({"hooks": [{"type": "command", "command": "/bin/echo keep"}]}));
        value["hooks"]["Stop"] = json!([
            {"hooks": [{"type": "command", "command": "moonbox hook-event --cli codex"}]}
        ]);

        assert!(uninstall_provider_entries(HookProvider::Codex, &mut value).expect("uninstall"));

        assert_eq!(count_moonbox_entries(HookProvider::Codex, &value), 0);
        assert!(
            value["hooks"]
                .as_object()
                .expect("hooks")
                .get("Stop")
                .is_none()
        );
        assert!(
            value["hooks"]["stop"]
                .as_array()
                .expect("stop groups")
                .iter()
                .any(|group| group["hooks"][0]["command"] == "/bin/echo keep")
        );
    }

    #[test]
    fn append_spool_line_rotates_by_size() {
        let root = env::temp_dir().join(format!("moonbox-hooks-spool-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("spool root");
        let path = root.join("events.jsonl");
        append_spool_line(&path, r#"{"event":1}"#, 16, 1).expect("append one");
        append_spool_line(&path, r#"{"event":2}"#, 16, 1).expect("append two");

        let current = fs::read_to_string(&path).expect("current spool");
        assert!(current.contains(r#""event":2"#));
        let rotations = fs::read_dir(&root)
            .expect("read rotations")
            .filter_map(Result::ok)
            .filter(|entry| {
                let file_name = entry.file_name().to_string_lossy().to_string();
                file_name != "events.jsonl" && file_name.starts_with("events.")
            })
            .count();
        assert_eq!(rotations, 1);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn parse_spool_event_accepts_codex_snake_case_and_summarizes_tool() {
        let event = parse_spool_event(
            r#"{"version":1,"cli":"codex","captured_at_ms":1780000000000,"hook_event_name":"pre_tool_use","session_id":"s1","transcript_path":"/tmp/s1.jsonl","tmux_pane":"%7","event":{"tool_name":"Edit","tool_input":{"file_path":"src/app.rs"}}}"#,
        )
        .expect("event");

        assert_eq!(event.cli, CliTool::Codex);
        assert_eq!(event.kind, HookEventKind::PreToolUse);
        assert_eq!(event.session_id, "s1");
        assert_eq!(event.transcript_path.as_deref(), Some("/tmp/s1.jsonl"));
        assert_eq!(event.tmux_pane.as_deref(), Some("%7"));
        assert_eq!(event.summary, "Running Edit src/app.rs");
    }

    #[test]
    fn live_state_tracks_waiting_queue_and_dequeues_on_followup_event() {
        let mut state = HookLiveState::new(PathBuf::from("/tmp/moonbox-unused-spool"));
        state.apply_event(HookSpoolEvent {
            cli: CliTool::Claude,
            session_id: "claude-s1".into(),
            transcript_path: Some("/tmp/claude-s1.jsonl".into()),
            cwd: Some("/repo".into()),
            tmux: None,
            tmux_pane: Some("%1".into()),
            captured_at_ms: 1000,
            event_name: "PermissionRequest".into(),
            kind: HookEventKind::PermissionRequest,
            summary: "Approval: Edit".into(),
            wait_reason: Some("Approval: Edit".into()),
        });

        let waiting = state.waiting_sessions();
        assert_eq!(waiting.len(), 1);
        assert_eq!(waiting[0].status, HookSessionStatus::Waiting);
        assert_eq!(waiting[0].wait_reason.as_deref(), Some("Approval: Edit"));

        state.apply_event(HookSpoolEvent {
            cli: CliTool::Claude,
            session_id: "claude-s1".into(),
            transcript_path: Some("/tmp/claude-s1.jsonl".into()),
            cwd: Some("/repo".into()),
            tmux: None,
            tmux_pane: Some("%1".into()),
            captured_at_ms: 2000,
            event_name: "PreToolUse".into(),
            kind: HookEventKind::PreToolUse,
            summary: "Running Edit src/lib.rs".into(),
            wait_reason: None,
        });

        assert!(state.waiting_sessions().is_empty());
        let live = state
            .session_for(CliTool::Claude, "claude-s1", Some("/tmp/claude-s1.jsonl"))
            .expect("live session");
        assert_eq!(live.status, HookSessionStatus::Running);
        assert_eq!(live.status_since_ms, 2000);
    }

    #[test]
    fn read_spool_events_respects_offset_and_skips_bad_lines() {
        let root = env::temp_dir().join(format!("moonbox-hooks-read-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("spool root");
        let path = root.join("events.jsonl");
        let first = r#"{"version":1,"cli":"codex","captured_at_ms":1,"hook_event_name":"stop","session_id":"s1","event":{}}"#;
        fs::write(&path, format!("{first}\nnot-json\n")).expect("write one");
        let read = read_spool_events(&path, 0).expect("read one");
        assert_eq!(read.events.len(), 1);
        assert_eq!(read.skipped_lines, 1);

        let second = r#"{"version":1,"cli":"codex","captured_at_ms":2,"hook_event_name":"session_end","session_id":"s1","event":{}}"#;
        append_spool_line(&path, second, 4096, 2).expect("append second");
        let read = read_spool_events(&path, read.next_offset).expect("read tail");
        assert_eq!(read.events.len(), 1);
        assert_eq!(read.events[0].kind, HookEventKind::SessionEnd);
        let _ = fs::remove_dir_all(root);
    }
}
