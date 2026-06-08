use std::path::{Path, PathBuf};

use super::model::{
    CliTool, ContinuationLevel, ContinuationOptions, ContinuationProtocol, PackageImportPlan,
    SessionSummary, WorkCapsule, WorkspaceRestoreMode, WorkspaceRestorePlan,
};

pub fn build_continuation_protocol(
    session: &SessionSummary,
    target: CliTool,
    capsule: &WorkCapsule,
    capsule_path: Option<&str>,
    options: ContinuationOptions,
) -> ContinuationProtocol {
    let package_import = package_import_plan(target, capsule_path, options.requested_level);
    let workspace_restore = workspace_restore_plan(session, target, capsule, options);
    let notes = protocol_notes(&package_import, &workspace_restore);

    ContinuationProtocol {
        version: 1,
        requested_level: options.requested_level,
        target_input_level: ContinuationLevel::PromptOnly,
        package_import,
        workspace_restore,
        notes,
    }
}

fn package_import_plan(
    target: CliTool,
    capsule_path: Option<&str>,
    requested_level: ContinuationLevel,
) -> PackageImportPlan {
    let requested = requested_level == ContinuationLevel::PackageImport;
    PackageImportPlan {
        requested,
        supported: false,
        target_native_import: false,
        capsule_path: capsule_path.map(str::to_owned),
        command: None,
        reason: if requested {
            format!(
                "{target} has no verified native Capsule import contract yet; Moonbox will not claim package import support"
            )
        } else if capsule_path.is_some() {
            "Capsule file is read and verified by Moonbox, then summarized into a prompt-only target handoff".into()
        } else {
            "native continuation package import not requested".into()
        },
        warnings: if requested {
            vec![
                "Requested package import would currently degrade to prompt-only target input."
                    .into(),
            ]
        } else {
            Vec::new()
        },
    }
}

fn workspace_restore_plan(
    session: &SessionSummary,
    target: CliTool,
    capsule: &WorkCapsule,
    options: ContinuationOptions,
) -> WorkspaceRestorePlan {
    let requested = options.requested_level == ContinuationLevel::WorkspaceRestore;
    let local_cwd = local_workspace_dir(&session.cwd);
    if !requested {
        return WorkspaceRestorePlan {
            requested: false,
            mode: WorkspaceRestoreMode::None,
            supported: true,
            reversible: true,
            preview_only: false,
            source_cwd: local_cwd.as_ref().map(|path| path.display().to_string()),
            target_cwd: local_cwd.as_ref().map(|path| path.display().to_string()),
            branch: None,
            worktree_path: None,
            commands: Vec::new(),
            cleanup_commands: Vec::new(),
            reason: if local_cwd.is_some() {
                "workspace restore not requested; target command will start in the source cwd when the target supports cwd arguments".into()
            } else {
                "workspace restore not requested; source cwd is not a local directory, so target uses the terminal default cwd".into()
            },
            warnings: Vec::new(),
        };
    }

    let Some(cwd) = local_cwd else {
        return WorkspaceRestorePlan {
            requested: true,
            mode: options.workspace_restore,
            supported: false,
            reversible: false,
            preview_only: true,
            source_cwd: None,
            target_cwd: None,
            branch: None,
            worktree_path: None,
            commands: Vec::new(),
            cleanup_commands: Vec::new(),
            reason: "cannot build workspace restore preview because the source cwd is not a local directory".into(),
            warnings: vec!["Select prompt-only handoff or capture a workspace snapshot from a local checkout first.".into()],
        };
    };

    let branch = restore_branch_name(target, capsule);
    match options.workspace_restore {
        WorkspaceRestoreMode::Branch => branch_restore_plan(&cwd, branch),
        WorkspaceRestoreMode::Worktree => worktree_restore_plan(&cwd, branch, target, capsule),
        WorkspaceRestoreMode::None => WorkspaceRestorePlan {
            requested: true,
            mode: WorkspaceRestoreMode::None,
            supported: false,
            reversible: false,
            preview_only: true,
            source_cwd: Some(cwd.display().to_string()),
            target_cwd: Some(cwd.display().to_string()),
            branch: None,
            worktree_path: None,
            commands: Vec::new(),
            cleanup_commands: Vec::new(),
            reason: "workspace restore was requested without a branch or worktree mode".into(),
            warnings: vec![
                "Use --workspace-restore branch or --workspace-restore worktree.".into(),
            ],
        },
    }
}

