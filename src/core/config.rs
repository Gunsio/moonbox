use std::{
    env, fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::moonbox_theme;

use super::{
    handoff::AgentRunner,
    model::{CliTool, TimelineKind},
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompilerPresetConfig {
    pub id: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    pub timeout_ms: Option<u64>,
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,
    pub description: Option<String>,
    pub homepage: Option<String>,
    pub github_stars: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SshHostConfig {
    pub name: String,
    #[serde(alias = "hostname")]
    pub host: String,
    pub user: Option<String>,
    pub port: Option<u16>,
    pub identity_file: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedactionPolicyConfig {
    pub enabled: Option<bool>,
    pub secret_scan: Option<bool>,
    pub path_redaction: Option<bool>,
    pub prompt_injection_warnings: Option<bool>,
    #[serde(default)]
    pub event_allowlist: Vec<TimelineKind>,
    #[serde(default)]
    pub file_allowlist: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HooksConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub smart_enter_tmux: bool,
    pub spool_path: Option<String>,
    #[serde(default = "default_hook_spool_max_bytes")]
    pub spool_max_bytes: u64,
    #[serde(default = "default_hook_spool_max_files")]
    pub spool_max_files: usize,
}

impl Default for HooksConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            smart_enter_tmux: false,
            spool_path: None,
            spool_max_bytes: default_hook_spool_max_bytes(),
            spool_max_files: default_hook_spool_max_files(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiLanguage {
    #[default]
    English,
    ZhHans,
}

impl UiLanguage {
    pub fn label(self) -> &'static str {
        match self {
            Self::English => "English",
            Self::ZhHans => "简体中文",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::English => Self::ZhHans,
            Self::ZhHans => Self::English,
        }
    }

    pub fn previous(self) -> Self {
        self.next()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UiThemeName {
    #[default]
    Moonbox,
    TokyoNight,
    Gruvbox,
    LuoshenSwan,
    LuoshenDragon,
    LuoshenChrysanthemum,
    LuoshenPine,
}

impl UiThemeName {
    const SELECTABLE: [Self; 5] = [
        Self::Moonbox,
        Self::LuoshenSwan,
        Self::LuoshenDragon,
        Self::LuoshenChrysanthemum,
        Self::LuoshenPine,
    ];

    pub fn label(self) -> &'static str {
        moonbox_theme::ThemeId::from(self).label()
    }

    pub fn ascii_icon(self) -> &'static str {
        moonbox_theme::ThemeId::from(self).ascii_icon()
    }

    pub fn normalized_for_ui(self) -> Self {
        match self {
            Self::TokyoNight | Self::Gruvbox => Self::Moonbox,
            theme => theme,
        }
    }

    pub fn next(self) -> Self {
        let current = self.normalized_for_ui();
        let index = Self::SELECTABLE
            .iter()
            .position(|theme| *theme == current)
            .unwrap_or_default();
        Self::SELECTABLE[(index + 1) % Self::SELECTABLE.len()]
    }

    pub fn previous(self) -> Self {
        let current = self.normalized_for_ui();
        let index = Self::SELECTABLE
            .iter()
            .position(|theme| *theme == current)
            .unwrap_or_default();
        Self::SELECTABLE[(index + Self::SELECTABLE.len() - 1) % Self::SELECTABLE.len()]
    }
}

impl From<UiThemeName> for moonbox_theme::ThemeId {
    fn from(theme: UiThemeName) -> Self {
        match theme {
            UiThemeName::Moonbox => Self::Moonbox,
            UiThemeName::TokyoNight => Self::TokyoNight,
            UiThemeName::Gruvbox => Self::Gruvbox,
            UiThemeName::LuoshenSwan => Self::LuoshenSwan,
            UiThemeName::LuoshenDragon => Self::LuoshenDragon,
            UiThemeName::LuoshenChrysanthemum => Self::LuoshenChrysanthemum,
            UiThemeName::LuoshenPine => Self::LuoshenPine,
        }
    }
}

impl From<moonbox_theme::ThemeId> for UiThemeName {
    fn from(theme: moonbox_theme::ThemeId) -> Self {
        match theme {
            moonbox_theme::ThemeId::Moonbox => Self::Moonbox,
            moonbox_theme::ThemeId::TokyoNight => Self::TokyoNight,
            moonbox_theme::ThemeId::Gruvbox => Self::Gruvbox,
            moonbox_theme::ThemeId::LuoshenSwan => Self::LuoshenSwan,
            moonbox_theme::ThemeId::LuoshenDragon => Self::LuoshenDragon,
            moonbox_theme::ThemeId::LuoshenChrysanthemum => Self::LuoshenChrysanthemum,
            moonbox_theme::ThemeId::LuoshenPine => Self::LuoshenPine,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandoffRunnerPreference {
    #[default]
    Codex,
    Claude,
}

impl HandoffRunnerPreference {
    pub fn label(self) -> &'static str {
        match self {
            Self::Codex => "Codex",
            Self::Claude => "Claude",
        }
    }

    pub fn runner(self) -> AgentRunner {
        match self {
            Self::Codex => AgentRunner::Codex,
            Self::Claude => AgentRunner::Claude,
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Codex => Self::Claude,
            Self::Claude => Self::Codex,
        }
    }

    pub fn previous(self) -> Self {
        self.next()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct UiPreferencesConfig {
    #[serde(default)]
    pub language: UiLanguage,
    #[serde(default)]
    pub theme: UiThemeName,
}

impl UiPreferencesConfig {
    fn normalized_for_ui(self) -> Self {
        Self {
            language: self.language,
            theme: self.theme.normalized_for_ui(),
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct UserConfig {
    last_target: Option<CliTool>,
    default_compiler: Option<String>,
    redaction_policy: Option<RedactionPolicyConfig>,
    #[serde(default)]
    hooks: HooksConfig,
    #[serde(default)]
    ui: UiPreferencesConfig,
    #[serde(default)]
    handoff_runner: HandoffRunnerPreference,
    #[serde(default)]
    compiler_presets: Vec<CompilerPresetConfig>,
    #[serde(default)]
    ssh_hosts: Vec<SshHostConfig>,
    #[serde(default)]
    starred_sessions: Vec<String>,
}

pub fn load_last_target() -> Option<CliTool> {
    let path = config_path()?;
    let contents = fs::read_to_string(path).ok()?;
    serde_json::from_str::<UserConfig>(&contents)
        .ok()
        .and_then(|config| config.last_target)
}

pub fn save_last_target(target: CliTool) -> Result<(), Box<dyn std::error::Error>> {
    let path = config_path().ok_or("missing home directory")?;
    save_last_target_to_path(&path, target)
}

fn save_last_target_to_path(
    path: &Path,
    target: CliTool,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut config = load_user_config_from_path(path).unwrap_or_default();
    config.last_target = Some(target);
    save_user_config_to_path(path, &config)
}

pub fn load_default_compiler() -> Option<String> {
    load_user_config()
        .ok()
        .and_then(|config| config.default_compiler)
        .filter(|id| !id.trim().is_empty())
}

pub fn load_compiler_presets() -> Vec<CompilerPresetConfig> {
    load_user_config()
        .map(|config| config.compiler_presets)
        .unwrap_or_default()
        .into_iter()
        .filter(|preset| !preset.id.trim().is_empty() && !preset.command.trim().is_empty())
        .collect()
}

pub fn load_ssh_host_configs() -> Vec<SshHostConfig> {
    load_user_config()
        .map(|config| config.ssh_hosts)
        .unwrap_or_default()
        .into_iter()
        .filter(|host| !host.name.trim().is_empty() && !host.host.trim().is_empty())
        .collect()
}

pub fn add_ssh_host_config(host: SshHostConfig) -> Result<(), Box<dyn std::error::Error>> {
    let path = config_path().ok_or("missing home directory")?;
    add_ssh_host_config_to_path(&path, host)
}

fn add_ssh_host_config_to_path(
    path: &Path,
    host: SshHostConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut config = load_user_config_from_path(path).unwrap_or_default();
    if config.ssh_hosts.iter().any(|entry| entry.name == host.name) {
        return Err(format!("ssh host {} already exists", host.name).into());
    }
    config.ssh_hosts.push(host);
    save_user_config_to_path(path, &config)
}

pub fn remove_ssh_host_config(name: &str) -> Result<bool, Box<dyn std::error::Error>> {
    let path = config_path().ok_or("missing home directory")?;
    remove_ssh_host_config_from_path(&path, name)
}

fn remove_ssh_host_config_from_path(
    path: &Path,
    name: &str,
) -> Result<bool, Box<dyn std::error::Error>> {
    let mut config = load_user_config_from_path(path).unwrap_or_default();
    let before = config.ssh_hosts.len();
    config.ssh_hosts.retain(|entry| entry.name != name);
    if config.ssh_hosts.len() == before {
        return Ok(false);
    }
    save_user_config_to_path(path, &config)?;
    Ok(true)
}

pub fn load_starred_sessions() -> Vec<String> {
    load_user_config()
        .map(|config| config.starred_sessions)
        .unwrap_or_default()
        .into_iter()
        .filter(|id| !id.trim().is_empty())
        .collect()
}

pub fn load_redaction_policy_config() -> Option<RedactionPolicyConfig> {
    load_user_config()
        .ok()
        .and_then(|config| config.redaction_policy)
}

pub fn load_hooks_config() -> HooksConfig {
    load_user_config()
        .map(|config| config.hooks)
        .unwrap_or_default()
}

#[cfg(not(test))]
pub fn load_ui_preferences_config() -> UiPreferencesConfig {
    load_user_config()
        .map(|config| config.ui.normalized_for_ui())
        .unwrap_or_default()
}

#[cfg_attr(test, allow(dead_code))]
pub fn load_handoff_runner_preference() -> HandoffRunnerPreference {
    load_user_config()
        .map(|config| config.handoff_runner)
        .unwrap_or_default()
}

pub fn save_hooks_config(config: HooksConfig) -> Result<(), Box<dyn std::error::Error>> {
    let path = config_path().ok_or("missing home directory")?;
    let mut user_config = load_user_config_from_path(&path).unwrap_or_default();
    user_config.hooks = config;
    save_user_config_to_path(&path, &user_config)
}

#[cfg(test)]
fn save_ui_preferences_config_to_path(
    path: &Path,
    ui: UiPreferencesConfig,
) -> Result<UiPreferencesConfig, Box<dyn std::error::Error>> {
    let mut user_config = load_user_config_from_path(path).unwrap_or_default();
    user_config.ui = ui.normalized_for_ui();
    save_user_config_to_path(path, &user_config)?;
    Ok(user_config.ui)
}

pub fn save_ui_preferences_smart_enter_and_handoff_runner(
    ui: UiPreferencesConfig,
    smart_enter_tmux: bool,
    handoff_runner: HandoffRunnerPreference,
) -> Result<(UiPreferencesConfig, HooksConfig, HandoffRunnerPreference), Box<dyn std::error::Error>>
{
    let path = config_path().ok_or("missing home directory")?;
    save_ui_preferences_smart_enter_and_handoff_runner_to_path(
        &path,
        ui,
        smart_enter_tmux,
        handoff_runner,
    )
}

fn save_ui_preferences_smart_enter_and_handoff_runner_to_path(
    path: &Path,
    ui: UiPreferencesConfig,
    smart_enter_tmux: bool,
    handoff_runner: HandoffRunnerPreference,
) -> Result<(UiPreferencesConfig, HooksConfig, HandoffRunnerPreference), Box<dyn std::error::Error>>
{
    let mut user_config = load_user_config_from_path(path).unwrap_or_default();
    user_config.ui = ui.normalized_for_ui();
    user_config.hooks.smart_enter_tmux = smart_enter_tmux;
    user_config.handoff_runner = handoff_runner;
    save_user_config_to_path(path, &user_config)?;
    Ok((
        user_config.ui,
        user_config.hooks,
        user_config.handoff_runner,
    ))
}

#[cfg(test)]
fn save_ui_preferences_and_smart_enter_to_path(
    path: &Path,
    ui: UiPreferencesConfig,
    smart_enter_tmux: bool,
) -> Result<(UiPreferencesConfig, HooksConfig), Box<dyn std::error::Error>> {
    let mut user_config = load_user_config_from_path(path).unwrap_or_default();
    user_config.ui = ui.normalized_for_ui();
    user_config.hooks.smart_enter_tmux = smart_enter_tmux;
    save_user_config_to_path(path, &user_config)?;
    Ok((user_config.ui, user_config.hooks))
}

pub fn save_starred_sessions(sessions: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let path = config_path().ok_or("missing home directory")?;
    let mut config = load_user_config_from_path(&path).unwrap_or_default();
    config.starred_sessions = sessions.to_vec();
    save_user_config_to_path(&path, &config)
}

pub(crate) fn validate_config_file_at(path: &Path) -> Result<(), String> {
    load_user_config_from_path(path)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

pub(crate) fn config_path() -> Option<PathBuf> {
    if let Ok(path) = env::var("MOONBOX_CONFIG") {
        return Some(PathBuf::from(path));
    }
    #[cfg(test)]
    {
        Some(env::temp_dir().join(format!("moonbox-test-config-{}.json", std::process::id())))
    }
    #[cfg(not(test))]
    {
        env::var_os("HOME").map(|home| {
            PathBuf::from(home)
                .join(".config")
                .join("moonbox")
                .join("config.json")
        })
    }
}

fn load_user_config() -> Result<UserConfig, Box<dyn std::error::Error>> {
    let path = config_path().ok_or("missing home directory")?;
    load_user_config_from_path(&path)
}

fn load_user_config_from_path(path: &Path) -> Result<UserConfig, Box<dyn std::error::Error>> {
    let contents = fs::read_to_string(path)?;
    Ok(serde_json::from_str::<UserConfig>(&contents)?)
}

fn save_user_config_to_path(
    path: &Path,
    config: &UserConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_string_pretty(config)?)?;
    Ok(())
}

fn enabled_by_default() -> bool {
    true
}

fn default_hook_spool_max_bytes() -> u64 {
    10 * 1024 * 1024
}

fn default_hook_spool_max_files() -> usize {
    5
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_compiler_presets_with_enabled_default() {
        let config = serde_json::from_str::<UserConfig>(
            r#"{
	  "default_compiler": "handoff",
	  "redaction_policy": {
	    "enabled": true,
	    "secret_scan": true,
	    "path_redaction": true,
	    "prompt_injection_warnings": true,
	    "event_allowlist": ["user", "assistant", "tool", "rewind_point"],
	    "file_allowlist": ["README.md", "src/"]
	  },
	  "ui": {"language": "zh_hans", "theme": "tokyo-night"},
	  "compiler_presets": [
    {"id": "handoff", "command": "/bin/moonbox-handoff", "args": ["--mode", "handoff"], "timeout_ms": 12000, "description": "Compresses source timelines for target CLIs.", "homepage": "https://github.com/example/handoff", "github_stars": 42}
  ],
  "ssh_hosts": [
    {"name": "dev", "hostname": "dev.example.com", "user": "moon", "port": 2222, "identity_file": "~/.ssh/dev", "tags": ["dev"]}
  ],
  "starred_sessions": ["codex:session-1"]
}"#,
        )
        .expect("config");

        assert_eq!(config.default_compiler.as_deref(), Some("handoff"));
        let redaction = config.redaction_policy.expect("redaction policy");
        assert_eq!(
            redaction.event_allowlist,
            [
                TimelineKind::User,
                TimelineKind::Assistant,
                TimelineKind::Tool,
                TimelineKind::RewindPoint
            ]
        );
        assert_eq!(redaction.file_allowlist, ["README.md", "src/"]);
        assert_eq!(config.compiler_presets[0].id, "handoff");
        assert!(config.compiler_presets[0].enabled);
        assert!(!config.hooks.enabled);
        assert!(!config.hooks.smart_enter_tmux);
        assert_eq!(config.hooks.spool_max_bytes, default_hook_spool_max_bytes());
        assert_eq!(config.ui.language, UiLanguage::ZhHans);
        assert_eq!(config.ui.theme, UiThemeName::TokyoNight);
        assert_eq!(config.handoff_runner, HandoffRunnerPreference::Codex);
        assert_eq!(config.ui.theme.normalized_for_ui(), UiThemeName::Moonbox);
        assert_eq!(config.compiler_presets[0].args, ["--mode", "handoff"]);
        assert_eq!(
            config.compiler_presets[0].description.as_deref(),
            Some("Compresses source timelines for target CLIs.")
        );
        assert_eq!(
            config.compiler_presets[0].homepage.as_deref(),
            Some("https://github.com/example/handoff")
        );
        assert_eq!(config.compiler_presets[0].github_stars, Some(42));
        assert_eq!(config.ssh_hosts[0].name, "dev");
        assert_eq!(config.ssh_hosts[0].host, "dev.example.com");
        assert_eq!(config.starred_sessions, ["codex:session-1"]);
    }

    #[test]
    fn parses_luoshen_theme_ids() {
        let config = serde_json::from_str::<UserConfig>(
            r#"{
  "ui": {"language": "english", "theme": "luoshen-chrysanthemum"}
}"#,
        )
        .expect("config");

        assert_eq!(config.ui.language, UiLanguage::English);
        assert_eq!(config.ui.theme, UiThemeName::LuoshenChrysanthemum);
        assert_eq!(config.ui.theme.label(), "荣曜秋菊 / Radiant Chrysanthemum");
        assert_eq!(config.ui.theme.next(), UiThemeName::LuoshenPine);
        assert_eq!(config.ui.theme.previous(), UiThemeName::LuoshenDragon);
    }

    #[test]
    fn save_last_target_preserves_compiler_presets_and_ssh_hosts() {
        let path = env::temp_dir().join(format!(
            "moonbox-config-preserve-{}.json",
            std::process::id()
        ));
        let _ = fs::remove_file(&path);
        fs::write(
            &path,
            r#"{
  "default_compiler": "handoff",
  "hooks": {"enabled": true, "smart_enter_tmux": true, "spool_path": "/tmp/moonbox/events.jsonl", "spool_max_bytes": 4096, "spool_max_files": 2},
  "ui": {"language": "zh_hans", "theme": "gruvbox"},
  "compiler_presets": [
    {"id": "handoff", "command": "/bin/moonbox-handoff", "enabled": false}
  ],
  "ssh_hosts": [
    {"name": "prod", "host": "prod.internal", "user": "deploy"}
  ],
  "starred_sessions": ["hermes:prod"]
}"#,
        )
        .expect("write config");

        save_last_target_to_path(&path, CliTool::Claude).expect("save");
        let saved = load_user_config_from_path(&path).expect("saved config");

        assert_eq!(saved.last_target, Some(CliTool::Claude));
        assert_eq!(saved.default_compiler.as_deref(), Some("handoff"));
        assert!(saved.hooks.enabled);
        assert!(saved.hooks.smart_enter_tmux);
        assert_eq!(
            saved.hooks.spool_path.as_deref(),
            Some("/tmp/moonbox/events.jsonl")
        );
        assert_eq!(saved.hooks.spool_max_bytes, 4096);
        assert_eq!(saved.hooks.spool_max_files, 2);
        assert_eq!(saved.ui.language, UiLanguage::ZhHans);
        assert_eq!(saved.ui.theme, UiThemeName::Gruvbox);
        assert_eq!(saved.ui.theme.normalized_for_ui(), UiThemeName::Moonbox);
        assert_eq!(saved.compiler_presets.len(), 1);
        assert!(!saved.compiler_presets[0].enabled);
        assert_eq!(saved.ssh_hosts.len(), 1);
        assert_eq!(saved.ssh_hosts[0].name, "prod");
        assert_eq!(saved.starred_sessions, ["hermes:prod"]);
    }

    #[test]
    fn save_starred_sessions_preserves_other_config() {
        let path = env::temp_dir().join(format!(
            "moonbox-config-starred-{}.json",
            std::process::id()
        ));
        let _ = fs::remove_file(&path);
        fs::write(
            &path,
            r#"{
  "last_target": "hermes",
  "default_compiler": "handoff",
  "compiler_presets": [
    {"id": "handoff", "command": "/bin/moonbox-handoff"}
  ],
  "ssh_hosts": [
    {"name": "prod", "host": "prod.internal"}
  ]
}"#,
        )
        .expect("write config");

        let mut config = load_user_config_from_path(&path).expect("config");
        config.starred_sessions = vec!["codex:abc".into(), "claude:def".into()];
        save_user_config_to_path(&path, &config).expect("save");
        let saved = load_user_config_from_path(&path).expect("saved config");

        assert_eq!(saved.last_target, Some(CliTool::Hermes));
        assert_eq!(saved.default_compiler.as_deref(), Some("handoff"));
        assert!(!saved.hooks.enabled);
        assert_eq!(saved.ui, UiPreferencesConfig::default());
        assert_eq!(saved.compiler_presets.len(), 1);
        assert_eq!(saved.ssh_hosts.len(), 1);
        assert_eq!(saved.starred_sessions, ["codex:abc", "claude:def"]);
    }

    #[test]
    fn set_smart_enter_tmux_preserves_other_config() {
        let path = env::temp_dir().join(format!(
            "moonbox-config-smart-enter-{}.json",
            std::process::id()
        ));
        let _ = fs::remove_file(&path);
        fs::write(
            &path,
            r#"{
  "hooks": {"enabled": true, "spool_path": "/tmp/moonbox/events.jsonl"},
  "ui": {"language": "zh_hans", "theme": "tokyo-night"},
  "ssh_hosts": [
    {"name": "prod", "host": "prod.internal"}
  ],
  "starred_sessions": ["codex:abc"]
}"#,
        )
        .expect("write config");

        let (_, hooks) = save_ui_preferences_and_smart_enter_to_path(
            &path,
            UiPreferencesConfig {
                language: UiLanguage::ZhHans,
                theme: UiThemeName::LuoshenSwan,
            },
            true,
        )
        .expect("save smart enter");
        let saved = load_user_config_from_path(&path).expect("saved");

        assert!(hooks.enabled);
        assert!(hooks.smart_enter_tmux);
        assert!(saved.hooks.enabled);
        assert!(saved.hooks.smart_enter_tmux);
        assert_eq!(
            saved.hooks.spool_path.as_deref(),
            Some("/tmp/moonbox/events.jsonl")
        );
        assert_eq!(saved.ui.language, UiLanguage::ZhHans);
        assert_eq!(saved.ui.theme, UiThemeName::LuoshenSwan);
        assert_eq!(saved.ssh_hosts[0].name, "prod");
        assert_eq!(saved.starred_sessions, ["codex:abc"]);
    }

    #[test]
    fn save_ui_preferences_preserves_other_config() {
        let path = env::temp_dir().join(format!(
            "moonbox-config-ui-prefs-{}.json",
            std::process::id()
        ));
        let _ = fs::remove_file(&path);
        fs::write(
            &path,
            r#"{
  "hooks": {"enabled": true, "smart_enter_tmux": true, "spool_path": "/tmp/moonbox/events.jsonl"},
  "compiler_presets": [
    {"id": "handoff", "command": "/bin/moonbox-handoff"}
  ],
  "ssh_hosts": [
    {"name": "prod", "host": "prod.internal"}
  ],
  "starred_sessions": ["codex:abc"]
}"#,
        )
        .expect("write config");

        let ui = save_ui_preferences_config_to_path(
            &path,
            UiPreferencesConfig {
                language: UiLanguage::ZhHans,
                theme: UiThemeName::LuoshenDragon,
            },
        )
        .expect("save ui preferences");
        let saved = load_user_config_from_path(&path).expect("saved");

        assert_eq!(ui.language, UiLanguage::ZhHans);
        assert_eq!(ui.theme, UiThemeName::LuoshenDragon);
        assert!(saved.hooks.enabled);
        assert!(saved.hooks.smart_enter_tmux);
        assert_eq!(
            saved.hooks.spool_path.as_deref(),
            Some("/tmp/moonbox/events.jsonl")
        );
        assert_eq!(saved.compiler_presets[0].id, "handoff");
        assert_eq!(saved.ssh_hosts[0].name, "prod");
        assert_eq!(saved.starred_sessions, ["codex:abc"]);
    }

    #[test]
    fn save_ui_preferences_and_smart_enter_preserves_other_config() {
        let path = env::temp_dir().join(format!(
            "moonbox-config-ui-smart-{}.json",
            std::process::id()
        ));
        let _ = fs::remove_file(&path);
        fs::write(
            &path,
            r#"{
  "hooks": {"enabled": true, "smart_enter_tmux": false, "spool_path": "/tmp/moonbox/events.jsonl"},
  "compiler_presets": [
    {"id": "handoff", "command": "/bin/moonbox-handoff"}
  ],
  "ssh_hosts": [
    {"name": "prod", "host": "prod.internal"}
  ],
  "starred_sessions": ["codex:abc"]
}"#,
        )
        .expect("write config");

        let (ui, hooks) = save_ui_preferences_and_smart_enter_to_path(
            &path,
            UiPreferencesConfig {
                language: UiLanguage::ZhHans,
                theme: UiThemeName::LuoshenPine,
            },
            true,
        )
        .expect("save ui preferences and smart enter");
        let saved = load_user_config_from_path(&path).expect("saved");

        assert_eq!(ui.language, UiLanguage::ZhHans);
        assert_eq!(ui.theme, UiThemeName::LuoshenPine);
        assert!(hooks.enabled);
        assert!(hooks.smart_enter_tmux);
        assert_eq!(saved.ui.language, UiLanguage::ZhHans);
        assert_eq!(saved.ui.theme, UiThemeName::LuoshenPine);
        assert_eq!(saved.compiler_presets[0].id, "handoff");
        assert_eq!(saved.ssh_hosts[0].name, "prod");
        assert_eq!(saved.starred_sessions, ["codex:abc"]);
    }

    #[test]
    fn save_ui_preferences_smart_enter_and_handoff_runner_preserves_other_config() {
        let path = env::temp_dir().join(format!(
            "moonbox-config-ui-runner-{}.json",
            std::process::id()
        ));
        let _ = fs::remove_file(&path);
        fs::write(
            &path,
            r#"{
  "hooks": {"enabled": true, "smart_enter_tmux": false, "spool_path": "/tmp/moonbox/events.jsonl"},
  "compiler_presets": [
    {"id": "handoff", "command": "/bin/moonbox-handoff"}
  ],
  "ssh_hosts": [
    {"name": "prod", "host": "prod.internal"}
  ],
  "starred_sessions": ["codex:abc"]
}"#,
        )
        .expect("write config");

        let (ui, hooks, runner) = save_ui_preferences_smart_enter_and_handoff_runner_to_path(
            &path,
            UiPreferencesConfig {
                language: UiLanguage::ZhHans,
                theme: UiThemeName::LuoshenPine,
            },
            true,
            HandoffRunnerPreference::Claude,
        )
        .expect("save ui preferences, smart enter, and runner");
        let saved = load_user_config_from_path(&path).expect("saved");

        assert_eq!(ui.language, UiLanguage::ZhHans);
        assert_eq!(ui.theme, UiThemeName::LuoshenPine);
        assert!(hooks.enabled);
        assert!(hooks.smart_enter_tmux);
        assert_eq!(runner, HandoffRunnerPreference::Claude);
        assert_eq!(saved.handoff_runner, HandoffRunnerPreference::Claude);
        assert_eq!(saved.compiler_presets[0].id, "handoff");
        assert_eq!(saved.ssh_hosts[0].name, "prod");
        assert_eq!(saved.starred_sessions, ["codex:abc"]);
    }

    #[test]
    fn add_ssh_host_config_preserves_other_config_and_rejects_duplicates() {
        let path = env::temp_dir().join(format!(
            "moonbox-config-add-ssh-{}.json",
            std::process::id()
        ));
        let _ = fs::remove_file(&path);
        fs::write(
            &path,
            r#"{
  "last_target": "claude",
  "compiler_presets": [
    {"id": "handoff", "command": "/bin/moonbox-handoff"}
  ],
  "ssh_hosts": []
}"#,
        )
        .expect("write config");

        add_ssh_host_config_to_path(
            &path,
            SshHostConfig {
                name: "devbox".into(),
                host: "10.37.218.31".into(),
                user: Some("yangyang.1205".into()),
                port: Some(22),
                identity_file: Some("~/.ssh/id_ed25519".into()),
                tags: vec!["dev".into()],
            },
        )
        .expect("add host");
        let duplicate = add_ssh_host_config_to_path(
            &path,
            SshHostConfig {
                name: "devbox".into(),
                host: "10.37.218.31".into(),
                user: None,
                port: None,
                identity_file: None,
                tags: Vec::new(),
            },
        )
        .expect_err("duplicate rejected");

        let saved = load_user_config_from_path(&path).expect("saved config");
        assert_eq!(saved.last_target, Some(CliTool::Claude));
        assert_eq!(saved.compiler_presets[0].id, "handoff");
        assert_eq!(saved.ssh_hosts[0].name, "devbox");
        assert!(duplicate.to_string().contains("already exists"));
    }

    #[test]
    fn remove_ssh_host_config_preserves_other_config() {
        let path = env::temp_dir().join(format!(
            "moonbox-config-remove-ssh-{}.json",
            std::process::id()
        ));
        let _ = fs::remove_file(&path);
        fs::write(
            &path,
            r#"{
  "last_target": "hermes",
  "compiler_presets": [
    {"id": "handoff", "command": "/bin/moonbox-handoff"}
  ],
  "ssh_hosts": [
    {"name": "keep", "host": "keep.example.com"},
    {"name": "delete-me", "host": "10.37.218.31"}
  ],
  "starred_sessions": ["codex:abc"]
}"#,
        )
        .expect("write config");

        assert!(remove_ssh_host_config_from_path(&path, "delete-me").expect("remove host"));
        assert!(!remove_ssh_host_config_from_path(&path, "missing").expect("missing host"));

        let saved = load_user_config_from_path(&path).expect("saved config");
        assert_eq!(saved.last_target, Some(CliTool::Hermes));
        assert_eq!(saved.compiler_presets[0].id, "handoff");
        assert_eq!(saved.starred_sessions, vec!["codex:abc"]);
        assert_eq!(saved.ssh_hosts.len(), 1);
        assert_eq!(saved.ssh_hosts[0].name, "keep");
    }

    #[test]
    fn validate_config_file_rejects_bad_schema() {
        let path = env::temp_dir().join(format!(
            "moonbox-config-invalid-{}.json",
            std::process::id()
        ));
        let _ = fs::remove_file(&path);
        fs::write(&path, r#"{"last_target":"not-a-tool"}"#).expect("write config");

        let error = validate_config_file_at(&path).expect_err("invalid config");

        assert!(error.contains("unknown variant"));
    }
}
