use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

use serde::{Deserialize, Serialize};

use super::error::CoreError;

const DEFAULT_DIFF_LINE_LIMIT: usize = 240;
const DEFAULT_COMMAND_OUTPUT_CHAR_LIMIT: usize = 12_000;
const KEY_FILE_CANDIDATES: &[&str] = &[
    "AGENTS.md",
    "CLAUDE.md",
    "README.md",
    "Cargo.toml",
    "package.json",
    "pnpm-lock.yaml",
    "package-lock.json",
    "yarn.lock",
    "pyproject.toml",
    "go.mod",
    ".gitignore",
];

#[derive(Debug, Clone)]
pub struct WorkspaceSnapshotOptions {
    pub path: PathBuf,
    pub diff_line_limit: usize,
    pub test_commands: Vec<String>,
}

impl Default for WorkspaceSnapshotOptions {
    fn default() -> Self {
        Self {
            path: PathBuf::from("."),
            diff_line_limit: DEFAULT_DIFF_LINE_LIMIT,
            test_commands: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceSnapshot {
    pub version: u16,
    pub cwd: String,
    pub repo_root: Option<String>,
    pub git: GitSnapshot,
    pub key_files: Vec<KeyFileSnapshot>,
    pub environment: EnvironmentSnapshot,
    pub test_commands: Vec<SnapshotCommandResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitSnapshot {
    pub available: bool,
    pub head: Option<String>,
    pub branch: Option<String>,
    pub upstream: Option<String>,
    pub dirty: bool,
    pub staged: Vec<String>,
    pub unstaged: Vec<String>,
    pub untracked: Vec<String>,
    pub staged_diff_stat: Option<String>,
    pub unstaged_diff_stat: Option<String>,
    pub staged_diff_preview: Option<DiffPreview>,
    pub unstaged_diff_preview: Option<DiffPreview>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffPreview {
    pub line_limit: usize,
    pub truncated: bool,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyFileSnapshot {
    pub path: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvironmentSnapshot {
    pub os: String,
    pub arch: String,
    pub shell: Option<String>,
    pub term: Option<String>,
    pub ci: bool,
    pub moonbox_session_mode: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotCommandResult {
    pub command: String,
    pub cwd: String,
    pub exit_code: Option<i32>,
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
}

pub fn capture_workspace_snapshot(
    options: &WorkspaceSnapshotOptions,
) -> Result<WorkspaceSnapshot, CoreError> {
    let cwd =
        canonical_or_original(&options.path).map_err(|error| CoreError::WorkspaceSnapshot {
            reason: format!("cannot read path {}: {error}", options.path.display()),
        })?;
    let repo_root = git_output(&cwd, &["rev-parse", "--show-toplevel"])
        .ok()
        .and_then(|output| non_empty(output.stdout));
    let git = repo_root
        .as_deref()
        .map(PathBuf::from)
        .map(|root| git_snapshot(&root, options.diff_line_limit))
        .unwrap_or_else(|| GitSnapshot {
            available: false,
            head: None,
            branch: None,
            upstream: None,
            dirty: false,
            staged: Vec::new(),
            unstaged: Vec::new(),
            untracked: Vec::new(),
            staged_diff_stat: None,
            unstaged_diff_stat: None,
            staged_diff_preview: None,
            unstaged_diff_preview: None,
            reason: Some("path is not inside a git worktree".into()),
        });
    let snapshot_root = repo_root
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or_else(|| cwd.clone());
    let test_commands = options
        .test_commands
        .iter()
        .map(|command| run_test_command(command, &snapshot_root))
        .collect::<Result<Vec<_>, _>>()?;

    Ok(WorkspaceSnapshot {
        version: 1,
        cwd: cwd.display().to_string(),
        repo_root,
        git,
        key_files: key_files(&snapshot_root),
        environment: environment_snapshot(),
        test_commands,
    })
}

fn git_snapshot(repo_root: &Path, diff_line_limit: usize) -> GitSnapshot {
    let head = git_output(repo_root, &["rev-parse", "HEAD"])
        .ok()
        .and_then(|output| non_empty(output.stdout));
    let branch = git_output(repo_root, &["branch", "--show-current"])
        .ok()
        .and_then(|output| non_empty(output.stdout));
    let upstream = git_output(
        repo_root,
        &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"],
    )
    .ok()
    .and_then(|output| non_empty(output.stdout));
    let porcelain = git_output(repo_root, &["status", "--porcelain=v1"])
        .map(|output| output.stdout)
        .unwrap_or_default();
    let staged = staged_paths(&porcelain);
    let unstaged = unstaged_paths(&porcelain);
    let untracked = untracked_paths(&porcelain);
    let staged_diff_stat = git_output(repo_root, &["diff", "--cached", "--stat"])
        .ok()
        .and_then(|output| non_empty(output.stdout));
    let unstaged_diff_stat = git_output(repo_root, &["diff", "--stat"])
        .ok()
        .and_then(|output| non_empty(output.stdout));
    let staged_diff_preview = git_output(repo_root, &["diff", "--cached", "--no-ext-diff"])
        .ok()
        .and_then(|output| diff_preview(output.stdout, diff_line_limit));
    let unstaged_diff_preview = git_output(repo_root, &["diff", "--no-ext-diff"])
        .ok()
        .and_then(|output| diff_preview(output.stdout, diff_line_limit));

    GitSnapshot {
        available: true,
        head,
        branch,
        upstream,
        dirty: !staged.is_empty() || !unstaged.is_empty() || !untracked.is_empty(),
        staged,
        unstaged,
        untracked,
        staged_diff_stat,
        unstaged_diff_stat,
        staged_diff_preview,
        unstaged_diff_preview,
        reason: None,
    }
}

fn staged_paths(porcelain: &str) -> Vec<String> {
    porcelain
        .lines()
        .filter(|line| {
            let status = line.as_bytes().first().copied().unwrap_or(b' ');
            status != b' ' && status != b'?'
        })
        .filter_map(status_path)
        .collect()
}

fn unstaged_paths(porcelain: &str) -> Vec<String> {
    porcelain
        .lines()
        .filter(|line| !line.starts_with("??"))
        .filter(|line| line.as_bytes().get(1).copied().unwrap_or(b' ') != b' ')
        .filter_map(status_path)
        .collect()
}

fn untracked_paths(porcelain: &str) -> Vec<String> {
    porcelain
        .lines()
        .filter(|line| line.starts_with("??"))
        .filter_map(status_path)
        .collect()
}

fn status_path(line: &str) -> Option<String> {
    line.get(3..)
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(str::to_owned)
}

fn diff_preview(text: String, line_limit: usize) -> Option<DiffPreview> {
    let text = text.trim_end();
    if text.is_empty() {
        return None;
    }
    if line_limit == 0 {
        return Some(DiffPreview {
            line_limit,
            truncated: false,
            text: text.into(),
        });
    }
    let mut lines = text.lines();
    let selected = lines.by_ref().take(line_limit).collect::<Vec<_>>();
    let truncated = lines.next().is_some();
    Some(DiffPreview {
        line_limit,
        truncated,
        text: selected.join("\n"),
    })
}

fn key_files(root: &Path) -> Vec<KeyFileSnapshot> {
    KEY_FILE_CANDIDATES
        .iter()
        .filter_map(|relative| {
            let path = root.join(relative);
            let metadata = fs::metadata(&path).ok()?;
            metadata.is_file().then(|| KeyFileSnapshot {
                path: (*relative).into(),
                bytes: metadata.len(),
            })
        })
        .collect()
}

fn environment_snapshot() -> EnvironmentSnapshot {
    EnvironmentSnapshot {
        os: env::consts::OS.into(),
        arch: env::consts::ARCH.into(),
        shell: env_value("SHELL"),
        term: env_value("TERM"),
        ci: env::var_os("CI").is_some(),
        moonbox_session_mode: env_value("MOONBOX_SESSION_MODE"),
    }
}

fn run_test_command(command: &str, cwd: &Path) -> Result<SnapshotCommandResult, CoreError> {
    let output = Command::new(default_shell())
        .arg("-lc")
        .arg(command)
        .current_dir(cwd)
        .output()
        .map_err(|error| CoreError::WorkspaceSnapshot {
            reason: format!("cannot run test command `{command}`: {error}"),
        })?;
    let (stdout, stdout_truncated) = truncate_chars(
        &String::from_utf8_lossy(&output.stdout),
        DEFAULT_COMMAND_OUTPUT_CHAR_LIMIT,
    );
    let (stderr, stderr_truncated) = truncate_chars(
        &String::from_utf8_lossy(&output.stderr),
        DEFAULT_COMMAND_OUTPUT_CHAR_LIMIT,
    );
    Ok(SnapshotCommandResult {
        command: command.into(),
        cwd: cwd.display().to_string(),
        exit_code: output.status.code(),
        success: output.status.success(),
        stdout,
        stderr,
        stdout_truncated,
        stderr_truncated,
    })
}

fn default_shell() -> String {
    env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into())
}

fn git_output(cwd: &Path, args: &[&str]) -> Result<ProcessOutput, CoreError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .output()
        .map_err(|error| CoreError::WorkspaceSnapshot {
            reason: format!("cannot run git {}: {error}", args.join(" ")),
        })?;
    if output.status.success() {
        Ok(ProcessOutput {
            stdout: String::from_utf8_lossy(&output.stdout).trim().to_owned(),
        })
    } else {
        Err(CoreError::WorkspaceSnapshot {
            reason: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        })
    }
}

struct ProcessOutput {
    stdout: String,
}

fn canonical_or_original(path: &Path) -> Result<PathBuf, std::io::Error> {
    fs::canonicalize(path).or_else(|_| {
        if path.exists() {
            Ok(path.to_path_buf())
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "path does not exist",
            ))
        }
    })
}

fn env_value(key: &str) -> Option<String> {
    env::var(key)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn non_empty(value: String) -> Option<String> {
    (!value.trim().is_empty()).then(|| value.trim().to_owned())
}

fn truncate_chars(value: &str, limit: usize) -> (String, bool) {
    if limit == 0 {
        return (value.into(), false);
    }
    let mut output = String::new();
    let mut truncated = false;
    for (index, character) in value.chars().enumerate() {
        if index >= limit {
            truncated = true;
            break;
        }
        output.push(character);
    }
    (output, truncated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn snapshot_captures_git_workspace_state() {
        let root = test_repo("workspace-state");
        write_file(&root, "README.md", "initial\n");
        git(&root, &["init"]);
        git(&root, &["add", "README.md"]);
        git(
            &root,
            &[
                "-c",
                "user.email=test@example.com",
                "-c",
                "user.name=Moonbox Test",
                "commit",
                "-m",
                "init",
            ],
        );
        write_file(&root, "README.md", "initial\nchanged\n");
        write_file(&root, "AGENTS.md", "agent instructions\n");
        git(&root, &["add", "AGENTS.md"]);
        write_file(&root, "scratch.txt", "untracked\n");

        let snapshot = capture_workspace_snapshot(&WorkspaceSnapshotOptions {
            path: root.clone(),
            diff_line_limit: 20,
            test_commands: Vec::new(),
        })
        .expect("snapshot");

        assert_eq!(snapshot.version, 1);
        assert!(snapshot.git.available);
        assert!(snapshot.git.dirty);
        assert!(snapshot.git.head.is_some());
        assert!(snapshot.git.staged.contains(&"AGENTS.md".into()));
        assert!(snapshot.git.unstaged.contains(&"README.md".into()));
        assert!(!snapshot.git.unstaged.contains(&"scratch.txt".into()));
        assert!(snapshot.git.untracked.contains(&"scratch.txt".into()));
        assert!(
            snapshot
                .key_files
                .iter()
                .any(|file| file.path == "README.md")
        );
        assert!(
            snapshot
                .key_files
                .iter()
                .any(|file| file.path == "AGENTS.md")
        );
    }

    #[test]
    fn snapshot_records_explicit_test_command_result() {
        let root = test_repo("test-command");
        git(&root, &["init"]);

        let snapshot = capture_workspace_snapshot(&WorkspaceSnapshotOptions {
            path: root.clone(),
            diff_line_limit: 20,
            test_commands: vec!["printf snapshot-ok".into()],
        })
        .expect("snapshot");

        assert_eq!(snapshot.test_commands.len(), 1);
        assert!(snapshot.test_commands[0].success);
        assert_eq!(snapshot.test_commands[0].stdout, "snapshot-ok");
    }

    fn test_repo(name: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let root = env::temp_dir().join(format!(
            "moonbox-snapshot-{name}-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("test repo");
        root
    }

    fn write_file(root: &Path, relative: &str, contents: &str) {
        fs::write(root.join(relative), contents).expect("write file");
    }

    fn git(root: &Path, args: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .status()
            .expect("git command");
        assert!(status.success(), "git command failed: {args:?}");
    }
}
