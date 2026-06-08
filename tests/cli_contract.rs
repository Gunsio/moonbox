use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
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
        "MOONBOX_CODEX_BIN",
        "MOONBOX_CLAUDE_BIN",
        "MOONBOX_HERMES_BIN",
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
    assert!(bash.contains("ssh"));

    let fish = output_text(
        moon_command("completion-moon-fish")
            .args(["completions", "fish"])
            .output()
            .expect("moon fish completions"),
    );
    assert!(fish.contains("complete -c moon"));
    assert!(fish.contains("replay-eval"));
    assert!(fish.contains("completions"));
    assert!(fish.contains("ssh"));

    let zsh = output_text(
        moonbox_command("completion-explicit-moon-zsh")
            .args(["completions", "--bin", "moon", "zsh"])
            .output()
            .expect("explicit moon zsh completions"),
    );
    assert!(zsh.contains("#compdef moon"));
    assert!(zsh.contains("replay-eval"));
    assert!(zsh.contains("completions"));
    assert!(zsh.contains("ssh"));
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
    assert!(svg.contains("Capsule Review"));
    assert!(svg.contains("Target receives"));
    assert!(svg.contains("Draft Work Capsule"));
    assert!(svg.contains("moonbox launch --execute"));
    assert!(svg.contains("Handoff"));

    let main_svg = output_text(
        moonbox_command("docs-snapshot-main")
            .arg("docs-snapshot")
            .args(["--variant", "main"])
            .output()
            .expect("main docs snapshot"),
    );
    assert!(main_svg.starts_with("<svg "));
    assert!(main_svg.contains("Moonbox main workbench screenshot"));
    assert!(main_svg.contains("Sessions"));
    assert!(main_svg.contains("Timeline"));
    assert!(main_svg.contains("Real Session Metadata"));
    assert!(!main_svg.contains("Handoff Review"));

    let timeline_svg = output_text(
        moonbox_command("docs-snapshot-timeline")
            .arg("docs-snapshot")
            .args(["--variant", "timeline"])
            .output()
            .expect("timeline docs snapshot"),
    );
    assert!(timeline_svg.starts_with("<svg "));
    assert!(timeline_svg.contains("Moonbox timeline zoom screenshot"));
    assert!(timeline_svg.contains("Timeline"));
    assert!(timeline_svg.contains("Zoomed Timeline"));
    assert!(timeline_svg.contains("REWIND"));
    assert!(!timeline_svg.contains("Session Details"));
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

    for tool in ["claude", "hermes"] {
        let adapter = adapters
            .iter()
            .find(|adapter| adapter["cli"] == tool)
            .unwrap_or_else(|| panic!("missing {tool} adapter report"));
        assert_eq!(adapter["provenance"], "missing");
        assert_eq!(adapter["active"], false);
        assert_eq!(adapter["session_count"], 0);
        assert_eq!(adapter["filter_status"], "excluded_missing_store");
    }
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
    assert!(handoff_prompt.contains("Instructions\n- Continue from the selected rewind point"));
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
    assert_eq!(verify["status"], "pass");
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
    assert_eq!(request["rewind_event_id"], "evt-074");
    assert_eq!(request["compiler"], "engineering-handoff");

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
