use std::{
    env,
    process::{Command, Output, Stdio},
};

use serde::{Deserialize, Serialize};

use super::{
    adapter::report_from_sessions,
    data,
    error::CoreError,
    model::{
        CapsuleCompileRequest, CliTool, SessionAnatomy, SessionAnatomyStatus, SessionSummary,
        SourceProvenance, WorkbenchData,
    },
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
    #[serde(default)]
    pub ssh_host: Option<String>,
    #[serde(default)]
    pub ssh_user: Option<String>,
    #[serde(default)]
    pub ssh_port: Option<u16>,
    #[serde(default)]
    pub ssh_identity_file: Option<String>,
    #[serde(default)]
    pub config_source: Option<String>,
    #[serde(default)]
    pub config_path: Option<String>,
}

impl DataSpaceEntry {
    pub fn local() -> Self {
        Self {
            id: "local".into(),
            label: "Local".into(),
            kind: DataSpaceKind::Local,
            detail: "this machine".into(),
            ssh_host: None,
            ssh_user: None,
            ssh_port: None,
            ssh_identity_file: None,
            config_source: Some("runtime".into()),
            config_path: None,
        }
    }

    pub fn is_local(&self) -> bool {
        self.kind == DataSpaceKind::Local
    }
}

pub fn list_data_spaces() -> Vec<DataSpaceEntry> {
    let mut spaces = vec![DataSpaceEntry::local()];
    spaces.extend(
        ssh::list_managed_ssh_hosts()
            .into_iter()
            .map(space_from_ssh_host),
    );
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

pub fn load_remote_workbench_for_session(
    space: &DataSpaceEntry,
    source_session: SessionSummary,
    sessions: Vec<SessionSummary>,
    target: CliTool,
) -> Result<WorkbenchData, CoreError> {
    let request = load_remote_compile_request(space, &source_session, target)?;
    let source_session =
        remote_source_session_with_anatomy(space, source_session, request.source_session);
    data::workbench_data_from_timeline_snapshot(
        source_session,
        sessions.clone(),
        remote_adapter_reports(space, &sessions),
        target,
        request.timeline,
    )
}

fn space_from_ssh_host(host: SshHostEntry) -> DataSpaceEntry {
    let ssh_host = host.host.clone();
    let detail = host
        .user
        .as_ref()
        .map(|user| format!("{user}@{}", host.host))
        .unwrap_or(host.host);
    let config_source = match host.source {
        ssh::SshHostSource::MoonboxConfig => "Moonbox config",
        ssh::SshHostSource::OpensshConfig => "OpenSSH config",
    };
    DataSpaceEntry {
        id: format!("ssh:{}", host.name),
        label: host.name,
        kind: DataSpaceKind::Ssh,
        detail,
        ssh_host: Some(ssh_host),
        ssh_user: host.user,
        ssh_port: host.port,
        ssh_identity_file: host.identity_file,
        config_source: Some(config_source.into()),
        config_path: host.source_path,
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
    let output =
        run_remote_command_with_bin(space, ssh_bin, &remote_inventory_command(), "inventory")?;
    serde_json::from_slice::<Vec<SessionSummary>>(&output.stdout).map_err(|error| {
        CoreError::DataSpaceLoad {
            space: space.label.clone(),
            reason: format!("remote sessions JSON is invalid: {error}"),
        }
    })
}

fn load_remote_compile_request(
    space: &DataSpaceEntry,
    session: &SessionSummary,
    target: CliTool,
) -> Result<CapsuleCompileRequest, CoreError> {
    let ssh_bin = env::var("MOONBOX_SSH_BIN").unwrap_or_else(|_| "ssh".into());
    load_remote_compile_request_with_bin(space, session, target, &ssh_bin)
}

fn load_remote_compile_request_with_bin(
    space: &DataSpaceEntry,
    session: &SessionSummary,
    target: CliTool,
    ssh_bin: &str,
) -> Result<CapsuleCompileRequest, CoreError> {
    let command = remote_compile_request_command(&session.id, target);
    let output = run_remote_command_with_bin(space, ssh_bin, &command, "timeline")?;
    serde_json::from_slice::<CapsuleCompileRequest>(&output.stdout).map_err(|error| {
        CoreError::DataSpaceLoad {
            space: space.label.clone(),
            reason: format!("remote timeline JSON is invalid: {error}"),
        }
    })
}

fn remote_source_session_with_anatomy(
    space: &DataSpaceEntry,
    mut fallback: SessionSummary,
    mut remote: SessionSummary,
) -> SessionSummary {
    if remote.id != fallback.id || remote.cli != fallback.cli {
        fallback.anatomy = Some(remote_anatomy_fallback(format!(
            "Remote details returned {} {}, expected {} {}; keeping inventory metadata with degraded anatomy.",
            remote.cli, remote.id, fallback.cli, fallback.id
        )));
        return fallback;
    }
    if remote.anatomy.is_none() {
        remote.anatomy = Some(remote_anatomy_fallback(format!(
            "Remote moonbox on {} did not return session anatomy; upgrade the remote moonbox binary to M92 or newer.",
            space.label
        )));
    }
    remote
}

fn remote_anatomy_fallback(reason: impl Into<String>) -> SessionAnatomy {
    SessionAnatomy {
        status: SessionAnatomyStatus::Missing,
        scan_scope: "remote-unavailable".into(),
        notes: vec![reason.into()],
        ..SessionAnatomy::default()
    }
}

fn run_remote_command_with_bin(
    space: &DataSpaceEntry,
    ssh_bin: &str,
    remote_command: &str,
    action: &str,
) -> Result<Output, CoreError> {
    let host = space
        .id
        .strip_prefix("ssh:")
        .ok_or_else(|| CoreError::DataSpaceLoad {
            space: space.label.clone(),
            reason: "data space is not an SSH host".into(),
        })?;
    let target = ssh_target(space, host);
    let mut command = Command::new(ssh_bin);
    command
        .args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=6"])
        .stdin(Stdio::null());
    if let Some(port) = space.ssh_port {
        command.arg("-p").arg(port.to_string());
    }
    if let Some(identity_file) = space.ssh_identity_file.as_deref() {
        command.arg("-i").arg(identity_file);
    }
    let output = command
        .arg(target)
        .arg(remote_command)
        .output()
        .map_err(|error| CoreError::DataSpaceLoad {
            space: space.label.clone(),
            reason: format!("cannot start ssh {action} command: {error}"),
        })?;
    if !output.status.success() {
        return Err(CoreError::DataSpaceLoad {
            space: space.label.clone(),
            reason: format!(
                "ssh {action} exited with {}; stderr: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        });
    }
    Ok(output)
}

fn ssh_target(space: &DataSpaceEntry, fallback: &str) -> String {
    let host = space.ssh_host.as_deref().unwrap_or(fallback);
    space
        .ssh_user
        .as_ref()
        .filter(|user| !user.trim().is_empty())
        .map(|user| format!("{user}@{host}"))
        .unwrap_or_else(|| host.into())
}

fn remote_inventory_command() -> String {
    remote_moonbox_command(remote_moonbox_binary().as_deref(), &["sessions", "--json"])
}

#[cfg(test)]
fn remote_inventory_command_for(binary: Option<&str>) -> String {
    remote_moonbox_command(binary, &["sessions", "--json"])
}

fn remote_compile_request_command(session_id: &str, target: CliTool) -> String {
    remote_moonbox_command(
        remote_moonbox_binary().as_deref(),
        &[
            "compile-request",
            "--session",
            session_id,
            "--target",
            target.id(),
            "--json",
        ],
    )
}

fn remote_moonbox_command(binary: Option<&str>, args: &[&str]) -> String {
    let args = args
        .iter()
        .map(|arg| shell_quote(arg))
        .collect::<Vec<_>>()
        .join(" ");
    if let Some(binary) = binary {
        return format!("exec {} {args}", shell_quote(binary));
    }
    format!(
        "PATH=\"$HOME/.local/bin:$HOME/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:$PATH\"; \
if command -v moonbox >/dev/null 2>&1; then exec moonbox {args}; \
elif command -v moon >/dev/null 2>&1; then exec moon {args}; \
else echo \"moonbox remote inventory command not found; install moonbox on the remote host or set MOONBOX_REMOTE_BIN=/absolute/path/to/moonbox\" >&2; exit 127; fi"
    )
}

fn remote_moonbox_binary() -> Option<String> {
    env::var("MOONBOX_REMOTE_BIN")
        .ok()
        .filter(|value| !value.trim().is_empty())
}

fn shell_quote(value: &str) -> String {
    if value.bytes().all(|byte| {
        byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'_' | b'-' | b':' | b'+')
    }) {
        return value.into();
    }
    format!("'{}'", value.replace('\'', "'\\''"))
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
    use crate::core::model::{SessionRuntimeStatus, SessionStatus, SourceProvenance};

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
	  *"moonbox sessions --json"*|*"moon sessions --json"*) ;;
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
            ssh_host: Some("devbox.internal".into()),
            ssh_user: None,
            ssh_port: None,
            ssh_identity_file: None,
            config_source: Some("OpenSSH config".into()),
            config_path: Some("/tmp/ssh_config".into()),
        };

        let sessions =
            load_remote_sessions_with_bin(&space, script.to_str().expect("utf-8 script"))
                .expect("remote sessions");

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, "remote-codex-1");
        assert_eq!(sessions[0].status, SessionStatus::Healthy);
        assert_eq!(sessions[0].source_provenance, SourceProvenance::Real);
    }

    #[test]
    fn ssh_target_uses_moonbox_config_connection_fields() {
        let space = DataSpaceEntry {
            id: "ssh:devbox".into(),
            label: "devbox".into(),
            kind: DataSpaceKind::Ssh,
            detail: "yangyang.1205@10.37.218.31".into(),
            ssh_host: Some("10.37.218.31".into()),
            ssh_user: Some("yangyang.1205".into()),
            ssh_port: Some(22),
            ssh_identity_file: Some("~/.ssh/id_ed25519".into()),
            config_source: Some("Moonbox config".into()),
            config_path: None,
        };

        assert_eq!(ssh_target(&space, "devbox"), "yangyang.1205@10.37.218.31");
    }

    #[test]
    fn remote_inventory_command_searches_common_user_install_paths() {
        let command = remote_inventory_command();

        assert!(command.contains("$HOME/.local/bin"));
        assert!(command.contains("$HOME/.cargo/bin"));
        assert!(command.contains("command -v moonbox"));
        assert!(command.contains("command -v moon"));
        assert!(command.contains("moonbox remote inventory command not found"));
    }

    #[test]
    fn remote_inventory_command_honors_explicit_binary_override() {
        let command = remote_inventory_command_for(Some("/opt/moon box/bin/moonbox"));

        assert_eq!(command, "exec '/opt/moon box/bin/moonbox' sessions --json");
    }

    #[cfg(unix)]
    #[test]
    fn remote_timeline_uses_compile_request_without_opening_sessions() {
        let script = env::temp_dir().join(format!(
            "moonbox-fake-ssh-{}-{}",
            std::process::id(),
            "timeline"
        ));
        fs::write(
            &script,
            r#"#!/bin/sh
	case "$*" in
	  *"compile-request --session remote-codex-1 --target hermes --json"*) ;;
	  *) echo "unexpected: $*" >&2; exit 2 ;;
	esac
cat <<'JSON'
{
  "version": 1,
  "source_cli": "codex",
  "target_cli": "hermes",
  "source_session": {
    "id": "remote-codex-1",
    "cli": "codex",
    "title": "Remote task",
    "cwd": "/srv/app",
    "updated_at": "2026-06-07T10:00:00+08:00",
    "updated": "2026-06-07 10:00",
    "status": "healthy",
    "branch": "main",
    "token_count": null,
    "health_reason": null,
    "event_count": 1,
    "resume_command": "codex resume remote-codex-1",
    "source_provenance": "real",
    "source_path": "/remote/session.jsonl",
    "parse_skip_count": 0
  },
  "rewind_event_id": "evt-1",
  "token_budget": 100000,
  "compiler": "engineering-handoff",
  "timeline": {
    "version": 1,
    "source_cli": "codex",
    "source_session": "remote-codex-1",
    "events": [
      {
        "id": "evt-1",
        "time": "10:00",
        "kind": "user",
        "title": "User",
        "detail": "remote hello",
        "metadata": {"note": ""}
      }
    ]
  },
  "redaction": {
    "version": 1,
    "enabled": false,
    "policy": "disabled",
    "secret_scan": false,
    "path_redaction": false,
    "event_allowlist": [],
    "file_allowlist": [],
    "secrets_redacted": 0,
    "paths_redacted": 0,
    "events_removed": 0,
    "prompt_injection_warnings": 0,
    "external_compiler_disclosure": "",
    "warnings": []
  }
}
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
            ssh_host: Some("devbox.internal".into()),
            ssh_user: None,
            ssh_port: None,
            ssh_identity_file: None,
            config_source: Some("OpenSSH config".into()),
            config_path: Some("/tmp/ssh_config".into()),
        };
        let session = SessionSummary {
            id: "remote-codex-1".into(),
            cli: CliTool::Codex,
            title: "Remote task".into(),
            cwd: "/srv/app".into(),
            updated_at: "2026-06-07T10:00:00+08:00".into(),
            updated: "2026-06-07 10:00".into(),
            runtime_status: SessionRuntimeStatus::Unknown,
            runtime_reason: None,
            status: SessionStatus::Healthy,
            branch: Some("main".into()),
            token_count: None,
            health_reason: None,
            event_count: 1,
            resume_command: "codex resume remote-codex-1".into(),
            source_provenance: SourceProvenance::Real,
            source_path: Some("/remote/session.jsonl".into()),
            source_size_bytes: None,
            parse_skip_count: 0,
            provider_metadata: None,
            context_health: None,
            anatomy: None,
        };

        let request = load_remote_compile_request_with_bin(
            &space,
            &session,
            CliTool::Hermes,
            script.to_str().unwrap(),
        )
        .expect("remote timeline");

        assert_eq!(request.timeline.source_session, "remote-codex-1");
        assert_eq!(request.timeline.events.len(), 1);
        assert_eq!(request.timeline.events[0].detail, "remote hello");
    }

    #[cfg(unix)]
    #[test]
    fn remote_session_details_use_remote_anatomy_from_compile_request() {
        let script = env::temp_dir().join(format!(
            "moonbox-fake-ssh-{}-{}",
            std::process::id(),
            "remote-anatomy"
        ));
        fs::write(
            &script,
            r#"#!/bin/sh
	case "$*" in
	  *"compile-request --session remote-codex-1 --target hermes --json"*) ;;
	  *) echo "unexpected: $*" >&2; exit 2 ;;
	esac
