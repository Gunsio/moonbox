use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

use rusqlite::{Connection, params};
use serde_json::Value;

fn moonbox_command(test_name: &str) -> Command {
    fixture_safe_command(env!("CARGO_BIN_EXE_moonbox"), test_name)
}

fn moon_command(test_name: &str) -> Command {
    fixture_safe_command(env!("CARGO_BIN_EXE_moon"), test_name)
}

fn fixture_safe_command(binary: &str, test_name: &str) -> Command {
    let home = fixture_home(test_name);
    let codex_home = home.join("codex");
    let claude_home = home.join("claude");
    let hermes_home = home.join("hermes");
    fs::create_dir_all(&codex_home).expect("codex fixture home");
    fs::create_dir_all(&claude_home).expect("claude fixture home");
    fs::create_dir_all(&hermes_home).expect("hermes fixture home");

    let mut command = Command::new(binary);
    command
        .env("MOONBOX_CODEX_HOME", codex_home)
        .env("MOONBOX_CLAUDE_HOME", claude_home)
        .env("MOONBOX_HERMES_HOME", hermes_home)
        .env("MOONBOX_CONFIG", home.join("config.json"))
        .env("MOONBOX_HOME", home.join("moonbox-home"))
        .env("MOONBOX_CAPSULE_STORE", home.join("capsules.sqlite"))
        .env("MOONBOX_LAUNCH_LEDGER", home.join("launches.sqlite"))
        .env("MOONBOX_SSH_CONFIG", home.join(".ssh").join("config"))
        .env("MOONBOX_SESSION_LIMIT", "50")
        .env("MOONBOX_SESSION_SCAN_LIMIT", "500")
        .env("MOONBOX_SESSION_SUMMARY_LINE_LIMIT", "800");

    for key in [
        "CODEX_HOME",
        "CLAUDE_HOME",
        "HERMES_HOME",
        "MOONBOX_COMPILER",
        "MOONBOX_COMPILER_ID",
        "MOONBOX_COMPILER_ARGS",
        "MOONBOX_COMPILER_TIMEOUT_MS",
        "MOONBOX_HOOK_SPOOL",
        "MOONBOX_CODEX_BIN",
        "MOONBOX_CODEX_APP_SERVER_FIXTURE",
        "MOONBOX_CODEX_APP_SERVER_PROXY",
        "MOONBOX_CODEX_APP_SERVER_SOCKET",
        "MOONBOX_CODEX_APP_SERVER_TIMEOUT_MS",
        "MOONBOX_CLAUDE_BIN",
        "MOONBOX_HERMES_BIN",
        "MOONBOX_REDACTION",
        "MOONBOX_REDACTION_EVENT_ALLOWLIST",
        "MOONBOX_REDACTION_FILE_ALLOWLIST",
        "MOONBOX_SESSION_MODE",
    ] {
        command.env_remove(key);
    }

    command
}

fn fixture_home(test_name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("cli-contract-home")
        .join(std::process::id().to_string())
        .join(test_name)
}

fn output_text(output: Output) -> String {
    assert!(
        output.status.success(),
        "command failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("utf8 stdout")
}

fn error_text(output: Output) -> String {
    assert!(
        !output.status.success(),
        "command unexpectedly succeeded\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn output_json(output: Output) -> Value {
    let text = output_text(output);
    serde_json::from_str(&text).unwrap_or_else(|error| {
        panic!("invalid json: {error}\nstdout:\n{text}");
    })
}

fn write_file(root: &Path, relative: &str, contents: &str) {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("file parent");
    }
    fs::write(path, contents).expect("fixture file");
}

fn write_executable_script(root: &Path, relative: &str, contents: &str) -> PathBuf {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("script parent");
    }
    fs::write(&path, contents).expect("script file");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mut permissions = fs::metadata(&path).expect("script metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions).expect("script permissions");
    }
    path
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

fn write_codex_thread_index(root: &Path, rollout_path: &Path, id: &str, title: &str) {
    let db = Connection::open(root.join("state_5.sqlite")).expect("codex state db");
    db.execute_batch(
        r#"
        create table threads (
            id text primary key,
            rollout_path text not null,
            created_at integer not null,
            updated_at integer not null,
            created_at_ms integer,
            updated_at_ms integer,
            cwd text not null,
            title text not null,
            preview text not null default '',
            first_user_message text not null default '',
            git_branch text,
            tokens_used integer not null default 0,
            archived integer not null default 0
        );
        "#,
    )
    .expect("codex state schema");
    db.execute(
        r#"
        insert into threads (
            id,
            rollout_path,
            created_at,
            updated_at,
            created_at_ms,
            updated_at_ms,
            cwd,
            title,
            preview,
            first_user_message,
            git_branch,
            tokens_used,
            archived
        ) values (?1, ?2, 0, 0, 1780736400000, 1780736400000, ?3, ?4, '', '', ?5, 0, 0)
        "#,
        params![
            id,
            rollout_path.display().to_string(),
            "/tmp/moonbox-renamed",
            title,
            "rename-branch"
        ],
    )
    .expect("codex state row");
}

fn write_hermes_state_db(root: &Path) {
    fs::create_dir_all(root).expect("hermes home");
    let db = Connection::open(root.join("state.db")).expect("hermes state db");
    db.execute_batch(
        r#"
        create table sessions (
            id text primary key,
            source text not null,
            user_id text,
            model text,
            model_config text,
            system_prompt text,
            parent_session_id text,
            started_at real not null,
            ended_at real,
            end_reason text,
            message_count integer default 0,
            tool_call_count integer default 0,
            input_tokens integer default 0,
            output_tokens integer default 0,
            cache_read_tokens integer default 0,
            cache_write_tokens integer default 0,
            reasoning_tokens integer default 0,
            cwd text,
            title text,
            handoff_state text,
            handoff_platform text,
            handoff_error text,
            rewind_count integer not null default 0,
            archived integer not null default 0
        );
        create table messages (
            id integer primary key autoincrement,
            session_id text not null,
            role text not null,
            content text,
            tool_calls text,
            tool_name text,
            timestamp real not null,
            token_count integer,
            finish_reason text,
            reasoning text,
            reasoning_content text,
            reasoning_details text,
            active integer not null default 1
        );
        insert into sessions (
            id, source, user_id, model, model_config, system_prompt, parent_session_id,
            started_at, message_count, tool_call_count, input_tokens, output_tokens,
            cache_read_tokens, cache_write_tokens, reasoning_tokens, cwd, title,
            handoff_state, handoff_platform, archived
        ) values
            ('hermes-cli-real', 'cli', 'local-user', 'gpt-5', '{"temperature":0.1}', 'CLI system prompt', null, 1780640474, 2, 0, 8, 7, 0, 0, 0, '/repo', 'CLI session', null, null, 0),
            ('hermes-feishu-real', 'feishu', 'ou_123', 'claude-sonnet', '{"mode":"ops"}', 'Feishu system prompt', 'parent-feishu', 1780641494, 3, 1, 10, 20, 3, 4, 5, null, null, 'ready', 'feishu', 0),
            ('hermes-discord-archived', 'discord', 'du_123', 'gpt-5', null, null, null, 1780649999, 1, 0, 1, 1, 0, 0, 0, null, 'Archived Discord', null, null, 1);
        insert into messages (session_id, role, content, timestamp, active) values
            ('hermes-cli-real', 'user', 'Fix CLI source', 1780640475, 1),
            ('hermes-cli-real', 'assistant', 'Done', 1780640476, 1),
            ('hermes-feishu-real', 'session_meta', 'Feishu source context', 1780641494, 1),
            ('hermes-feishu-real', 'user', 'Investigate Feishu gateway', 1780641495, 1),
            ('hermes-feishu-real', 'assistant', 'Gateway snippet confirmed', 1780641496, 1),
            ('hermes-feishu-real', 'user', 'Close unrelated item', 1780641497, 1);
        "#,
    )
    .expect("hermes schema");

    let sessions_json = root.join("sessions").join("sessions.json");
    fs::create_dir_all(sessions_json.parent().expect("sessions parent")).expect("sessions dir");
    fs::write(
        sessions_json,
        r#"{
  "agent:main:feishu:dm:chat": {
    "session_id": "hermes-feishu-real",
    "session_key": "agent:main:feishu:dm:chat",
    "display_name": "Feishu Ops",
    "platform": "feishu",
    "chat_type": "dm",
    "total_tokens": 42,
    "suspended": false,
    "resume_pending": false,
    "expiry_finalized": false,
    "origin": {"chat_name": "Feishu Ops", "thread_ts": "t-1"}
  }
}"#,
    )
    .expect("hermes sessions json");
}

