use std::{
    env,
    process::{Command, Stdio},
};

use serde::{Deserialize, Serialize};

use super::{
    adapter::report_from_sessions,
    data,
    error::CoreError,
    model::{CliTool, SessionSummary, SourceProvenance, WorkbenchData},
    ssh::{self, SshHostEntry},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataSpaceKind {
    Local,
    Ssh,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataSpaceEntry {
    pub id: String,
    pub label: String,
    pub kind: DataSpaceKind,
    pub detail: String,
}

impl DataSpaceEntry {
    pub fn local() -> Self {
        Self {
            id: "local".into(),
            label: "Local".into(),
            kind: DataSpaceKind::Local,
            detail: "this machine".into(),
        }
    }

    pub fn is_local(&self) -> bool {
        self.kind == DataSpaceKind::Local
    }
}

pub fn list_data_spaces() -> Vec<DataSpaceEntry> {
    let mut spaces = vec![DataSpaceEntry::local()];
    if let Ok(hosts) = ssh::list_ssh_hosts() {
        spaces.extend(hosts.into_iter().map(space_from_ssh_host));
    }
    spaces
}

pub fn load_workbench_for_space(
    space: &DataSpaceEntry,
    source: CliTool,
    target: CliTool,
) -> Result<WorkbenchData, CoreError> {
    if space.is_local() {
        return data::workbench_data(source, target);
    }
    let sessions = load_remote_sessions(space)?;
    let source_session = sessions
        .iter()
        .find(|session| session.cli == source)
        .cloned()
        .or_else(|| sessions.first().cloned())
        .ok_or_else(|| CoreError::DataSpaceLoad {
            space: space.label.clone(),
            reason: "remote inventory returned no sessions".into(),
        })?;
    Ok(workbench_from_remote_sessions(
        space,
        source_session,
        sessions,
        target,
    ))
}

pub fn workbench_from_remote_sessions(
    space: &DataSpaceEntry,
    source_session: SessionSummary,
    sessions: Vec<SessionSummary>,
    target: CliTool,
) -> WorkbenchData {
    data::workbench_data_from_readonly_inventory(
        source_session,
        sessions.clone(),
        remote_adapter_reports(space, &sessions),
        target,
    )
}

fn space_from_ssh_host(host: SshHostEntry) -> DataSpaceEntry {
    let detail = host
        .user
        .as_ref()
        .map(|user| format!("{user}@{}", host.host))
        .unwrap_or(host.host);
    DataSpaceEntry {
        id: format!("ssh:{}", host.name),
        label: host.name,
        kind: DataSpaceKind::Ssh,
        detail,
    }
}

fn load_remote_sessions(space: &DataSpaceEntry) -> Result<Vec<SessionSummary>, CoreError> {
    let ssh_bin = env::var("MOONBOX_SSH_BIN").unwrap_or_else(|_| "ssh".into());
    load_remote_sessions_with_bin(space, &ssh_bin)
}

fn load_remote_sessions_with_bin(
    space: &DataSpaceEntry,
    ssh_bin: &str,
) -> Result<Vec<SessionSummary>, CoreError> {
    let host = space
        .id
        .strip_prefix("ssh:")
        .ok_or_else(|| CoreError::DataSpaceLoad {
            space: space.label.clone(),
            reason: "data space is not an SSH host".into(),
        })?;
    let output = Command::new(ssh_bin)
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=6",
            host,
            remote_moonbox_binary().as_str(),
            "sessions",
            "--json",
        ])
        .stdin(Stdio::null())
        .output()
        .map_err(|error| CoreError::DataSpaceLoad {
            space: space.label.clone(),
            reason: format!("cannot start ssh inventory command: {error}"),
        })?;
    if !output.status.success() {
        return Err(CoreError::DataSpaceLoad {
            space: space.label.clone(),
            reason: format!(
                "ssh inventory exited with {}; stderr: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        });
    }
    serde_json::from_slice::<Vec<SessionSummary>>(&output.stdout).map_err(|error| {
        CoreError::DataSpaceLoad {
            space: space.label.clone(),
            reason: format!("remote sessions JSON is invalid: {error}"),
        }
    })
}

fn remote_moonbox_binary() -> String {
    env::var("MOONBOX_REMOTE_BIN")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "moonbox".into())
}

fn remote_adapter_reports(
    space: &DataSpaceEntry,
    sessions: &[SessionSummary],
) -> Vec<super::model::SourceAdapterReport> {
    CliTool::ALL
        .into_iter()
        .map(|tool| {
            let tool_sessions = sessions
                .iter()
                .filter(|session| session.cli == tool)
                .cloned()
                .collect::<Vec<_>>();
            report_from_sessions(
                tool,
                SourceProvenance::Real,
                true,
                Some(format!("ssh://{}", space.label)),
                "remote_space_inventory",
                format!("read-only SSH inventory from {}", space.label),
                &tool_sessions,
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    use super::*;
    use crate::core::model::{SessionStatus, SourceProvenance};

    #[test]
    fn data_spaces_include_local_first() {
        let spaces = list_data_spaces();

        assert_eq!(spaces[0].id, "local");
        assert!(spaces[0].is_local());
    }

    #[cfg(unix)]
    #[test]
    fn remote_inventory_uses_ssh_without_opening_sessions() {
        let script = env::temp_dir().join(format!(
            "moonbox-fake-ssh-{}-{}",
            std::process::id(),
            "sessions"
        ));
        fs::write(
            &script,
            r#"#!/bin/sh
case "$*" in
  *"moonbox sessions --json"*) ;;
  *) echo "unexpected: $*" >&2; exit 2 ;;
esac
cat <<'JSON'
[{"id":"remote-codex-1","cli":"codex","title":"Remote task","cwd":"/srv/app","updated_at":"2026-06-07T10:00:00+08:00","updated":"2026-06-07 10:00","status":"healthy","branch":"main","token_count":null,"health_reason":null,"event_count":9,"resume_command":"codex resume remote-codex-1","source_provenance":"real","source_path":"/remote/session.jsonl","parse_skip_count":0}]
JSON
"#,
        )
        .expect("write fake ssh");
        let mut perms = fs::metadata(&script).expect("metadata").permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&script, perms).expect("chmod");
        let space = DataSpaceEntry {
            id: "ssh:devbox".into(),
            label: "devbox".into(),
            kind: DataSpaceKind::Ssh,
            detail: "devbox.internal".into(),
        };

        let sessions =
            load_remote_sessions_with_bin(&space, script.to_str().expect("utf-8 script"))
                .expect("remote sessions");

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, "remote-codex-1");
        assert_eq!(sessions[0].status, SessionStatus::Healthy);
        assert_eq!(sessions[0].source_provenance, SourceProvenance::Real);
    }
}