cat <<'JSON'
{
  "version": 1,
  "source_cli": "codex",
  "target_cli": "hermes",
  "source_session": {
    "id": "remote-codex-1",
    "cli": "codex",
    "title": "Remote task",
    "cwd": "/srv/app",
    "updated_at": "2026-06-07T10:00:00+08:00",
    "updated": "2026-06-07 10:00",
    "status": "healthy",
    "branch": "main",
    "token_count": 42000,
    "health_reason": null,
    "event_count": 5,
    "resume_command": "codex resume remote-codex-1",
    "source_provenance": "real",
    "source_path": "/remote/session.jsonl",
    "source_size_bytes": 2048,
    "parse_skip_count": 0,
    "anatomy": {
      "status": "ready",
      "scan_scope": "full",
      "source_size_bytes": 2048,
      "analyzed_bytes": 2048,
      "sampled": false,
      "total_lines": 5,
      "malformed_lines": 0,
      "value_signals": [
        {
          "rank": 1,
          "group": "Continuation",
          "label": "Active tail",
          "value": "512B / 2 rows",
          "detail": "remote compact tail"
        }
      ],
      "compact": {
        "label": "context_compacted",
        "line_number": 3,
        "tail_lines": 2,
        "tail_bytes": 512,
        "detail": "remote active tail"
      },
      "size_profile": [
        {"label": "compacted", "count": 1, "bytes": 1024}
      ],
      "event_profile": [
        {"label": "token_count", "count": 1, "bytes": 256}
      ],
      "content_profile": [
        {"label": "content:text", "count": 2, "bytes": 128}
      ]
    }
  },
  "rewind_event_id": "evt-1",
  "token_budget": 100000,
  "compiler": "engineering-handoff",
  "timeline": {
    "version": 1,
    "source_cli": "codex",
    "source_session": "remote-codex-1",
    "events": [
      {
        "id": "evt-1",
        "time": "10:00",
        "kind": "user",
        "title": "User",
        "detail": "remote hello",
        "metadata": {"note": ""}
      }
    ]
  },
  "redaction": {
    "version": 1,
    "enabled": false,
    "policy": "disabled",
    "secret_scan": false,
    "path_redaction": false,
    "event_allowlist": [],
    "file_allowlist": [],
    "secrets_redacted": 0,
    "paths_redacted": 0,
    "events_removed": 0,
    "prompt_injection_warnings": 0,
    "external_compiler_disclosure": "",
    "warnings": []
  }
}
JSON
"#,
        )
        .expect("write fake ssh");
        let mut perms = fs::metadata(&script).expect("metadata").permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&script, perms).expect("chmod");
        let space = remote_fixture_space();
        let session = remote_fixture_session();
        let request = load_remote_compile_request_with_bin(
            &space,
            &session,
            CliTool::Hermes,
            script.to_str().unwrap(),
        )
        .expect("remote compile request");
        let source_session =
            remote_source_session_with_anatomy(&space, session.clone(), request.source_session);

        let anatomy = source_session.anatomy.as_ref().expect("remote anatomy");
        assert_eq!(anatomy.status, SessionAnatomyStatus::Ready);
        assert_eq!(anatomy.scan_scope, "full");
        assert_eq!(
            anatomy.compact.as_ref().map(|compact| compact.tail_lines),
            Some(2)
        );

        let data = data::workbench_data_from_timeline_snapshot(
            source_session,
            vec![session],
            remote_adapter_reports(&space, &[]),
            CliTool::Hermes,
            request.timeline,
        )
        .expect("remote workbench");
        let selected = data
            .sessions
            .iter()
            .find(|session| session.id == "remote-codex-1")
            .expect("selected session");
        assert_eq!(
            selected.anatomy.as_ref().map(|anatomy| anatomy.status),
            Some(SessionAnatomyStatus::Ready)
        );
    }

    #[test]
    fn remote_session_details_mark_old_remote_without_anatomy() {
        let space = remote_fixture_space();
        let fallback = remote_fixture_session();
        let remote = remote_fixture_session();

        let source_session = remote_source_session_with_anatomy(&space, fallback, remote);

        let anatomy = source_session.anatomy.as_ref().expect("fallback anatomy");
        assert_eq!(anatomy.status, SessionAnatomyStatus::Missing);
        assert_eq!(anatomy.scan_scope, "remote-unavailable");
        assert!(
            anatomy
                .notes
                .iter()
                .any(|note| note.contains("upgrade the remote moonbox binary to M92 or newer"))
        );
    }

    #[test]
    fn remote_session_details_keep_inventory_row_when_remote_returns_wrong_session() {
        let space = remote_fixture_space();
        let fallback = remote_fixture_session();
        let mut remote = remote_fixture_session();
        remote.id = "other-session".into();

        let source_session =
            remote_source_session_with_anatomy(&space, fallback.clone(), remote.clone());

        assert_eq!(source_session.id, fallback.id);
        let anatomy = source_session.anatomy.as_ref().expect("fallback anatomy");
        assert_eq!(anatomy.status, SessionAnatomyStatus::Missing);
        assert!(
            anatomy
                .notes
                .iter()
                .any(|note| note.contains("expected") && note.contains("remote-codex-1"))
        );
    }

    fn remote_fixture_space() -> DataSpaceEntry {
        DataSpaceEntry {
            id: "ssh:devbox".into(),
            label: "devbox".into(),
            kind: DataSpaceKind::Ssh,
            detail: "devbox.internal".into(),
            ssh_host: Some("devbox.internal".into()),
            ssh_user: None,
            ssh_port: None,
            ssh_identity_file: None,
            config_source: Some("OpenSSH config".into()),
            config_path: Some("/tmp/ssh_config".into()),
        }
    }

    fn remote_fixture_session() -> SessionSummary {
        SessionSummary {
            id: "remote-codex-1".into(),
            cli: CliTool::Codex,
            title: "Remote task".into(),
            cwd: "/srv/app".into(),
            updated_at: "2026-06-07T10:00:00+08:00".into(),
            updated: "2026-06-07 10:00".into(),
            runtime_status: SessionRuntimeStatus::Unknown,
            runtime_reason: None,
            status: SessionStatus::Healthy,
            branch: Some("main".into()),
            token_count: None,
            health_reason: None,
            event_count: 1,
            resume_command: "codex resume remote-codex-1".into(),
            source_provenance: SourceProvenance::Real,
            source_path: Some("/remote/session.jsonl".into()),
            source_size_bytes: None,
            parse_skip_count: 0,
            provider_metadata: None,
            context_health: None,
            anatomy: None,
        }
    }
}