fn codex_app_server_fixture_json() -> &'static str {
    r#"{
      "responses": [
        {"method":"thread/list","result":{"data":[{
          "cliVersion":"0.0.0-test",
          "createdAt":1780732800,
          "cwd":"/tmp/moonbox-app-server",
          "ephemeral":false,
          "id":"codex-app-cli",
          "modelProvider":"openai",
          "name":"Codex app-server CLI fixture",
          "preview":"Use app-server for inventory",
          "sessionId":"codex-app-cli",
          "source":"cli",
          "status":{"type":"active","activeFlags":[]},
          "turns":[],
          "updatedAt":1780736400,
          "gitInfo":{"branch":"main"}
        }]}},
        {"method":"thread/read","thread_id":"codex-app-cli","result":{"thread":{
          "cliVersion":"0.0.0-test",
          "createdAt":1780732800,
          "cwd":"/tmp/moonbox-app-server",
          "ephemeral":false,
          "id":"codex-app-cli",
          "modelProvider":"openai",
          "name":"Codex app-server CLI fixture",
          "preview":"Use app-server for inventory",
          "sessionId":"codex-app-cli",
          "source":"cli",
          "status":{"type":"active","activeFlags":[]},
          "turns":[],
          "updatedAt":1780736400,
          "gitInfo":{"branch":"main"}
        }}}
      ]
    }"#
}

#[test]
fn moonbox_and_moon_expose_the_same_version_contract() {
    let moonbox = output_text(
        moonbox_command("version-contract")
            .arg("--version")
            .output()
            .expect("moonbox version"),
    );
    let moon = output_text(
        moon_command("version-contract")
            .arg("--version")
            .output()
            .expect("moon version"),
    );

    assert!(moonbox.contains("moonbox"));
    assert_eq!(moonbox, moon);
}

#[test]
fn completion_generation_uses_requested_or_invoked_binary_name() {
    let bash = output_text(
        moonbox_command("completion-moonbox-bash")
            .args(["completions", "bash"])
            .output()
            .expect("moonbox bash completions"),
    );
    assert!(bash.contains("_moonbox"));
    assert!(bash.contains("moonbox"));
    assert!(bash.contains("replay-eval"));
    assert!(bash.contains("completions"));
    assert!(bash.contains("open-app"));
    assert!(bash.contains("snapshot"));
    assert!(bash.contains("ssh"));
    assert!(bash.contains("hooks"));
    assert!(bash.contains("hook-event"));

    let fish = output_text(
        moon_command("completion-moon-fish")
            .args(["completions", "fish"])
            .output()
            .expect("moon fish completions"),
    );
    assert!(fish.contains("complete -c moon"));
    assert!(fish.contains("replay-eval"));
    assert!(fish.contains("completions"));
    assert!(fish.contains("open-app"));
    assert!(fish.contains("snapshot"));
    assert!(fish.contains("ssh"));
    assert!(fish.contains("hooks"));
    assert!(fish.contains("hook-event"));

    let zsh = output_text(
        moonbox_command("completion-explicit-moon-zsh")
            .args(["completions", "--bin", "moon", "zsh"])
            .output()
            .expect("explicit moon zsh completions"),
    );
    assert!(zsh.contains("#compdef moon"));
    assert!(zsh.contains("replay-eval"));
    assert!(zsh.contains("completions"));
    assert!(zsh.contains("open-app"));
    assert!(zsh.contains("snapshot"));
    assert!(zsh.contains("ssh"));
    assert!(zsh.contains("hooks"));
    assert!(zsh.contains("hook-event"));
}

#[test]
fn snapshot_cli_contract_captures_isolated_workspace_state() {
    let test_name = "snapshot-workspace";
    let repo = fixture_home(test_name).join("repo");
    fs::create_dir_all(&repo).expect("snapshot repo");
    git(&repo, &["init"]);
    write_file(&repo, "README.md", "initial\n");
    git(&repo, &["add", "README.md"]);
    git(
        &repo,
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
    write_file(&repo, "README.md", "initial\nchanged\n");
    write_file(&repo, "AGENTS.md", "handoff rules\n");
    git(&repo, &["add", "AGENTS.md"]);
    write_file(&repo, "scratch.txt", "untracked\n");

    let repo_arg = repo.to_str().expect("repo path");
    let json = output_json(
        moon_command(test_name)
            .args([
                "snapshot",
                "--json",
                "--path",
                repo_arg,
                "--diff-lines",
                "20",
                "--test-command",
                "printf snapshot-cli",
            ])
            .output()
            .expect("snapshot json"),
    );

    assert_eq!(json["version"], 1);
    assert_eq!(json["git"]["available"], true);
    assert_eq!(json["git"]["dirty"], true);
    assert!(json["git"]["head"].as_str().expect("head").len() >= 12);
    assert!(!json["git"]["branch"].as_str().expect("branch").is_empty());
    assert_eq!(json["git"]["staged"][0], "AGENTS.md");
    assert_eq!(json["git"]["unstaged"][0], "README.md");
    assert_eq!(json["git"]["untracked"][0], "scratch.txt");
    assert_eq!(json["test_commands"][0]["success"], true);
    assert_eq!(json["test_commands"][0]["stdout"], "snapshot-cli");
    assert!(
        json["key_files"]
            .as_array()
            .expect("key files")
            .iter()
            .any(|file| file["path"] == "README.md")
    );

    let text = output_text(
        moon_command("snapshot-workspace-text")
            .args(["snapshot", "--path", repo_arg])
            .output()
            .expect("snapshot text"),
    );
    assert!(text.contains("workspace snapshot: v1"));
    assert!(text.contains("dirty: true"));
    assert!(text.contains("staged: 1"));
    assert!(text.contains("unstaged: 1"));
    assert!(text.contains("untracked: 1"));
}

#[test]
fn ssh_cli_contract_lists_configured_hosts_without_connecting() {
    let home = fixture_home("ssh-list");
    fs::create_dir_all(home.join(".ssh")).expect("ssh fixture dir");
    fs::write(
        home.join("config.json"),
        r#"{
  "ssh_hosts": [
    {"name": "prod-api", "host": "prod-api.internal", "user": "deploy", "port": 2222, "identity_file": "~/.ssh/prod-api", "tags": ["prod"]}
  ]
}"#,
    )
    .expect("moonbox ssh config");
    fs::write(
        home.join(".ssh").join("config"),
        r#"
Host dev-box
  HostName dev-box.internal
  User dev
  Port 2200

Host *
  User ignored
"#,
    )
    .expect("openssh config");

    let json = output_json(
        moon_command("ssh-list")
            .args(["ssh", "--json"])
            .output()
            .expect("ssh json"),
    );
    let hosts = json.as_array().expect("ssh host array");

    assert_eq!(hosts.len(), 2);
    assert!(hosts.iter().any(|host| {
        host["name"] == "prod-api"
            && host["host"] == "prod-api.internal"
            && host["source"] == "moonbox_config"
    }));
    assert!(hosts.iter().any(|host| {
        host["name"] == "dev-box"
            && host["host"] == "dev-box.internal"
            && host["source"] == "openssh_config"
    }));

    let text = output_text(
        moon_command("ssh-list")
            .arg("ssh")
            .output()
            .expect("ssh text"),
    );
    assert!(text.contains("SSH hosts: 2"));
    assert!(text.contains("prod-api"));
    assert!(text.contains("target deploy@prod-api.internal:2222  source moonbox"));
    assert!(text.contains("identity ~/.ssh/prod-api"));
    assert!(text.contains("dev-box"));
    assert!(text.contains("target dev@dev-box.internal:2200  source openssh"));
    assert!(!text.contains("cli-contract-home"));
}

#[test]
fn hooks_cli_is_preview_first_and_uninstalls_only_moonbox_entries() {
    let test_name = "hooks-config";
    let home = fixture_home(test_name);
    fs::create_dir_all(home.join("claude")).expect("claude fixture dir");
    fs::create_dir_all(home.join("codex")).expect("codex fixture dir");
    fs::write(
        home.join("claude").join("settings.json"),
        r#"{
  "hooks": {
    "Stop": [
      {"hooks": [{"type": "command", "command": "/bin/echo keep-claude"}]}
    ]
  }
}"#,
    )
    .expect("claude settings");
    fs::write(
        home.join("codex").join("hooks.json"),
        r#"{
  "hooks": {
    "Stop": [
      {"hooks": [{"type": "command", "command": "/bin/echo keep-codex"}]}
    ]
  }
}"#,
    )
    .expect("codex hooks");
    fs::write(
        home.join("codex").join("config.toml"),
        "[features]\nhooks = false\n",
    )
    .expect("codex config");

    let preview = output_json(
        moonbox_command(test_name)
            .args(["hooks", "install", "--json"])
            .output()
            .expect("hooks preview"),
    );
    assert_eq!(preview["dry_run"], true);
    assert!(!home.join("config.json").exists());
    assert!(
        !fs::read_to_string(home.join("claude").join("settings.json"))
            .expect("claude after preview")
            .contains("hook-event")
    );

    let applied = output_json(
        moonbox_command(test_name)
            .args(["hooks", "install", "--apply", "--json"])
            .output()
            .expect("hooks install"),
    );
    assert_eq!(applied["dry_run"], false);
    assert_eq!(applied["moonbox_enabled_after"], true);
    assert!(
        applied["providers"]
            .as_array()
            .expect("providers")
            .iter()
            .all(|provider| provider["changed"] == true)
    );
    let status = output_json(
        moonbox_command(test_name)
            .args(["hooks", "status", "--json"])
            .output()
            .expect("hooks status"),
    );
    assert_eq!(status["moonbox_enabled"], true);
    assert_eq!(status["providers"][1]["feature_enabled"], false);
    assert_eq!(status["providers"][0]["installed"], true);
    assert_eq!(status["providers"][1]["installed"], true);

    let reinstall = output_json(
        moonbox_command(test_name)
            .args(["hooks", "install", "--apply", "--json"])
            .output()
            .expect("hooks reinstall"),
    );
    assert!(
        reinstall["providers"]
            .as_array()
            .expect("providers")
            .iter()
            .all(|provider| provider["changed"] == false)
    );

    let removed = output_json(
        moonbox_command(test_name)
            .args(["hooks", "uninstall", "--apply", "--json"])
            .output()
            .expect("hooks uninstall"),
    );
    assert_eq!(removed["moonbox_enabled_after"], false);
    let claude = fs::read_to_string(home.join("claude").join("settings.json"))
        .expect("claude after uninstall");
    let codex =
        fs::read_to_string(home.join("codex").join("hooks.json")).expect("codex after uninstall");
    assert!(claude.contains("keep-claude"));
    assert!(codex.contains("keep-codex"));
    assert!(!claude.contains("hook-event"));
    assert!(!codex.contains("hook-event"));
}

