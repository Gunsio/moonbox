use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, ExitStatus},
};

use super::{
    error::CoreError,
    handoff::{self, AgentRunner},
};

const MATT_HANDOFF_SKILL_SOURCE: &str =
    "https://github.com/mattpocock/skills/tree/main/skills/productivity/handoff";
const MATT_HANDOFF_MARKER_FILE: &str = ".moonbox-source";
const AGENTBUDDY_NPM_REGISTRY: &str = "https://bnpm.byted.org";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupInstallTarget {
    CodexSdk,
    ClaudeSdk,
    MattHandoff,
}

impl SetupInstallTarget {
    pub fn label(self) -> &'static str {
        match self {
            Self::CodexSdk => "Codex SDK runner",
            Self::ClaudeSdk => "Claude SDK runner",
            Self::MattHandoff => "matt-handoff skill",
        }
    }

    pub fn cli_arg(self) -> &'static str {
        match self {
            Self::CodexSdk => "codex-sdk",
            Self::ClaudeSdk => "claude-sdk",
            Self::MattHandoff => "matt-handoff",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetupInstallReport {
    pub target: SetupInstallTarget,
    pub destination: Option<PathBuf>,
    pub message: String,
}

pub fn install(target: SetupInstallTarget) -> Result<SetupInstallReport, CoreError> {
    match target {
        SetupInstallTarget::CodexSdk => install_runner_sdk(AgentRunner::Codex),
        SetupInstallTarget::ClaudeSdk => install_runner_sdk(AgentRunner::Claude),
        SetupInstallTarget::MattHandoff => install_matt_handoff(),
    }
}

pub fn setup_command_display_for_current_exe(target: SetupInstallTarget) -> String {
    match env::current_exe() {
        Ok(path) => format!("{} setup install {}", path.display(), target.cli_arg()),
        Err(_) => format!("moonbox setup install {}", target.cli_arg()),
    }
}

fn install_runner_sdk(runner: AgentRunner) -> Result<SetupInstallReport, CoreError> {
    let plan = handoff::runner_sdk_install_plan(runner).ok_or_else(|| CoreError::Setup {
        reason: format!("no usable python3 found for {} SDK setup", runner.label()),
    })?;
    println!(
        "Installing {} SDK into {}",
        runner.label(),
        plan.venv_root.display()
    );
    run_status(
        "create Python venv",
        Command::new(&plan.python)
            .arg("-m")
            .arg("venv")
            .arg(&plan.venv_root)
            .status(),
    )?;
    run_status(
        "install Python SDK package",
        Command::new(&plan.managed_python)
            .arg("-m")
            .arg("pip")
            .arg("install")
            .arg(&plan.package)
            .status(),
    )?;
    Ok(SetupInstallReport {
        target: match runner {
            AgentRunner::Codex => SetupInstallTarget::CodexSdk,
            AgentRunner::Claude => SetupInstallTarget::ClaudeSdk,
        },
        destination: Some(plan.managed_python),
        message: format!("installed {} SDK package {}", runner.label(), plan.package),
    })
}

fn install_matt_handoff() -> Result<SetupInstallReport, CoreError> {
    if let Some(existing) = installed_handoff_skill_dir() {
        let is_moonbox_matt_install = is_matt_handoff_marker(&existing);
        if !is_moonbox_matt_install {
            return Err(CoreError::Setup {
                reason: format!(
                    "handoff skill already exists at {}; not overwriting it as matt-handoff",
                    existing.display()
                ),
            });
        }
        return Ok(SetupInstallReport {
            target: SetupInstallTarget::MattHandoff,
            destination: Some(existing.clone()),
            message: format!("matt-handoff already installed at {}", existing.display()),
        });
    }

    println!("Moonbox setup: matt-handoff skill");
    println!("Source: {MATT_HANDOFF_SKILL_SOURCE}");
    println!("Installer: agentbuddy via npx");
    println!("Scope: global agent skill");
    println!(
        "Moonbox will not write the skill contents itself; the installer owns download, confirmation, and placement."
    );
    println!();
    run_status(
        "run agentbuddy skill installer",
        Command::new("npx")
            .env("npm_config_registry", AGENTBUDDY_NPM_REGISTRY)
            .arg("--yes")
            .arg("agentbuddy@latest")
            .arg("skill")
            .arg("add")
            .arg(MATT_HANDOFF_SKILL_SOURCE)
            .arg("-g")
            .status(),
    )?;
    let destination = installed_handoff_skill_dir().ok_or_else(|| CoreError::Setup {
        reason: "agentbuddy completed but no installed handoff skill was found".into(),
    })?;
    write_matt_handoff_marker(&destination)?;
    Ok(SetupInstallReport {
        target: SetupInstallTarget::MattHandoff,
        destination: Some(destination.clone()),
        message: format!("installed matt-handoff at {}", destination.display()),
    })
}

fn installed_handoff_skill_dir() -> Option<PathBuf> {
    handoff_skill_roots()
        .into_iter()
        .map(|root| root.join("handoff"))
        .find(|path| path.join("SKILL.md").is_file())
}

fn handoff_skill_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(paths) = env::var_os("MOONBOX_SKILLS_DIRS") {
        roots.extend(env::split_paths(&paths));
    }
    if let Some(path) = env::var_os("CODEX_HOME").or_else(|| env::var_os("MOONBOX_CODEX_HOME")) {
        roots.push(PathBuf::from(path).join("skills"));
    }
    if let Some(home) = env::var_os("HOME") {
        roots.push(PathBuf::from(&home).join(".codex").join("skills"));
        roots.push(PathBuf::from(home).join(".agents").join("skills"));
    }
    roots
}

fn is_matt_handoff_marker(skill_dir: &Path) -> bool {
    fs::read_to_string(skill_dir.join(MATT_HANDOFF_MARKER_FILE))
        .ok()
        .is_some_and(|contents| contents.contains("mattpocock/skills"))
}

fn write_matt_handoff_marker(skill_dir: &Path) -> Result<(), CoreError> {
    fs::write(
        skill_dir.join(MATT_HANDOFF_MARKER_FILE),
        format!("source={MATT_HANDOFF_SKILL_SOURCE}\ninstaller=agentbuddy\n"),
    )
    .map_err(|error| CoreError::Setup {
        reason: format!(
            "cannot write matt-handoff marker in {}: {error}",
            skill_dir.display()
        ),
    })
}

fn run_status(stage: &'static str, result: std::io::Result<ExitStatus>) -> Result<(), CoreError> {
    let status = result.map_err(|error| CoreError::Setup {
        reason: format!("{stage} failed to start: {error}"),
    })?;
    if status.success() {
        Ok(())
    } else {
        Err(CoreError::Setup {
            reason: match status.code() {
                Some(code) => format!("{stage} exited with code {code}"),
                None => format!("{stage} exited without a status code"),
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setup_install_target_cli_args_are_stable() {
        assert_eq!(SetupInstallTarget::CodexSdk.cli_arg(), "codex-sdk");
        assert_eq!(SetupInstallTarget::ClaudeSdk.cli_arg(), "claude-sdk");
        assert_eq!(SetupInstallTarget::MattHandoff.cli_arg(), "matt-handoff");
    }
}