fn branch_restore_plan(cwd: &Path, branch: String) -> WorkspaceRestorePlan {
    WorkspaceRestorePlan {
        requested: true,
        mode: WorkspaceRestoreMode::Branch,
        supported: false,
        reversible: true,
        preview_only: true,
        source_cwd: Some(cwd.display().to_string()),
        target_cwd: Some(cwd.display().to_string()),
        branch: Some(branch.clone()),
        worktree_path: None,
        commands: vec![
            format!("git -C {} status --short", shell_quote_path(cwd)),
            format!("git -C {} switch -c {}", shell_quote_path(cwd), shell_quote(&branch)),
        ],
        cleanup_commands: vec![
            format!("git -C {} switch -", shell_quote_path(cwd)),
            format!("git -C {} branch -D {}", shell_quote_path(cwd), shell_quote(&branch)),
        ],
        reason: "branch restore is preview-only in M60; Moonbox will not mutate the current worktree before target launch".into(),
        warnings: vec![
            "Branch mode changes the current worktree branch if the preview commands are run manually.".into(),
            "Review dirty files and upstream state before applying the preview.".into(),
        ],
    }
}

fn worktree_restore_plan(
    cwd: &Path,
    branch: String,
    target: CliTool,
    capsule: &WorkCapsule,
) -> WorkspaceRestorePlan {
    let worktree_path = worktree_path(cwd, target, capsule);
    WorkspaceRestorePlan {
        requested: true,
        mode: WorkspaceRestoreMode::Worktree,
        supported: false,
        reversible: true,
        preview_only: true,
        source_cwd: Some(cwd.display().to_string()),
        target_cwd: Some(worktree_path.display().to_string()),
        branch: Some(branch.clone()),
        worktree_path: Some(worktree_path.display().to_string()),
        commands: vec![
            format!("git -C {} status --short", shell_quote_path(cwd)),
            format!(
                "git -C {} worktree add {} -b {} HEAD",
                shell_quote_path(cwd),
                shell_quote_path(&worktree_path),
                shell_quote(&branch)
            ),
        ],
        cleanup_commands: vec![
            format!(
                "git -C {} worktree remove {}",
                shell_quote_path(cwd),
                shell_quote_path(&worktree_path)
            ),
            format!("git -C {} branch -D {}", shell_quote_path(cwd), shell_quote(&branch)),
        ],
        reason: "worktree restore is preview-only in M60; Moonbox will not create a worktree before target launch".into(),
        warnings: vec![
            "Worktree mode is the recommended reversible restore path once execution support exists.".into(),
            "Preview commands are local evidence, not target CLI input.".into(),
        ],
    }
}

fn protocol_notes(
    package_import: &PackageImportPlan,
    workspace_restore: &WorkspaceRestorePlan,
) -> Vec<String> {
    let mut notes = Vec::new();
    notes.push(
        "Target input remains prompt-only unless a requested capability is explicitly supported."
            .into(),
    );
    if package_import.requested && !package_import.supported {
        notes.push("Package import is blocked because no target-native Capsule import adapter is verified.".into());
    }
    if workspace_restore.requested {
        notes.push(
            "Workspace restore actions are preview-only in M60 and are never run implicitly."
                .into(),
        );
    } else {
        notes.push("Workspace restore is not claimed for this launch plan.".into());
    }
    notes
}

fn restore_branch_name(target: CliTool, capsule: &WorkCapsule) -> String {
    let rewind = capsule
        .rewind_point
        .split_whitespace()
        .next()
        .unwrap_or("rewind");
    format!("moonbox/{}-restore-{}", target.id(), safe_fragment(rewind))
}

fn worktree_path(cwd: &Path, target: CliTool, capsule: &WorkCapsule) -> PathBuf {
    let rewind = capsule
        .rewind_point
        .split_whitespace()
        .next()
        .unwrap_or("rewind");
    let parent = cwd.parent().unwrap_or(cwd);
    parent.join("moonbox-worktrees").join(format!(
        "{}-{}-{}",
        safe_fragment(&capsule.source_session),
        target.id(),
        safe_fragment(rewind)
    ))
}