#[test]
fn hook_event_appends_to_isolated_spool_and_exits_silent() {
    let test_name = "hooks-spool";
    let home = fixture_home(test_name);
    fs::create_dir_all(&home).expect("home dir");
    fs::write(
        home.join("config.json"),
        r#"{"hooks":{"enabled":true,"spool_max_bytes":4096,"spool_max_files":2}}"#,
    )
    .expect("hooks config");

    let mut child = moonbox_command(test_name)
        .args(["hook-event", "--cli", "codex"])
        .env("TMUX", "/tmp/tmux-501/default,1,0")
        .env("TMUX_PANE", "%42")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("hook-event spawn");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(
            br#"{"session_id":"s1","hook_event_name":"PermissionRequest","cwd":"/repo","transcript_path":"/tmp/session.jsonl"}"#,
        )
        .expect("write hook stdin");
    let output = child.wait_with_output().expect("hook output");
    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout).expect("stdout"), "");

    let spool = home.join("moonbox-home").join("spool").join("events.jsonl");
    let contents = fs::read_to_string(spool).expect("spool contents");
    let line = contents.lines().next().expect("spool line");
    let event: Value = serde_json::from_str(line).expect("spool json");
    assert_eq!(event["version"], 1);
    assert_eq!(event["cli"], "codex");
    assert_eq!(event["session_id"], "s1");
    assert_eq!(event["hook_event_name"], "PermissionRequest");
    assert_eq!(event["tmux_pane"], "%42");
    assert_eq!(event["event"]["transcript_path"], "/tmp/session.jsonl");
}

#[test]
fn docs_snapshot_is_hidden_fixture_safe_and_generated() {
    let help = output_text(
        moonbox_command("docs-snapshot-help")
            .arg("--help")
            .output()
            .expect("moonbox help"),
    );
    assert!(!help.contains("docs-snapshot"));

    let svg = output_text(
        moonbox_command("docs-snapshot")
            .arg("docs-snapshot")
            .output()
            .expect("docs snapshot"),
    );

    assert!(svg.starts_with("<svg "));
    assert!(svg.contains("Handoff Review"));
    assert!(svg.contains("Capsule"));
    assert!(svg.contains("Draft Handoff"));
    assert!(svg.contains("Target receives"));
    assert!(svg.contains("Run local target"));
}

#[test]
fn replay_eval_cli_contract_is_fixture_only() {
    let report = output_json(
        moonbox_command("replay-eval")
            .args(["replay-eval", "--json"])
            .output()
            .expect("replay eval"),
    );

    assert_eq!(report["fixture_only"], true);
    assert_eq!(report["source_count"], 3);
    assert_eq!(report["target_count"], 3);
    assert_eq!(report["matrix_case_count"], 9);
    assert_eq!(report["synthetic_case_count"], 3);
    assert_eq!(report["case_count"], 12);
    assert_eq!(report["coverage_count"], 5);
    assert_eq!(report["pipeline_passed"], true);
    assert!(
        report["coverage"]
            .as_array()
            .expect("coverage")
            .iter()
            .all(|coverage| coverage["covered"] == true)
    );
}

#[test]
fn doctor_cli_contract_is_non_executing_and_fixture_safe() {
    let binary = env!("CARGO_BIN_EXE_moonbox");
    let report = output_json(
        moonbox_command("doctor")
            .arg("doctor")
            .arg("--json")
            .env("MOONBOX_CODEX_BIN", binary)
            .env("MOONBOX_CLAUDE_BIN", binary)
            .env("MOONBOX_HERMES_BIN", binary)
            .output()
            .expect("doctor"),
    );

    assert_eq!(report["version"], 1);
    assert_eq!(report["ready"], true);
    assert_eq!(report["status"], "pass");

    let checks = report["checks"].as_array().expect("checks");
    for name in [
        "config_file",
        "hooks_event_channel",
        "source_codex_adapter",
        "source_claude_adapter",
        "source_hermes_adapter",
        "session_discovery",
        "target_codex_binary",
        "target_claude_binary",
        "target_hermes_binary",
        "compiler_catalog",
    ] {
        assert!(
            checks.iter().any(|check| check["name"] == name),
            "missing doctor check {name}: {report:#}"
        );
    }

    let adapters = report["source_adapters"]
        .as_array()
        .expect("source adapters");
    assert_eq!(adapters.len(), 3);
    assert!(
        adapters
            .iter()
            .all(|adapter| adapter["provenance"] == "fixture")
    );
    assert!(
        adapters
            .iter()
            .all(|adapter| adapter["active"] == true && adapter["session_count"] == 1)
    );
    assert!(
        adapters
            .iter()
            .all(|adapter| adapter["scan_entry_count"] == 1
                && adapter["scan_truncated"] == false
                && adapter["summary_line_limit"].is_null()
                && adapter["scan_entry_limit"].is_null())
    );
    for adapter in adapters {
        let capabilities = &adapter["capabilities"];
        assert_eq!(adapter["fidelity"]["status"], "fallback");
        assert_eq!(adapter["fidelity"]["primary_surface"], "embedded_fixture");
        assert!(adapter["fidelity"]["fallback_surface"].is_null());
        assert_eq!(capabilities["version"], 1);
        assert_eq!(capabilities["local_store"]["status"], "available");
        assert_eq!(capabilities["native_handoff"]["status"], "unavailable");
        assert_eq!(capabilities["fork_resume"]["status"], "unavailable");
    }
    for check in checks.iter().filter(|check| {
        check["name"]
            .as_str()
            .is_some_and(|name| name.starts_with("source_"))
    }) {
        let detail = check["detail"].as_str().expect("source check detail");
        assert!(detail.contains("fidelity=fallback"));
        assert!(detail.contains("surface=embedded_fixture"));
        assert!(detail.contains("capabilities=local_store:available"));
        assert!(detail.contains("native_handoff:unavailable"));
    }
}

#[test]
fn session_listing_uses_fixture_fallback_when_source_homes_are_isolated() {
    let sessions = output_json(
        moonbox_command("sessions")
            .args(["sessions", "--json"])
            .output()
            .expect("sessions"),
    );
    let ids = sessions
        .as_array()
        .expect("session array")
        .iter()
        .map(|session| session["id"].as_str().expect("session id"))
        .collect::<Vec<_>>();

    assert_eq!(
        ids,
        ["codex-cxcp-design", "claude-qc-platform", "hermes-cxcp-502"]
    );
    assert!(
        sessions.as_array().expect("session array").iter().all(
            |session| session["source_provenance"] == "fixture"
                && session["parse_skip_count"] == 0
                && session["runtime_status"] == "unknown"
                && session["runtime_reason"]
                    .as_str()
                    .is_some_and(|reason| !reason.trim().is_empty())
                && session["source_path"]
                    .as_str()
                    .expect("source path")
                    .starts_with("fixtures/adapters/")
        )
    );
}

#[test]
fn session_listing_source_filter_matches_global_entry_model() {
    let sessions = output_json(
        moonbox_command("sessions-filter")
            .args(["sessions", "--json", "--filter", "hermes"])
            .output()
            .expect("sessions"),
    );
    let sessions = sessions.as_array().expect("session array");

    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0]["id"], "hermes-cxcp-502");
    assert_eq!(sessions[0]["cli"], "hermes");
}

