use std::{env, fs, path::PathBuf};

use serde::{Deserialize, Serialize};

use super::model::CliTool;

#[derive(Debug, Default, Serialize, Deserialize)]
struct UserConfig {
    last_target: Option<CliTool>,
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
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let config = UserConfig {
        last_target: Some(target),
    };
    fs::write(path, serde_json::to_string_pretty(&config)?)?;
    Ok(())
}

fn config_path() -> Option<PathBuf> {
    if let Ok(path) = env::var("MOONBOX_CONFIG") {
        return Some(PathBuf::from(path));
    }
    env::var_os("HOME").map(|home| {
        PathBuf::from(home)
            .join(".config")
            .join("moonbox")
            .join("config.json")
    })
}
