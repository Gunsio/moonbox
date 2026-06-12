use std::{
    env,
    process::{Command, Output},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TmuxPaneTarget {
    pub socket_path: String,
    pub pane_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TmuxJumpCommand {
    pub program: String,
    pub socket_path: String,
    pub pane_id: String,
    pub display: String,
}

impl TmuxPaneTarget {
    pub fn command(&self) -> TmuxJumpCommand {
        let program = tmux_program();
        TmuxJumpCommand {
            display: format!(
                "{} -S {} select-window -t {} && {} -S {} select-pane -t {}",
                program, self.socket_path, self.pane_id, program, self.socket_path, self.pane_id
            ),
            program,
            socket_path: self.socket_path.clone(),
            pane_id: self.pane_id.clone(),
        }
    }
}

pub fn target_from_hook(
    tmux: Option<&str>,
    pane_id: Option<&str>,
) -> Result<TmuxPaneTarget, String> {
    let pane_id = pane_id
        .map(str::trim)
        .filter(|pane| !pane.is_empty())
        .ok_or_else(|| "hook event did not include TMUX_PANE".to_string())?;
    let tmux = tmux
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "hook event did not include TMUX socket metadata".to_string())?;
    let socket_path = tmux
        .split(',')
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "TMUX metadata does not include a socket path".to_string())?;
    Ok(TmuxPaneTarget {
        socket_path: socket_path.into(),
        pane_id: pane_id.into(),
    })
}

pub fn tmux_program() -> String {
    env::var("MOONBOX_TMUX_BIN")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "tmux".into())
}

#[allow(dead_code)]
pub fn validate_pane(target: &TmuxPaneTarget) -> Result<(), String> {
    validate_pane_with_program(&tmux_program(), target)
}

pub fn execute_jump(command: &TmuxJumpCommand) -> Result<(), String> {
    let target = TmuxPaneTarget {
        socket_path: command.socket_path.clone(),
        pane_id: command.pane_id.clone(),
    };
    validate_pane_with_program(&command.program, &target)?;
    run_tmux_status(
        &command.program,
        &[
            "-S",
            &command.socket_path,
            "select-window",
            "-t",
            &command.pane_id,
        ],
    )?;
    run_tmux_status(
        &command.program,
        &[
            "-S",
            &command.socket_path,
            "select-pane",
            "-t",
            &command.pane_id,
        ],
    )
}

fn validate_pane_with_program(program: &str, target: &TmuxPaneTarget) -> Result<(), String> {
    validate_pane_with_runner(program, target, run_tmux_output)
}

fn validate_pane_with_runner(
    program: &str,
    target: &TmuxPaneTarget,
    mut runner: impl FnMut(&str, &[&str]) -> Result<Output, String>,
) -> Result<(), String> {
    let output = runner(
        program,
        &[
            "-S",
            &target.socket_path,
            "list-panes",
            "-a",
            "-F",
            "#{pane_id}",
        ],
    )?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "tmux list-panes failed: {}",
            compact_stderr(&stderr)
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.lines().any(|line| line.trim() == target.pane_id) {
        Ok(())
    } else {
        Err(format!("tmux pane {} is not live", target.pane_id))
    }
}

fn run_tmux_output(program: &str, args: &[&str]) -> Result<Output, String> {
    Command::new(program)
        .args(args)
        .output()
        .map_err(|error| format!("cannot run {program}: {error}"))
}

fn run_tmux_status(program: &str, args: &[&str]) -> Result<(), String> {
    let output = run_tmux_output(program, args)?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(format!(
        "{} {} failed: {}",
        program,
        args.join(" "),
        compact_stderr(&stderr)
    ))
}

fn compact_stderr(stderr: &str) -> String {
    let compact = stderr.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.is_empty() {
        "no stderr".into()
    } else if compact.chars().count() > 120 {
        format!("{}...", compact.chars().take(117).collect::<String>())
    } else {
        compact
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::PermissionsExt, path::Path};

    #[test]
    fn target_from_hook_requires_socket_and_pane() {
        let target =
            target_from_hook(Some("/tmp/tmux-501/default,1,0"), Some("%42")).expect("target");
        assert_eq!(target.socket_path, "/tmp/tmux-501/default");
        assert_eq!(target.pane_id, "%42");
        assert!(target_from_hook(None, Some("%42")).is_err());
        assert!(target_from_hook(Some("/tmp/tmux-501/default,1,0"), None).is_err());
    }

    #[test]
    fn validate_pane_accepts_listed_pane_and_rejects_missing_pane() {
        let target = TmuxPaneTarget {
            socket_path: "/tmp/tmux-fixture".into(),
            pane_id: "%42".into(),
        };
        let ok =
            validate_pane_with_runner("tmux", &target, |_, _| Ok(fake_output(0, "%1\n%42\n", "")));
        assert!(ok.is_ok());

        let missing =
            validate_pane_with_runner("tmux", &target, |_, _| Ok(fake_output(0, "%1\n%2\n", "")))
                .expect_err("missing");
        assert!(missing.contains("not live"));
    }

    #[test]
    fn execute_jump_runs_select_window_then_select_pane() {
        let root = env::temp_dir().join(format!("moonbox-tmux-fake-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("root");
        let log = root.join("tmux.log");
        let fake = root.join("tmux");
        write_fake_tmux(&fake, &log, true);
        let command = TmuxJumpCommand {
            program: fake.display().to_string(),
            socket_path: "/tmp/tmux-fixture".into(),
            pane_id: "%42".into(),
            display: "fake tmux".into(),
        };

        execute_jump(&command).expect("jump");
        let log = fs::read_to_string(log).expect("log");
        assert!(log.contains("list-panes"));
        assert!(log.contains("select-window -t %42"));
        assert!(log.contains("select-pane -t %42"));
        let _ = fs::remove_dir_all(root);
    }

    fn fake_output(code: i32, stdout: &str, stderr: &str) -> Output {
        use std::os::unix::process::ExitStatusExt;
        Output {
            status: std::process::ExitStatus::from_raw(code),
            stdout: stdout.as_bytes().to_vec(),
            stderr: stderr.as_bytes().to_vec(),
        }
    }

    fn write_fake_tmux(path: &Path, log: &Path, include_pane: bool) {
        let panes = if include_pane { "%1\n%42\n" } else { "%1\n" };
        fs::write(
            path,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\ncase \"$*\" in\n  *list-panes*) cat <<'PANES'\n{}PANES\nexit 0 ;;\n  *select-window*) exit 0 ;;\n  *select-pane*) exit 0 ;;\nesac\nexit 1\n",
                log.display(),
                panes
            ),
        )
        .expect("fake tmux");
        let mut perms = fs::metadata(path).expect("meta").permissions();
        perms.set_mode(0o755);
        fs::set_permissions(path, perms).expect("chmod");
    }
}