#[test]
fn hermes_real_store_lists_all_sources_and_filters_by_provider_source() {
    let test_name = "hermes-all-source-inventory";
    let home = fixture_home(test_name);
    let hermes_home = home.join("hermes");
    write_hermes_state_db(&hermes_home);

    let all = output_json(
        moonbox_command(test_name)
            .args(["sessions", "--json", "--filter", "hermes"])
            .output()
            .expect("sessions all"),
    );
    let all = all.as_array().expect("session array");
    let ids = all
        .iter()
        .map(|session| session["id"].as_str().expect("id"))
        .collect::<Vec<_>>();

    assert_eq!(ids, ["hermes-feishu-real", "hermes-cli-real"]);
    assert!(!ids.contains(&"hermes-discord-archived"));
    assert_eq!(all[0]["provider_metadata"]["source"], "feishu");
    assert_eq!(all[0]["provider_metadata"]["platform"], "feishu");
    assert_eq!(all[0]["provider_metadata"]["user_id"], "ou_123");
    assert_eq!(
        all[0]["provider_metadata"]["session_key"],
        "agent:main:feishu:dm:chat"
    );
    assert_eq!(
        all[0]["provider_metadata"]["parent_session_id"],
        "parent-feishu"
    );
    assert_eq!(all[0]["provider_metadata"]["token_breakdown"]["total"], 42);
    assert_eq!(all[0]["provider_metadata"]["token_breakdown"]["input"], 10);
    assert_eq!(all[0]["provider_metadata"]["token_breakdown"]["output"], 20);
    assert_eq!(all[0]["provider_metadata"]["handoff"]["state"], "ready");
    assert_eq!(
        all[0]["provider_metadata"]["origin"]["chat_name"],
        "Feishu Ops"
    );
    assert!(
        all[0]["provider_metadata"]["system_prompt_snapshot"]
            .as_str()
            .expect("system prompt")
            .contains("Feishu system")
    );

    let cli_only = output_json(
        moonbox_command(test_name)
            .args([
                "sessions",
                "--json",
                "--filter",
                "hermes",
                "--hermes-source",
                "cli",
            ])
            .output()
            .expect("sessions cli"),
    );
    let cli_only = cli_only.as_array().expect("cli sessions");

    assert_eq!(cli_only.len(), 1);
    assert_eq!(cli_only[0]["id"], "hermes-cli-real");
    assert_eq!(cli_only[0]["provider_metadata"]["source"], "cli");

    let api_server = output_json(
        moonbox_command(test_name)
            .args(["sessions", "--json", "--hermes-source", "api-server"])
            .output()
            .expect("sessions api-server"),
    );

    assert_eq!(api_server.as_array().expect("api sessions").len(), 0);

    let search = output_json(
        moonbox_command(test_name)
            .args([
                "sessions",
                "--json",
                "--filter",
                "hermes",
                "--hermes-search",
                "Investigate Feishu gateway",
                "--hermes-search-limit",
                "1",
            ])
            .output()
            .expect("sessions search"),
    );
    let search = search.as_array().expect("search sessions");

    assert_eq!(search.len(), 1);
    assert_eq!(search[0]["id"], "hermes-feishu-real");
    assert_eq!(
        search[0]["provider_metadata"]["search"]["backend"],
        "local_sqlite_like"
    );
    assert_eq!(
        search[0]["provider_metadata"]["search"]["matched_message_count"],
        1
    );
    assert_eq!(
        search[0]["provider_metadata"]["search"]["continuation_point_count"],
        1
    );
    let point = &search[0]["provider_metadata"]["continuation_points"][0];
    assert_eq!(point["message_id"], "4");
    assert_eq!(point["event_id"], "evt-002");
    assert_eq!(point["role"], "user");
    assert!(
        point["snippet"]
            .as_str()
            .expect("snippet")
            .contains("Investigate Feishu gateway")
    );
    assert_eq!(point["bookend_before"], "Feishu source context");
    assert_eq!(point["bookend_after"], "Gateway snippet confirmed");
    assert_eq!(point["scroll_context"]["message_index"], 2);
    assert_eq!(point["scroll_context"]["total_messages"], 4);
    assert_eq!(point["scroll_context"]["before_message_id"], "3");
    assert_eq!(point["scroll_context"]["after_message_id"], "5");

    let binary = env!("CARGO_BIN_EXE_moonbox");
    let doctor = output_json(
        moonbox_command(test_name)
            .arg("doctor")
            .arg("--json")
            .env("MOONBOX_CODEX_BIN", binary)
            .env("MOONBOX_CLAUDE_BIN", binary)
            .env("MOONBOX_HERMES_BIN", binary)
            .output()
            .expect("doctor"),
    );
    let hermes = doctor["source_adapters"]
        .as_array()
        .expect("adapters")
        .iter()
        .find(|adapter| adapter["cli"] == "hermes")
        .expect("hermes adapter")
        .clone();

    assert_eq!(hermes["session_count"], 2);
    assert_eq!(hermes["fidelity"]["status"], "fallback");
    assert_eq!(hermes["fidelity"]["primary_surface"], "hermes_local_sqlite");
    assert_eq!(
        hermes["fidelity"]["fallback_surface"],
        "hermes_gateway_export_search"
    );
    assert_eq!(
        hermes["capabilities"]["rich_local_rpc"]["status"],
        "available"
    );
    assert_eq!(
        hermes["capabilities"]["cloud_metadata"]["status"],
        "available"
    );
    assert_eq!(
        hermes["capabilities"]["export_search"]["status"],
        "available"
    );
    assert!(
        hermes["capabilities"]["export_search"]["detail"]
            .as_str()
            .expect("export detail")
            .contains("message ids")
    );
}

#[test]
fn codex_sessions_json_uses_renamed_thread_name_from_session_index() {
    let test_name = "codex-renamed-thread-name";
    let home = fixture_home(test_name);
    let codex_home = home.join("codex");
    let rollout_path = codex_home
        .join("sessions")
        .join("2026")
        .join("06")
        .join("08")
        .join("rollout-2026-06-08T10-00-00-codex-renamed.jsonl");
    fs::create_dir_all(rollout_path.parent().expect("rollout parent")).expect("codex sessions");
    fs::write(
        &rollout_path,
        r#"{"timestamp":"2026-06-08T10:00:00Z","type":"session_meta","payload":{"id":"codex-renamed-cli","cwd":"/tmp/moonbox-renamed","git":{"branch":"rename-branch"}}}
{"timestamp":"2026-06-08T10:01:00Z","type":"response_item","payload":{"role":"user","content":[{"type":"input_text","text":"old url title"}]}}"#,
    )
    .expect("codex rollout");
    write_codex_thread_index(
        &codex_home,
        &rollout_path,
        "codex-renamed-cli",
        "https://bytedance.larkoffice.com/wiki/old-title",
    );
    fs::write(
        codex_home.join("session_index.jsonl"),
        r#"{"id":"codex-renamed-cli","thread_name":"102_303","updated_at":"2026-06-08T10:05:00Z"}"#,
    )
    .expect("codex session index");

    let sessions = output_json(
        moonbox_command(test_name)
            .args(["sessions", "--json", "--filter", "codex"])
            .output()
            .expect("sessions"),
    );
    let sessions = sessions.as_array().expect("session array");

    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0]["id"], "codex-renamed-cli");
    assert_eq!(sessions[0]["title"], "102_303");
    assert_ne!(
        sessions[0]["title"],
        "https://bytedance.larkoffice.com/wiki/old-title"
    );
}

#[test]
fn codex_app_server_fixture_is_preferred_source_and_open_app_preview() {
    let test_name = "codex-app-server-fixture";
    let home = fixture_home(test_name);
    let fixture_path = home.join("codex-app-server.json");
    write_file(
        &home,
        "codex-app-server.json",
        codex_app_server_fixture_json(),
    );

    let sessions = output_json(
        moonbox_command(test_name)
            .args(["sessions", "--json", "--filter", "codex"])
            .env("MOONBOX_CODEX_APP_SERVER_FIXTURE", &fixture_path)
            .output()
            .expect("sessions"),
    );
    let sessions = sessions.as_array().expect("session array");

    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0]["id"], "codex-app-cli");
    assert_eq!(sessions[0]["title"], "Codex app-server CLI fixture");
    assert_eq!(sessions[0]["source_provenance"], "real");
    assert_eq!(sessions[0]["runtime_status"], "active");
    assert_eq!(
        sessions[0]["source_path"],
        "codex-app-server://threads/codex-app-cli"
    );

    let open_app = output_json(
        moonbox_command(test_name)
            .args(["open-app", "--session", "codex-app-cli", "--json"])
            .env("MOONBOX_CODEX_APP_SERVER_FIXTURE", &fixture_path)
            .output()
            .expect("open-app"),
    );
    assert_eq!(open_app["action"], "app_deep_link");
    assert_eq!(open_app["dry_run"], true);
    assert_eq!(open_app["supported"], true);
    assert_eq!(open_app["deep_link"], "codex://threads/codex-app-cli");
    assert!(open_app.get("command").is_none());

    let binary = env!("CARGO_BIN_EXE_moonbox");
    let doctor = output_json(
        moonbox_command(test_name)
            .arg("doctor")
            .arg("--json")
            .env("MOONBOX_CODEX_APP_SERVER_FIXTURE", &fixture_path)
            .env("MOONBOX_CODEX_BIN", binary)
            .env("MOONBOX_CLAUDE_BIN", binary)
            .env("MOONBOX_HERMES_BIN", binary)
            .output()
            .expect("doctor"),
    );
    let adapters = doctor["source_adapters"]
        .as_array()
        .expect("source adapters");
    let codex = adapters
        .iter()
        .find(|adapter| adapter["cli"] == "codex")
        .expect("codex adapter");

    assert_eq!(codex["filter_status"], "included_codex_app_server");
    assert_eq!(codex["fidelity"]["status"], "full_fidelity");
    assert_eq!(
        codex["fidelity"]["primary_surface"],
        "codex_app_server_thread_api"
    );
    assert!(codex["fidelity"]["fallback_surface"].is_null());
    assert_eq!(
        codex["capabilities"]["rich_local_rpc"]["status"],
        "available"
    );
    assert_eq!(codex["capabilities"]["deep_link"]["status"], "available");
    assert_eq!(
        codex["capabilities"]["local_store"]["status"],
        "unavailable"
    );
}