fn local_workspace_dir(cwd: &str) -> Option<PathBuf> {
    let cwd = cwd.trim();
    if cwd.is_empty() || cwd == "~" || cwd.contains("<path:redacted>") {
        return None;
    }
    let expanded = expand_home(cwd);
    let path = PathBuf::from(expanded);
    (path.is_absolute() && path.is_dir()).then_some(path)
}

fn expand_home(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("~/")
        && let Some(home) = std::env::var_os("HOME")
    {
        return PathBuf::from(home)
            .join(rest)
            .to_string_lossy()
            .into_owned();
    }
    path.into()
}

fn safe_fragment(value: &str) -> String {
    let fragment = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    if fragment.is_empty() {
        "unknown".into()
    } else {
        fragment
    }
}

fn shell_quote_path(path: &Path) -> String {
    shell_quote(&path.display().to_string())
}

fn shell_quote(value: &str) -> String {
    if value.is_empty() {
        return "''".into();
    }
    if value.bytes().all(|byte| {
        byte.is_ascii_alphanumeric()
            || matches!(byte, b'_' | b'-' | b'.' | b'/' | b':' | b'=' | b',')
    }) {
        return value.into();
    }
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use crate::core::{
        data,
        model::{CliTool, ContinuationLevel, ContinuationOptions, WorkspaceRestoreMode},
    };

    use super::*;

    #[test]
    fn default_protocol_is_prompt_only_and_honest_about_workspace() {
        let data = data::workbench_data(CliTool::Codex, CliTool::Hermes).expect("data");
        let session = data
            .sessions
            .iter()
            .find(|session| session.id == data.capsule.source_session)
            .expect("session");

        let protocol = build_continuation_protocol(
            session,
            CliTool::Hermes,
            &data.capsule,
            None,
            ContinuationOptions::default(),
        );

        assert_eq!(protocol.requested_level, ContinuationLevel::PromptOnly);
        assert_eq!(protocol.target_input_level, ContinuationLevel::PromptOnly);
        assert!(!protocol.package_import.requested);
        assert!(!protocol.workspace_restore.requested);
        assert!(protocol.workspace_restore.supported);
    }

    #[test]
    fn package_import_request_is_not_silently_downgraded() {
        let data = data::workbench_data(CliTool::Codex, CliTool::Hermes).expect("data");
        let session = data
            .sessions
            .iter()
            .find(|session| session.id == data.capsule.source_session)
            .expect("session");

        let protocol = build_continuation_protocol(
            session,
            CliTool::Hermes,
            &data.capsule,
            Some("./capsule.json"),
            ContinuationOptions::new(Some(ContinuationLevel::PackageImport), None),
        );

        assert!(protocol.package_import.requested);
        assert!(!protocol.package_import.supported);
        assert_eq!(protocol.target_input_level, ContinuationLevel::PromptOnly);
        assert_eq!(
            protocol.package_import.capsule_path.as_deref(),
            Some("./capsule.json")
        );
    }

    #[test]
    fn workspace_restore_request_builds_reversible_worktree_preview_only_plan() {
        let root =
            std::env::temp_dir().join(format!("moonbox-continuation-{}", std::process::id()));
        fs::create_dir_all(&root).expect("workspace root");
        let mut data = data::workbench_data(CliTool::Codex, CliTool::Hermes).expect("data");
        let mut session = data
            .sessions
            .iter()
            .find(|session| session.id == data.capsule.source_session)
            .expect("session")
            .clone();
        session.cwd = root.display().to_string();
        data.capsule.rewind_point = "evt-091 / user request".into();

        let protocol = build_continuation_protocol(
            &session,
            CliTool::Hermes,
            &data.capsule,
            None,
            ContinuationOptions::new(
                Some(ContinuationLevel::WorkspaceRestore),
                Some(WorkspaceRestoreMode::Worktree),
            ),
        );

        assert!(protocol.workspace_restore.requested);
        assert_eq!(
            protocol.workspace_restore.mode,
            WorkspaceRestoreMode::Worktree
        );
        assert!(!protocol.workspace_restore.supported);
        assert!(protocol.workspace_restore.reversible);
        assert!(protocol.workspace_restore.preview_only);
        assert!(
            protocol
                .workspace_restore
                .commands
                .iter()
                .any(|command| command.contains("worktree add"))
        );
        assert!(
            protocol
                .workspace_restore
                .cleanup_commands
                .iter()
                .any(|command| command.contains("worktree remove"))
        );
    }
}
