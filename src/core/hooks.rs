use std::{
    env, fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use super::{
    config::{self, HooksConfig},
    error::CoreError,
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
    "SessionStart",
    "UserPromptSubmit",
    "PreToolUse",
    "PermissionRequest",
    "PostToolUse",
    "Stop",
];
const MOONBOX_HOOK_MARKER: &str = "hook-event --cli";

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
    pub moonbox_config_path: Option<String>,
    pub spool: HookSpoolReport,
    pub providers: Vec<HookProviderChange>,
    pub notes: Vec<String>,
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
    for event in provider.events() {
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
                .events()
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
        "M93 surfaces configuration and spool health only; live badges, waiting queue, and tmux jump are later milestones.".into(),
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
        value["hooks"]["Stop"]
            .as_array_mut()
            .expect("stop groups")
            .push(json!({"hooks": [{"type": "command", "command": "/bin/echo keep"}]}));

        assert!(uninstall_provider_entries(HookProvider::Codex, &mut value).expect("uninstall"));

        assert_eq!(count_moonbox_entries(HookProvider::Codex, &value), 0);
        assert!(
            value["hooks"]["Stop"]
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
}