#[test]
fn auto_mode_does_not_mix_real_sessions_with_missing_source_fixtures() {
    let test_name = "real-store-no-fixture-mix";
    let home = fixture_home(test_name);
    let codex_store = home.join("codex").join("sessions").join("2026");
    fs::create_dir_all(&codex_store).expect("codex store");
    fs::write(
        codex_store.join("real-codex.jsonl"),
        r#"{"timestamp":"2026-06-06T10:00:00Z","type":"session_meta","payload":{"id":"codex-real-isolated","cwd":"/tmp/moonbox-real","git":{"branch":"main"}}}
{"timestamp":"2026-06-06T10:01:00Z","type":"response_item","payload":{"role":"user","content":[{"type":"input_text","text":"Use real Codex store only"}]}}
not-json"#,
    )
    .expect("codex jsonl");

    let sessions = output_json(
        moonbox_command(test_name)
            .args(["sessions", "--json"])
            .output()
            .expect("sessions"),
    );
    let ids = sessions
        .as_array()
        .expect("session array")
        .iter()
        .map(|session| session["id"].as_str().expect("session id"))
        .collect::<Vec<_>>();

    assert_eq!(ids, ["codex-real-isolated"]);
    assert!(!ids.contains(&"claude-qc-platform"));
    assert!(!ids.contains(&"hermes-cxcp-502"));
    let session = &sessions.as_array().expect("session array")[0];
    assert_eq!(session["source_provenance"], "real");
    assert_eq!(session["parse_skip_count"], 1);
    assert_eq!(session["runtime_status"], "unknown");
    assert!(
        session["runtime_reason"]
            .as_str()
            .is_some_and(|reason| reason.contains("does not expose live runtime activity"))
    );
    assert!(
        session["source_path"]
            .as_str()
            .expect("source path")
            .ends_with("real-codex.jsonl")
    );
}

#[test]
fn doctor_reports_real_and_missing_source_adapters_without_fixture_mixing() {
    let test_name = "doctor-real-store-report";
    let home = fixture_home(test_name);
    let codex_store = home.join("codex").join("sessions").join("2026");
    fs::create_dir_all(&codex_store).expect("codex store");
    fs::write(
        codex_store.join("real-codex.jsonl"),
        r#"{"timestamp":"2026-06-06T10:00:00Z","type":"session_meta","payload":{"id":"codex-real-isolated","cwd":"/tmp/moonbox-real","git":{"branch":"main"}}}
{"timestamp":"2026-06-06T10:01:00Z","type":"response_item","payload":{"role":"user","content":[{"type":"input_text","text":"Use real Codex store only"}]}}"#,
    )
    .expect("codex jsonl");

    let binary = env!("CARGO_BIN_EXE_moonbox");
    let report = output_json(
        moonbox_command(test_name)
            .arg("doctor")
            .arg("--json")
            .env("MOONBOX_CODEX_BIN", binary)
            .env("MOONBOX_CLAUDE_BIN", binary)
            .env("MOONBOX_HERMES_BIN", binary)
            .output()
            .expect("doctor"),
    );

    assert_eq!(report["ready"], true);
    assert_eq!(report["status"], "warn");
    let adapters = report["source_adapters"]
        .as_array()
        .expect("source adapters");
    let codex = adapters
        .iter()
        .find(|adapter| adapter["cli"] == "codex")
        .expect("codex adapter");
    assert_eq!(codex["provenance"], "real");
    assert_eq!(codex["active"], true);
    assert_eq!(codex["session_count"], 1);
    assert_eq!(codex["list_limit"], 50);
    assert_eq!(codex["scan_entry_limit"], 500);
    assert_eq!(codex["summary_line_limit"], 800);
    assert_eq!(codex["scan_truncated"], false);
    assert_eq!(codex["fidelity"]["status"], "fallback");
    assert_eq!(
        codex["fidelity"]["primary_surface"],
        "codex_sqlite_jsonl_read_only"
    );
    assert_eq!(codex["capabilities"]["local_store"]["status"], "available");
    assert_eq!(codex["capabilities"]["fork_resume"]["status"], "available");
    assert_eq!(
        codex["capabilities"]["native_handoff"]["status"],
        "unavailable"
    );

    for tool in ["claude", "hermes"] {
        let adapter = adapters
            .iter()
            .find(|adapter| adapter["cli"] == tool)
            .unwrap_or_else(|| panic!("missing {tool} adapter report"));
        assert_eq!(adapter["provenance"], "missing");
        assert_eq!(adapter["active"], false);
        assert_eq!(adapter["session_count"], 0);
        assert_eq!(adapter["filter_status"], "excluded_missing_store");
        assert_eq!(adapter["fidelity"]["status"], "missing");
        assert_eq!(adapter["fidelity"]["primary_surface"], "none");
        assert_eq!(
            adapter["capabilities"]["local_store"]["status"],
            "unavailable"
        );
        assert_eq!(
            adapter["capabilities"]["native_handoff"]["status"],
            "unavailable"
        );
    }
}

#[test]
fn doctor_and_sessions_report_claude_m63_surface_boundaries() {
    let test_name = "doctor-claude-m63-surfaces";
    let home = fixture_home(test_name);
    let claude_store = home.join("claude").join("projects").join("-repo");
    fs::create_dir_all(&claude_store).expect("claude store");
    fs::write(
        claude_store.join("sdk-child.jsonl"),
        r#"{"type":"system","subtype":"init","session_id":"sdk-child","cwd":"/repo","model":"claude-sonnet-4-20250514","tools":["Read"],"mcp_servers":{"fs":{}}}
{"type":"user","session_id":"sdk-child","timestamp":"2026-06-08T09:00:00.000Z","cwd":"/repo","message":{"content":"Implement M63"}}
{"type":"hook","subtype":"PreToolUse","session_id":"sdk-child","timestamp":"2026-06-08T09:00:01.000Z","hook_event_name":"PreToolUse","message":{"content":"allow Read"}}
{"type":"result","subtype":"success","session_id":"sdk-child","parent_session_id":"parent-session","total_cost_usd":0.0042,"duration_ms":1200,"duration_api_ms":900,"num_turns":3,"result":"done"}"#,
    )
    .expect("claude jsonl");

    let binary = env!("CARGO_BIN_EXE_moonbox");
    let doctor = output_json(
        moonbox_command(test_name)
            .arg("doctor")
            .arg("--json")
            .env("MOONBOX_CODEX_BIN", binary)
            .env("MOONBOX_CLAUDE_BIN", binary)
            .env("MOONBOX_HERMES_BIN", binary)
            .output()
            .expect("doctor"),
    );
    let adapters = doctor["source_adapters"]
        .as_array()
        .expect("source adapters");
    let claude = adapters
        .iter()
        .find(|adapter| adapter["cli"] == "claude")
        .expect("claude adapter");

    assert_eq!(claude["provenance"], "real");
    assert_eq!(claude["active"], true);
    assert_eq!(claude["session_count"], 1);
    assert_eq!(claude["fidelity"]["status"], "partial");
    assert_eq!(
        claude["fidelity"]["primary_surface"],
        "claude_project_jsonl"
    );
    assert_eq!(
        claude["capabilities"]["rich_local_rpc"]["status"],
        "available"
    );
    assert!(
        claude["capabilities"]["rich_local_rpc"]["detail"]
            .as_str()
            .expect("rich local detail")
            .contains("does not invoke Claude")
    );
    assert_eq!(
        claude["capabilities"]["remote_control"]["status"],
        "unavailable"
    );
    assert!(
        claude["capabilities"]["remote_control"]["detail"]
            .as_str()
            .expect("remote detail")
            .contains("not launched")
    );

    let sessions = output_json(
        moonbox_command(test_name)
            .args(["sessions", "--json", "--filter", "claude"])
            .output()
            .expect("sessions"),
    );
    let sessions = sessions.as_array().expect("session array");

    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0]["id"], "sdk-child");
    assert_eq!(sessions[0]["title"], "Implement M63");
    let health = sessions[0]["health_reason"].as_str().expect("health");
    assert!(health.contains("stream-json/SDK metadata parsed"));
    assert!(health.contains("cost_usd=0.004200"));
    assert!(health.contains("turns=3"));
    assert!(health.contains("hook_events=1"));
    assert!(health.contains("forked_from=parent-session"));
}

