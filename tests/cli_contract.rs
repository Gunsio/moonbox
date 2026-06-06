use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

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

    let fish = output_text(
        moon_command("completion-moon-fish")
            .args(["completions", "fish"])
            .output()
            .expect("moon fish completions"),
    );
    assert!(fish.contains("complete -c moon"));
    assert!(fish.contains("replay-eval"));
    assert!(fish.contains("completions"));

    let zsh = output_text(
        moonbox_command("completion-explicit-moon-zsh")
            .args(["completions", "--bin", "moon", "zsh"])
            .output()
            .expect("explicit moon zsh completions"),
    );
    assert!(zsh.contains("#compdef moon"));
    assert!(zsh.contains("replay-eval"));
    assert!(zsh.contains("completions"));
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
    assert!(svg.contains("Launch Review"));
    assert!(svg.contains("Readiness details"));
    assert!(svg.contains("moonbox launch --execute"));
    assert!(svg.contains("copy execute command"));
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
