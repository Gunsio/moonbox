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

#[derive(Debug, Default, Serialize, Deserialize)]
struct UserConfig {
    last_target: Option<CliTool>,
    default_compiler: Option<String>,
    #[serde(default)]
    compiler_presets: Vec<CompilerPresetConfig>,
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
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut config = load_user_config_from_path(path).unwrap_or_default();
    config.last_target = Some(target);
    fs::write(path, serde_json::to_string_pretty(&config)?)?;
    Ok(())
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

fn config_path() -> Option<PathBuf> {
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
  ]
}"#,
        )
        .expect("config");

        assert_eq!(config.default_compiler.as_deref(), Some("handoff"));
        assert_eq!(config.compiler_presets[0].id, "handoff");
        assert!(config.compiler_presets[0].enabled);
        assert_eq!(config.compiler_presets[0].args, ["--mode", "handoff"]);
    }

    #[test]
    fn save_last_target_preserves_compiler_presets() {
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
  ]
}"#,
        )
        .expect("write config");

        save_last_target_to_path(&path, CliTool::Claude).expect("save");
        let saved = load_user_config_from_path(&path).expect("saved config");

        assert_eq!(saved.last_target, Some(CliTool::Claude));
        assert_eq!(saved.default_compiler.as_deref(), Some("handoff"));
        assert_eq!(saved.compiler_presets.len(), 1);
        assert!(!saved.compiler_presets[0].enabled);
    }
}
