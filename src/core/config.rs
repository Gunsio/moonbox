use std::{
    env, fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use super::model::CliTool;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompilerPresetConfig {
    pub id: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    pub timeout_ms: Option<u64>,
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,
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

#[derive(Debug, Default, Serialize, Deserialize)]
struct UserConfig {
    last_target: Option<CliTool>,
    default_compiler: Option<String>,
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

pub fn load_starred_sessions() -> Vec<String> {
    load_user_config()
        .map(|config| config.starred_sessions)
        .unwrap_or_default()
        .into_iter()
        .filter(|id| !id.trim().is_empty())
        .collect()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_compiler_presets_with_enabled_default() {
        let config = serde_json::from_str::<UserConfig>(
            r#"{
  "default_compiler": "handoff",
  "compiler_presets": [
    {"id": "handoff", "command": "/bin/moonbox-handoff", "args": ["--mode", "handoff"], "timeout_ms": 12000}
  ],
  "ssh_hosts": [
    {"name": "dev", "hostname": "dev.example.com", "user": "moon", "port": 2222, "identity_file": "~/.ssh/dev", "tags": ["dev"]}
  ],
  "starred_sessions": ["codex:session-1"]
}"#,
        )
        .expect("config");

        assert_eq!(config.default_compiler.as_deref(), Some("handoff"));
        assert_eq!(config.compiler_presets[0].id, "handoff");
        assert!(config.compiler_presets[0].enabled);
        assert_eq!(config.compiler_presets[0].args, ["--mode", "handoff"]);
        assert_eq!(config.ssh_hosts[0].name, "dev");
        assert_eq!(config.ssh_hosts[0].host, "dev.example.com");
        assert_eq!(config.starred_sessions, ["codex:session-1"]);
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
        assert_eq!(saved.compiler_presets.len(), 1);
        assert_eq!(saved.ssh_hosts.len(), 1);
        assert_eq!(saved.starred_sessions, ["codex:abc", "claude:def"]);
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