#[test]
fn doctor_reports_bounded_real_store_scan_cost() {
    let test_name = "doctor-real-store-scan-budget";
    let home = fixture_home(test_name);
    let codex_store = home.join("codex").join("sessions");
    fs::create_dir_all(&codex_store).expect("codex store");
    for id in ["codex-a", "codex-b", "codex-c"] {
        fs::write(
            codex_store.join(format!("{id}.jsonl")),
            format!(
                r#"{{"timestamp":"2026-06-06T10:00:00Z","type":"session_meta","payload":{{"id":"{id}","cwd":"/tmp/moonbox-real"}}}}"#
            ),
        )
        .expect("codex jsonl");
    }

    let binary = env!("CARGO_BIN_EXE_moonbox");
    let report = output_json(
        moonbox_command(test_name)
            .arg("doctor")
            .arg("--json")
            .env("MOONBOX_SESSION_SCAN_LIMIT", "2")
            .env("MOONBOX_CODEX_BIN", binary)
            .env("MOONBOX_CLAUDE_BIN", binary)
            .env("MOONBOX_HERMES_BIN", binary)
            .output()
            .expect("doctor"),
    );

    assert_eq!(report["ready"], true);
    assert_eq!(report["status"], "warn");
    let adapters = report["source_adapters"]
        .as_array()
        .expect("source adapters");
    let codex = adapters
        .iter()
        .find(|adapter| adapter["cli"] == "codex")
        .expect("codex adapter");
    assert_eq!(codex["provenance"], "real");
    assert_eq!(codex["session_count"], 2);
    assert_eq!(codex["list_limit"], 50);
    assert_eq!(codex["scan_entry_limit"], 2);
    assert_eq!(codex["summary_line_limit"], 800);
    assert_eq!(codex["scan_entry_count"], 2);
    assert_eq!(codex["scan_truncated"], true);
    assert_eq!(codex["fidelity"]["status"], "fallback");
    assert_eq!(codex["capabilities"]["local_store"]["status"], "available");

    let checks = report["checks"].as_array().expect("checks");
    let codex_check = checks
        .iter()
        .find(|check| check["name"] == "source_codex_adapter")
        .expect("codex check");
    assert_eq!(codex_check["status"], "warn");
    assert!(
        codex_check["detail"]
            .as_str()
            .expect("detail")
            .contains("scan_truncated=true")
    );
    assert!(
        codex_check["detail"]
            .as_str()
            .expect("detail")
            .contains("fidelity=fallback")
    );
    assert!(
        codex_check["detail"]
            .as_str()
            .expect("detail")
            .contains("capabilities=local_store:available")
    );
}

#[test]
fn fixture_session_mode_ignores_real_shaped_source_homes() {
    let test_name = "fixture-session-mode";
    let home = fixture_home(test_name);
    let codex_store = home.join("codex").join("sessions").join("2026");
    fs::create_dir_all(&codex_store).expect("codex store");
    fs::write(codex_store.join("recent-active.jsonl"), "{not-json\n").expect("codex jsonl");

    let sessions = output_json(
        moonbox_command(test_name)
            .args(["sessions", "--json"])
            .env("MOONBOX_SESSION_MODE", "fixture")
            .output()
            .expect("fixture mode sessions"),
    );
    let ids = sessions
        .as_array()
        .expect("session array")
        .iter()
        .map(|session| session["id"].as_str().expect("session id"))
        .collect::<Vec<_>>();

    assert_eq!(
        ids,
        ["codex-cxcp-design", "claude-qc-platform", "hermes-cxcp-502"]
    );
    assert!(!ids.contains(&"recent-active"));
}

#[test]
fn execute_commands_require_explicit_session_to_avoid_implicit_latest_resume() {
    let open_error = error_text(
        moonbox_command("execute-requires-session")
            .args(["open", "--execute"])
            .output()
            .expect("open execute"),
    );
    assert!(open_error.contains("explicit --session"));
    assert!(open_error.contains("newest active session"));

    let launch_error = error_text(
        moonbox_command("execute-requires-session")
            .args(["launch", "--execute", "--target", "hermes"])
            .output()
            .expect("launch execute"),
    );
    assert!(launch_error.contains("explicit --session"));
    assert!(launch_error.contains("newest active session"));
}

#[test]
fn open_launch_and_verify_public_cli_contracts_are_dry_run_by_default() {
    let open = output_json(
        moonbox_command("dry-run-contract")
            .args(["open", "--session", "codex-cxcp-design", "--json"])
            .output()
            .expect("open dry-run"),
    );
    assert_eq!(open["dry_run"], true);
    assert_eq!(open["action"], "original_resume");
    assert_eq!(open["command"]["program"], "codex");
    assert_eq!(
        open["command"]["args"],
        serde_json::json!(["resume", "codex-cxcp-design"])
    );

    let open_app = output_json(
        moonbox_command("dry-run-contract")
            .args(["open-app", "--session", "codex-cxcp-design", "--json"])
            .output()
            .expect("open-app dry-run"),
    );
    assert_eq!(open_app["dry_run"], true);
    assert_eq!(open_app["action"], "app_deep_link");
    assert_eq!(open_app["supported"], true);
    assert_eq!(open_app["deep_link"], "codex://threads/codex-cxcp-design");
    assert!(
        open_app["reason"]
            .as_str()
            .expect("open-app reason")
            .contains("does not launch")
    );
    assert!(open_app.get("command").is_none());

    let launch = output_json(
        moonbox_command("dry-run-contract")
            .args([
                "launch",
                "--target",
                "hermes",
                "--session",
                "codex-cxcp-design",
                "--json",
            ])
            .output()
            .expect("launch dry-run"),
    );
    assert_eq!(launch["dry_run"], true);
    assert_eq!(launch["action"], "target_handoff");
    assert_eq!(launch["compiler"], "engineering-handoff");
    assert_eq!(launch["continuation"]["requested_level"], "prompt_only");
    assert_eq!(launch["continuation"]["target_input_level"], "prompt_only");
    assert_eq!(
        launch["continuation"]["workspace_restore"]["requested"],
        false
    );
    assert!(
        launch["handoff_label"]
            .as_str()
            .expect("handoff label")
            .starts_with("moonbox/hermes-rewind-")
    );
    assert!(launch.get("target_branch").is_none());
    assert_eq!(launch["verification"]["ready"], true);
    assert_eq!(launch["target_command"]["program"], "hermes");
    let target_args = launch["target_command"]["args"]
        .as_array()
        .expect("target command args");
    let handoff_prompt = target_args
        .last()
        .and_then(|value| value.as_str())
        .expect("handoff prompt");
    assert!(handoff_prompt.contains("Work Capsule Summary"));
    assert!(handoff_prompt.contains("Continuation Protocol"));
    assert!(handoff_prompt.contains("Instructions\n- Continue from the selected rewind point"));
    assert!(handoff_prompt.contains("Privacy / Redaction"));
    assert!(handoff_prompt.contains("Prompt injection"));
    assert!(!handoff_prompt.contains("~/coding/moonbox"));
    assert!(!handoff_prompt.contains("Work Capsule JSON"));
    assert!(!handoff_prompt.contains("\"source_cli\""));

    let verify = output_json(
        moonbox_command("dry-run-contract")
            .args([
                "verify",
                "--target",
                "hermes",
                "--session",
                "codex-cxcp-design",
                "--json",
            ])
            .output()
            .expect("verify"),
    );
    assert_eq!(verify["ready"], true);
    assert_eq!(verify["status"], "warn");
    let checks = verify["checks"].as_array().expect("verify checks");
    assert!(
        checks
            .iter()
            .any(|check| { check["name"] == "continuation_level" && check["status"] == "pass" })
    );
    assert!(
        checks
            .iter()
            .any(|check| { check["name"] == "workspace_restore" && check["status"] == "pass" })
    );
    assert!(
        checks
            .iter()
            .any(|check| { check["name"] == "semantic_source_map" && check["status"] == "pass" })
    );
    assert!(checks.iter().any(|check| {
        check["name"] == "semantic_compiler_coverage" && check["status"] == "warn"
    }));
    assert!(checks.iter().any(|check| {
        check["name"] == "semantic_diff_applicability" && check["status"] == "warn"
    }));
}

#[test]
fn launch_continuation_protocol_blocks_unsupported_import_and_restore_modes() {
    let package = output_json(
        moonbox_command("continuation-package-import")
            .args([
                "launch",
                "--target",
                "hermes",
                "--session",
                "codex-cxcp-design",
                "--continuation",
                "package-import",
                "--json",
            ])
            .output()
            .expect("package import dry-run"),
    );

    assert_eq!(package["dry_run"], true);
    assert_eq!(package["verification"]["ready"], false);
    assert_eq!(package["verification"]["status"], "fail");
    assert_eq!(package["continuation"]["requested_level"], "package_import");
    assert_eq!(package["continuation"]["target_input_level"], "prompt_only");
    assert_eq!(package["continuation"]["package_import"]["requested"], true);
    assert_eq!(
        package["continuation"]["package_import"]["supported"],
        false
    );
    assert!(
        package["verification"]["checks"]
            .as_array()
            .expect("package checks")
            .iter()
            .any(|check| check["name"] == "package_import" && check["status"] == "fail")
    );

    let test_name = "continuation-worktree-restore";
    let home = fixture_home(test_name);
    let workspace = home.join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");
    git(&workspace, &["init"]);
    write_file(&workspace, "README.md", "restore preview\n");
    git(&workspace, &["add", "README.md"]);
    git(
        &workspace,
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

    let codex_home = home.join("codex");
    let rollout_path = codex_home
        .join("sessions")
        .join("2026")
        .join("06")
        .join("06")
        .join("rollout-2026-06-06T10-00-00-restore.jsonl");
    fs::create_dir_all(rollout_path.parent().expect("rollout parent")).expect("codex sessions");
    fs::write(
        &rollout_path,
        format!(
            r#"{{"timestamp":"2026-06-06T10:00:00Z","type":"session_meta","payload":{{"id":"codex-restore-preview","cwd":"{}","git":{{"branch":"main"}}}}}}
{{"timestamp":"2026-06-06T10:01:00Z","type":"response_item","payload":{{"type":"message","role":"user","content":[{{"type":"input_text","text":"Continue with a workspace restore preview"}}]}}}}"#,
            workspace.display()
        ),
    )
    .expect("codex rollout");
    let restore = output_json(
        moonbox_command(test_name)
            .args([
                "launch",
                "--target",
                "hermes",
                "--session",
                "codex-restore-preview",
                "--workspace-restore",
                "worktree",
                "--json",
            ])
            .output()
            .expect("workspace restore dry-run"),
    );

    assert_eq!(restore["verification"]["ready"], false);
    assert_eq!(restore["verification"]["status"], "fail");
    assert_eq!(
        restore["continuation"]["requested_level"],
        "workspace_restore"
    );
    let workspace_restore = &restore["continuation"]["workspace_restore"];
    assert_eq!(workspace_restore["requested"], true);
    assert_eq!(workspace_restore["mode"], "worktree");
    assert_eq!(workspace_restore["supported"], false);
    assert_eq!(workspace_restore["reversible"], true);
    assert_eq!(workspace_restore["preview_only"], true);
    assert!(
        workspace_restore["commands"]
            .as_array()
            .expect("restore commands")
            .iter()
            .any(|command| command.as_str().expect("command").contains("worktree add"))
    );
    assert!(
        workspace_restore["cleanup_commands"]
            .as_array()
            .expect("cleanup commands")
            .iter()
            .any(|command| command
                .as_str()
                .expect("command")
                .contains("worktree remove"))
    );
    assert!(
        restore["verification"]["checks"]
            .as_array()
            .expect("restore checks")
            .iter()
            .any(|check| check["name"] == "workspace_restore" && check["status"] == "fail")
    );
    let prompt = restore["target_command"]["args"]
        .as_array()
        .expect("target args")
        .last()
        .and_then(|value| value.as_str())
        .expect("target prompt");
    assert!(prompt.contains("Continuation Protocol"));
    assert!(!prompt.contains(&workspace.display().to_string()));
    assert!(!prompt.contains("worktree add"));
}

#[test]
fn launch_execute_blocks_real_builtin_draft_without_allow_draft() {
    let test_name = "real-draft-execute-block";
    let home = fixture_home(test_name);
    let codex_home = home.join("codex");
    let rollout_path = codex_home
        .join("sessions")
        .join("2026")
        .join("06")
        .join("06")
        .join("rollout-2026-06-06T10-00-00-real-draft.jsonl");
    fs::create_dir_all(rollout_path.parent().expect("rollout parent")).expect("codex sessions");
    fs::write(
        &rollout_path,
        r#"{"timestamp":"2026-06-06T10:00:00Z","type":"session_meta","payload":{"id":"codex-real-draft","cwd":"/tmp/moonbox-real-draft","git":{"branch":"main"}}}
{"timestamp":"2026-06-06T10:01:00Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"Continue a real session safely"}]}}"#,
    )
    .expect("codex rollout");
    write_codex_thread_index(
        &codex_home,
        &rollout_path,
        "codex-real-draft",
        "Real draft execute block",
    );

    let error = error_text(
        moonbox_command(test_name)
            .args([
                "launch",
                "--execute",
                "--target",
                "hermes",
                "--session",
                "codex-real-draft",
            ])
            .output()
            .expect("launch execute"),
    );

    assert!(error.contains("built-in draft compiler"));
    assert!(error.contains("--allow-draft"));
    assert!(error.contains("non-fixture session"));

    let launches = output_json(
        moonbox_command(test_name)
            .args(["launches", "list", "--json"])
            .output()
            .expect("launches list"),
    );
    let blocked = launches
        .as_array()
        .expect("launches")
        .iter()
        .find(|launch| launch["source_session"] == "codex-real-draft")
        .expect("blocked launch record");
    assert_eq!(blocked["status"], "blocked");
    assert_eq!(blocked["action"], "target_handoff");
    assert_eq!(blocked["target_cli"], "hermes");
    assert!(
        blocked["error_reason"]
            .as_str()
            .expect("error reason")
            .contains("built-in draft compiler")
    );
    assert!(
        !blocked["command"]
            .as_str()
            .expect("ledger command")
            .contains("Work Capsule")
    );
}

#[test]
fn capsule_and_compile_surfaces_accept_explicit_session_target_rewind_and_compiler() {
    let args = [
        "--session",
        "claude-qc-platform",
        "--target",
        "codex",
        "--rewind",
        "evt-074",
        "--compiler",
        "engineering-handoff",
        "--json",
    ];

    let capsule = output_json(
        moonbox_command("compile-surface-contract")
            .arg("capsule")
            .args(args)
            .output()
            .expect("capsule"),
    );
    assert_eq!(capsule["source_cli"], "claude");
    assert_eq!(capsule["target_cli"], "codex");
    assert_eq!(capsule["source_session"], "claude-qc-platform");
    assert!(
        capsule["rewind_point"]
            .as_str()
            .expect("rewind")
            .contains("evt-074")
    );
    assert_eq!(capsule["compiler"], "engineering-handoff");
    assert_eq!(capsule["raw_source_map"]["rewind_event_id"], "evt-074");
    assert_eq!(capsule["redaction"]["enabled"], true);
    assert_eq!(capsule["redaction"]["policy"], "standard");
    assert!(capsule["raw_refs"].as_array().expect("raw refs").len() >= 1);
    assert!(
        capsule["raw_refs"]
            .as_array()
            .expect("raw refs")
            .iter()
            .any(|raw_ref| raw_ref["source_event_id"] == "evt-074")
    );
    let rewind_ref = capsule["raw_refs"]
        .as_array()
        .expect("raw refs")
        .iter()
        .find(|raw_ref| raw_ref["source_event_id"] == "evt-074")
        .expect("rewind raw ref");
    assert_eq!(
        rewind_ref["message_ids"],
        serde_json::json!(["msg-claude-074"])
    );
    assert_eq!(
        rewind_ref["provider_item_ids"],
        serde_json::json!(["item-claude-074"])
    );
    assert_eq!(
        capsule["coverage"]["raw_ref_count"],
        serde_json::json!(capsule["raw_refs"].as_array().expect("raw refs").len())
    );

    let request = output_json(
        moonbox_command("compile-surface-contract")
            .arg("compile-request")
            .args(args)
            .output()
            .expect("compile request"),
    );
    assert_eq!(request["source_cli"], "claude");
    assert_eq!(request["target_cli"], "codex");
    assert_eq!(request["source_session"]["id"], "claude-qc-platform");
    assert_eq!(request["source_session"]["cwd"], "<path:redacted>");
    assert_eq!(request["rewind_event_id"], "evt-074");
    assert_eq!(request["compiler"], "engineering-handoff");
    let rewind_event = request["timeline"]["events"]
        .as_array()
        .expect("timeline events")
        .iter()
        .find(|event| event["id"] == "evt-074")
        .expect("rewind event");
    assert_eq!(
        rewind_event["metadata"]["message_ids"],
        serde_json::json!(["msg-claude-074"])
    );
    assert_eq!(
        rewind_event["metadata"]["provider_item_ids"],
        serde_json::json!(["item-claude-074"])
    );
    assert_eq!(
        rewind_event["metadata"]["raw_refs"][0]["provider_kind"],
        "rewind_point"
    );
    assert_eq!(request["redaction"]["enabled"], true);
    assert!(
        request["redaction"]["external_compiler_disclosure"]
            .as_str()
            .expect("redaction disclosure")
            .contains("External compilers receive a redacted")
    );

    let output = output_json(
        moonbox_command("compile-surface-contract")
            .arg("compile-output")
            .args(args)
            .output()
            .expect("compile output"),
    );
    assert_eq!(output["capsule"]["source_cli"], "claude");
    assert_eq!(output["capsule"]["target_cli"], "codex");
    assert_eq!(output["capsule"]["source_session"], "claude-qc-platform");
    assert!(
        output["capsule"]["rewind_point"]
            .as_str()
            .expect("rewind")
            .contains("evt-074")
    );
}

#[test]
fn capsule_store_cli_roundtrips_saved_capsules_without_launching_sessions() {
    let test_name = "capsule-store-contract";
    let home = fixture_home(test_name);
    let export_path = home.join("exports").join("demo.moonbox-capsule.json");
    let export_arg = export_path.to_str().expect("export path");

    let saved = output_json(
        moonbox_command(test_name)
            .args([
                "capsule",
                "save",
                "m73-demo",
                "--session",
                "codex-cxcp-design",
                "--target",
                "hermes",
                "--rewind",
                "evt-091",
                "--json",
            ])
            .output()
            .expect("capsule save"),
    );
    assert_eq!(saved["name"], "m73-demo");
    assert_eq!(saved["source_session"], "codex-cxcp-design");
    assert_eq!(saved["target_cli"], "hermes");
    assert_eq!(saved["capsule"]["source_cli"], "codex");
    let checksum = saved["checksum"].as_str().expect("checksum");
    assert!(checksum.starts_with("fnv64:"));

    let list = output_json(
        moonbox_command(test_name)
            .args(["capsule", "list", "--json"])
            .output()
            .expect("capsule list"),
    );
    assert!(
        list.as_array()
            .expect("capsule list")
            .iter()
            .any(|capsule| capsule["name"] == "m73-demo" && capsule["checksum"] == checksum)
    );

    let shown = output_json(
        moonbox_command(test_name)
            .args(["capsule", "show", "m73-demo", "--json"])
            .output()
            .expect("capsule show"),
    );
    assert_eq!(shown["checksum"], checksum);
    assert_eq!(
        shown["capsule"]["raw_source_map"]["rewind_event_id"],
        "evt-091"
    );

    let launch = output_json(
        moonbox_command(test_name)
            .args([
                "capsule", "launch", "m73-demo", "--target", "hermes", "--json",
            ])
            .output()
            .expect("capsule launch"),
    );
    assert_eq!(launch["dry_run"], true);
    assert_eq!(launch["capsule_path"], "store:m73-demo");
    assert_eq!(launch["target_command"]["program"], "hermes");
    assert_eq!(launch["verification"]["ready"], true);

    let exported = output_json(
        moonbox_command(test_name)
            .args([
                "capsule", "export", "m73-demo", "--output", export_arg, "--json",
            ])
            .output()
            .expect("capsule export"),
    );
    assert_eq!(exported["kind"], "moonbox.capsule.export");
    assert_eq!(exported["exported_by"], "moonbox");
    assert_eq!(exported["checksum"], checksum);
    assert!(export_path.exists());

    let deleted = output_json(
        moonbox_command(test_name)
            .args(["capsule", "delete", "m73-demo", "--json"])
            .output()
            .expect("capsule delete"),
    );
    assert_eq!(deleted["deleted"], true);

    let imported = output_json(
        moonbox_command(test_name)
            .args([
                "capsule",
                "import",
                export_arg,
                "--name",
                "m73-imported",
                "--json",
            ])
            .output()
            .expect("capsule import"),
    );
    assert_eq!(imported["imported"], true);
    assert_eq!(imported["name"], "m73-imported");
    assert_eq!(imported["verification"]["ready"], true);
    assert!(
        imported["verification"]["checks"]
            .as_array()
            .expect("import checks")
            .iter()
            .any(|check| check["name"] == "checksum" && check["status"] == "pass")
    );

    let imported_show = output_json(
        moonbox_command(test_name)
            .args(["capsule", "show", "m73-imported", "--json"])
            .output()
            .expect("imported show"),
    );
    assert_eq!(imported_show["checksum"], checksum);
    assert_eq!(
        imported_show["capsule"]["handoff_label"],
        saved["capsule"]["handoff_label"]
    );
}

#[test]
fn launch_ledger_cli_records_executes_and_links_capsules() {
    let test_name = "launch-ledger-contract";
    let home = fixture_home(test_name);
    let fake_bin = write_executable_script(&home, "bin/fake-target", "#!/bin/sh\nexit 0\n");

    let saved = output_json(
        moonbox_command(test_name)
            .args([
                "capsule",
                "save",
                "m74-demo",
                "--session",
                "codex-cxcp-design",
                "--target",
                "hermes",
                "--rewind",
                "evt-091",
                "--json",
            ])
            .output()
            .expect("capsule save"),
    );
    assert_eq!(saved["name"], "m74-demo");

    let capsule_execution = output_json(
        moonbox_command(test_name)
            .args([
                "capsule",
                "launch",
                "m74-demo",
                "--target",
                "hermes",
                "--execute",
                "--json",
            ])
            .env("MOONBOX_HERMES_BIN", &fake_bin)
            .output()
            .expect("capsule launch execute"),
    );
    assert_eq!(capsule_execution["status"], "success");
    assert_eq!(capsule_execution["exit_code"], 0);
    let capsule_launch_id = capsule_execution["launch_ledger"]["id"]
        .as_i64()
        .expect("capsule launch id");

    let shown = output_json(
        moonbox_command(test_name)
            .args(["launches", "show", &capsule_launch_id.to_string(), "--json"])
            .output()
            .expect("launches show"),
    );
    assert_eq!(shown["id"], capsule_launch_id);
    assert_eq!(shown["status"], "success");
    assert_eq!(shown["action"], "target_handoff");
    assert_eq!(shown["capsule_name"], "m74-demo");
    assert_eq!(shown["capsule_ref"], "store:m74-demo");
    assert_eq!(shown["source_session"], "codex-cxcp-design");
    assert!(
        shown["rewind_point"]
            .as_str()
            .expect("rewind")
            .starts_with("evt-091")
    );
    assert_eq!(shown["target_cli"], "hermes");
    assert_eq!(shown["dry_run"], false);
    assert!(
        shown["command"]
            .as_str()
            .expect("safe command")
            .contains("<handoff-prompt>")
    );
    assert!(
        !shown["command"]
            .as_str()
            .expect("safe command")
            .contains("Work Capsule")
    );

    let capsule_launches = output_json(
        moonbox_command(test_name)
            .args(["capsule", "launches", "m74-demo", "--json"])
            .output()
            .expect("capsule launches"),
    );
    assert!(
        capsule_launches
            .as_array()
            .expect("capsule launches")
            .iter()
            .any(|launch| launch["id"] == capsule_launch_id)
    );

    let open_execution = output_json(
        moonbox_command(test_name)
            .args([
                "open",
                "--execute",
                "--session",
                "codex-cxcp-design",
                "--json",
            ])
            .env("MOONBOX_CODEX_BIN", &fake_bin)
            .output()
            .expect("open execute"),
    );
    assert_eq!(open_execution["status"], "success");
    let open_launch_id = open_execution["launch_ledger"]["id"]
        .as_i64()
        .expect("open launch id");

    let linked = output_json(
        moonbox_command(test_name)
            .args([
                "launches",
                "link",
                &open_launch_id.to_string(),
                "--capsule",
                "m74-demo",
                "--json",
            ])
            .output()
            .expect("launches link"),
    );
    assert_eq!(linked["id"], open_launch_id);
    assert_eq!(linked["action"], "original_resume");
    assert_eq!(linked["capsule_name"], "m74-demo");
    assert_eq!(linked["capsule_ref"], "store:m74-demo");
    assert_eq!(
        linked["command"],
        format!("{} resume codex-cxcp-design", fake_bin.display())
    );

    let launches = output_json(
        moonbox_command(test_name)
            .args(["launches", "list", "--json"])
            .output()
            .expect("launches list"),
    );
    let ids = launches
        .as_array()
        .expect("launches list")
        .iter()
        .map(|launch| launch["id"].as_i64().expect("launch id"))
        .collect::<Vec<_>>();
    assert!(ids.contains(&capsule_launch_id));
    assert!(ids.contains(&open_launch_id));

    let linked_capsule_launches = output_json(
        moonbox_command(test_name)
            .args(["capsule", "launches", "m74-demo", "--json"])
            .output()
            .expect("linked capsule launches"),
    );
    let linked_ids = linked_capsule_launches
        .as_array()
        .expect("linked capsule launches")
        .iter()
        .map(|launch| launch["id"].as_i64().expect("launch id"))
        .collect::<Vec<_>>();
    assert!(linked_ids.contains(&capsule_launch_id));
    assert!(linked_ids.contains(&open_launch_id));
}
