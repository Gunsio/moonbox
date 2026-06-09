#![cfg_attr(
    not(test),
    deny(
        unsafe_code,
        clippy::expect_used,
        clippy::panic,
        clippy::todo,
        clippy::unimplemented,
        clippy::unwrap_used
    )
)]
#![warn(missing_docs)]

//! Moonbox command-line entrypoint.
//!
//! Moonbox is a CLI-first project. The stable public surface is the installed
//! `moonbox` and `moon` commands; Rust internals remain crate-private until an
//! API is intentionally stabilized.

mod app;
mod cli;
pub(crate) mod core;
mod tui;

use clap::{CommandFactory, Parser};
use cli::{Cli, Command};
use color_eyre::Result;
use std::{
    collections::HashSet,
    fs,
    io::{self, Write},
    path::Path,
};

/// Run the Moonbox command-line application.
pub fn run() -> Result<()> {
    color_eyre::install()?;

    let cli = Cli::parse();
    match cli.command.unwrap_or_default() {
        Command::Tui(args) => run_tui(args),
        Command::Sessions(args) => print_sessions(args),
        Command::Open(args) => print_open_command(args),
        Command::OpenApp(args) => print_open_app_plan(args),
        Command::Capsule(args) => print_capsule(args),
        Command::CompileRequest(args) => print_compile_request(args),
        Command::CompileOutput(args) => print_compile_output(args),
        Command::Compilers(args) => print_compilers(args),
        Command::Ssh(args) => print_ssh_hosts(args),
        Command::Doctor(args) => print_doctor(args),
        Command::Snapshot(args) => print_workspace_snapshot(args),
        Command::Completions(args) => print_completions(args),
        Command::Launch(args) => print_launch_plan(args),
        Command::Verify(args) => print_verify_report(args),
        Command::ReplayEval(args) => print_replay_eval(args),
        Command::DocsSnapshot(args) => print_docs_snapshot(args),
    }
}

fn run_tui(args: cli::TuiArgs) -> Result<()> {
    let target = args
        .target
        .or_else(core::config::load_last_target)
        .unwrap_or(core::model::CliTool::Hermes);
    let filter = args.filter.or(args.source);
    let source = filter.unwrap_or(core::model::CliTool::Codex);
    let mut terminal = ratatui::init();
    let result = tui::run_with_loading(&mut terminal, source, target, filter);
    ratatui::restore();
    execute_tui_exit_action(result?)
}

fn execute_tui_exit_action(action: Option<app::TuiExitAction>) -> Result<()> {
    match action {
        Some(app::TuiExitAction::OriginalResume(plan)) => {
            print!("{}", core::launcher::original_handoff_notice(&plan));
            io::stdout().flush()?;
            core::launcher::handoff_original_plan(*plan)?;
        }
        Some(app::TuiExitAction::TargetHandoff(plan)) => {
            core::launcher::execute_plan(*plan, false)?;
        }
        None => {}
    }
    Ok(())
}

fn print_sessions(args: cli::SessionListArgs) -> Result<()> {
    let filter = args.filter.or(args.source);
    let hermes_sources = normalized_hermes_sources(&args.hermes_sources);
    let sessions = core::workbench::list_sessions()?
        .into_iter()
        .filter(|session| filter.is_none_or(|filter| session.cli == filter))
        .filter(|session| {
            hermes_sources.is_empty() || hermes_source_matches(session, &hermes_sources)
        })
        .collect::<Vec<_>>();
    if args.json {
        println!("{}", serde_json::to_string_pretty(&sessions)?);
    } else {
        println!(
            "filter: {}",
            filter
                .map(|tool| tool.id().to_owned())
                .unwrap_or_else(|| "all".into())
        );
        if !hermes_sources.is_empty() {
            let mut sources = hermes_sources.iter().cloned().collect::<Vec<_>>();
            sources.sort();
            println!("hermes_source: {}", sources.join(","));
        }
        for session in sessions {
            println!(
                "{:<8} {:<7} {:<28} {:<24} {}{}",
                session.cli,
                session.source_provenance,
                session.title,
                session.cwd,
                session.updated,
                parse_skip_suffix(session.parse_skip_count)
            );
        }
    }
    Ok(())
}

fn normalized_hermes_sources(values: &[String]) -> HashSet<String> {
    values
        .iter()
        .filter_map(|value| normalized_hermes_source(value))
        .collect()
}

fn hermes_source_matches(
    session: &core::model::SessionSummary,
    hermes_sources: &HashSet<String>,
) -> bool {
    if session.cli != core::model::CliTool::Hermes {
        return false;
    }
    session
        .provider_metadata
        .as_ref()
        .and_then(|metadata| metadata.source.as_deref())
        .and_then(normalized_hermes_source)
        .is_some_and(|source| hermes_sources.contains(&source))
}

fn normalized_hermes_source(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let normalized = value.to_ascii_lowercase().replace(['-', ' ', '/'], "_");
    Some(match normalized.as_str() {
        "api" | "api_server" | "apiserver" => "api_server".into(),
        "cli" | "discord" | "telegram" | "slack" | "cron" => normalized,
        _ => normalized,
    })
}

fn parse_skip_suffix(parse_skip_count: usize) -> String {
    if parse_skip_count == 0 {
        String::new()
    } else {
        format!("  skipped={parse_skip_count}")
    }
}

fn print_open_command(args: cli::OpenArgs) -> Result<()> {
    if args.execute {
        let execution = core::workbench::execute_open(args.session.as_deref())?;
        if let Some(execution) = execution {
            if args.json {
                println!("{}", serde_json::to_string_pretty(&execution)?);
            } else {
                println!("open: execute");
                println!("status: {}", launch_status(execution.status));
                println!(
                    "exit: {}",
                    execution
                        .exit_code
                        .map(|code| code.to_string())
                        .unwrap_or_else(|| "signal".into())
                );
                println!("command: {}", execution.plan.command.display);
            }
        } else {
            println!("No session selected");
        }
        return Ok(());
    }

    if args.json {
        let plan = core::workbench::open_plan(args.session.as_deref())?;
        if let Some(plan) = plan {
            println!("{}", serde_json::to_string_pretty(&plan)?);
        } else {
            println!("No session selected");
        }
        return Ok(());
    }

    if let Some(command) = core::workbench::open_command(args.session.as_deref())? {
        println!("{command}");
    } else {
        println!("No session selected");
    }
    Ok(())
}

fn print_open_app_plan(args: cli::OpenAppArgs) -> Result<()> {
    let plan = core::workbench::open_app_plan(args.session.as_deref())?;
    if let Some(plan) = plan {
        if args.json {
            println!("{}", serde_json::to_string_pretty(&plan)?);
        } else {
            println!("open-app: dry-run");
            println!("action: app-deep-link");
            println!("session: {}", plan.source_session.id);
            println!("supported: {}", plan.supported);
            if let Some(deep_link) = plan.deep_link {
                println!("deep_link: {deep_link}");
            }
            println!("reason: {}", plan.reason);
        }
    } else {
        println!("No session selected");
    }
    Ok(())
}

fn print_capsule(args: cli::CompileArgs) -> Result<()> {
    let target = launch_target(args.target);
    let capsule = core::workbench::capsule_for_selection(
        args.session.as_deref(),
        target,
        args.rewind.as_deref(),
        args.compiler.as_deref(),
    )?;
    if let Some(capsule) = capsule {
        if args.json {
            println!("{}", serde_json::to_string_pretty(&capsule)?);
        } else {
            println!("source: {}", capsule.source_cli);
            println!("session: {}", capsule.source_session);
            println!("target_cli: {}", capsule.target_cli);
            println!("compiler: {}", capsule.compiler);
            println!("goal: {}", capsule.goal);
            println!("state: {}", capsule.state);
            println!("rewind: {}", capsule.rewind_point);
            println!("handoff_label: {}", capsule.handoff_label);
            print_redaction_report(&capsule.redaction);
        }
    } else {
        println!("No session selected");
    }
    Ok(())
}

fn print_compile_request(args: cli::CompileArgs) -> Result<()> {
    let target = launch_target(args.target);
    let request = core::workbench::compile_request_for_selection(
        args.session.as_deref(),
        target,
        args.rewind.as_deref(),
        args.compiler.as_deref(),
    )?;
    if let Some(request) = request {
        if args.json {
            println!("{}", serde_json::to_string_pretty(&request)?);
        } else {
            println!("source: {}", request.source_cli);
            println!("target: {}", request.target_cli);
            println!("session: {}", request.source_session.id);
            println!("rewind: {}", request.rewind_event_id);
            println!("compiler: {}", request.compiler);
            print_redaction_report(&request.redaction);
        }
    } else {
        println!("No session selected");
    }
    Ok(())
}

fn print_compile_output(args: cli::CompileArgs) -> Result<()> {
    let target = launch_target(args.target);
    let output = core::workbench::compile_output_for_selection(
        args.session.as_deref(),
        target,
        args.rewind.as_deref(),
        args.compiler.as_deref(),
    )?;
    if let Some(output) = output {
        if args.json {
            println!("{}", serde_json::to_string_pretty(&output)?);
        } else {
            println!("version: {}", output.version);
            println!("source: {}", output.capsule.source_cli);
            println!("session: {}", output.capsule.source_session);
            println!("target_cli: {}", output.capsule.target_cli);
            println!("compiler: {}", output.capsule.compiler);
            println!("goal: {}", output.capsule.goal);
            println!("handoff_label: {}", output.capsule.handoff_label);
            print_redaction_report(&output.capsule.redaction);
        }
    } else {
        println!("No session selected");
    }
    Ok(())
}

fn print_compilers(args: cli::JsonArgs) -> Result<()> {
    let compilers = core::compiler::compiler_catalog_entries();
    if args.json {
        println!("{}", serde_json::to_string_pretty(&compilers)?);
    } else {
        for compiler in compilers {
            let kind = format!("{:?}", compiler.kind);
            let status = format!("{:?}", compiler.status);
            println!(
                "{:<22} {:<11} {:<8} score={:<3} {}",
                compiler.id, kind, status, compiler.score, compiler.reason
            );
        }
    }
    Ok(())
}

fn print_ssh_hosts(args: cli::JsonArgs) -> Result<()> {
    let hosts = core::ssh::list_ssh_hosts()?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&hosts)?);
    } else if hosts.is_empty() {
        println!("No SSH hosts configured");
        println!("Add Host entries to ~/.ssh/config or ssh_hosts to ~/.config/moonbox/config.json");
    } else {
        println!("SSH hosts: {}", hosts.len());
        for host in hosts {
            println!("{}", host.name);
            println!(
                "  target {}  source {}",
                ssh_target_display(&host),
                ssh_source_label(host.source)
            );
            if let Some(identity_file) = host.identity_file {
                println!("  identity {identity_file}");
            }
        }
    }
    Ok(())
}

fn ssh_target_display(host: &core::ssh::SshHostEntry) -> String {
    let target = host
        .user
        .as_ref()
        .map(|user| format!("{user}@{}", host.host))
        .unwrap_or_else(|| host.host.clone());
    host.port
        .map(|port| format!("{target}:{port}"))
        .unwrap_or(target)
}

fn ssh_source_label(source: core::ssh::SshHostSource) -> &'static str {
    match source {
        core::ssh::SshHostSource::MoonboxConfig => "moonbox",
        core::ssh::SshHostSource::OpensshConfig => "openssh",
    }
}

fn print_doctor(args: cli::JsonArgs) -> Result<()> {
    let report = core::doctor::diagnose();
    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("doctor: {}", report.status);
        println!("ready: {}", report.ready);
        print_checks(&report.checks);
    }
    Ok(())
}

fn print_workspace_snapshot(args: cli::SnapshotArgs) -> Result<()> {
    let snapshot =
        core::snapshot::capture_workspace_snapshot(&core::snapshot::WorkspaceSnapshotOptions {
            path: args.path,
            diff_line_limit: args.diff_lines,
            test_commands: args.test_commands,
        })?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&snapshot)?);
    } else {
        print_snapshot_text(&snapshot);
    }
    Ok(())
}

fn print_snapshot_text(snapshot: &core::snapshot::WorkspaceSnapshot) {
    println!("workspace snapshot: v{}", snapshot.version);
    println!("cwd: {}", snapshot.cwd);
    println!("repo: {}", snapshot.repo_root.as_deref().unwrap_or("-"));
    println!(
        "git: {}",
        if snapshot.git.available {
            "available"
        } else {
            "unavailable"
        }
    );
    if let Some(reason) = &snapshot.git.reason {
        println!("reason: {reason}");
    }
    println!(
        "head: {}",
        snapshot.git.head.as_deref().map(short_sha).unwrap_or("-")
    );
    println!("branch: {}", snapshot.git.branch.as_deref().unwrap_or("-"));
    println!(
        "upstream: {}",
        snapshot.git.upstream.as_deref().unwrap_or("-")
    );
    println!("dirty: {}", snapshot.git.dirty);
    print_path_group("staged", &snapshot.git.staged);
    print_path_group("unstaged", &snapshot.git.unstaged);
    print_path_group("untracked", &snapshot.git.untracked);
    if let Some(stat) = &snapshot.git.staged_diff_stat {
        println!("staged diff stat:\n{stat}");
    }
    if let Some(stat) = &snapshot.git.unstaged_diff_stat {
        println!("unstaged diff stat:\n{stat}");
    }
    if !snapshot.key_files.is_empty() {
        println!("key files:");
        for file in &snapshot.key_files {
            println!("- {} {} bytes", file.path, file.bytes);
        }
    }
    println!(
        "env: {} {} shell={} term={} ci={}",
        snapshot.environment.os,
        snapshot.environment.arch,
        snapshot.environment.shell.as_deref().unwrap_or("-"),
        snapshot.environment.term.as_deref().unwrap_or("-"),
        snapshot.environment.ci
    );
    if !snapshot.test_commands.is_empty() {
        println!("test commands:");
        for command in &snapshot.test_commands {
            println!(
                "- {} status={} exit={}",
                command.command,
                if command.success { "pass" } else { "fail" },
                command
                    .exit_code
                    .map(|code| code.to_string())
                    .unwrap_or_else(|| "signal".into())
            );
        }
    }
}

fn print_path_group(label: &str, paths: &[String]) {
    println!("{label}: {}", paths.len());
    for path in paths.iter().take(20) {
        println!("- {path}");
    }
    if paths.len() > 20 {
        println!("- ... {} more", paths.len() - 20);
    }
}

fn short_sha(value: &str) -> &str {
    value.get(..12).unwrap_or(value)
}

fn print_completions(args: cli::CompletionsArgs) -> Result<()> {
    let bin_name = completion_bin_name(args.binary);
    let mut command = Cli::command();
    command.set_bin_name(bin_name.clone());
    clap_complete::generate(args.shell, &mut command, bin_name, &mut io::stdout().lock());
    Ok(())
}

fn completion_bin_name(binary: Option<cli::CompletionBinary>) -> String {
    if let Some(binary) = binary {
        return binary.as_str().to_owned();
    }

    std::env::args()
        .next()
        .and_then(|arg| {
            Path::new(&arg)
                .file_name()
                .and_then(|name| name.to_str())
                .map(str::to_owned)
        })
        .filter(|name| name == "moon")
        .unwrap_or_else(|| "moonbox".to_owned())
}

fn print_launch_plan(args: cli::LaunchArgs) -> Result<()> {
    let target = launch_target(args.target);
    let continuation_options = continuation_options(&args);
    if args.execute {
        let execution = core::workbench::execute_launch_with_options(
            args.session.as_deref(),
            target,
            args.capsule.as_deref(),
            args.allow_draft,
            continuation_options,
        )?;
        if let Some(execution) = execution {
            if args.json {
                println!("{}", serde_json::to_string_pretty(&execution)?);
            } else {
                println!("launch: execute");
                println!("status: {}", launch_status(execution.status));
                println!(
                    "exit: {}",
                    execution
                        .exit_code
                        .map(|code| code.to_string())
                        .unwrap_or_else(|| "signal".into())
                );
                println!("command: {}", execution.plan.command);
            }
        } else {
            println!("No session selected");
        }
        return Ok(());
    }

    let plan = core::workbench::launch_plan_with_options(
        args.session.as_deref(),
        target,
        args.capsule.as_deref(),
        continuation_options,
    )?;
    if let Some(plan) = plan {
        if args.json {
            println!("{}", serde_json::to_string_pretty(&plan)?);
        } else {
            println!("launch: dry-run");
            println!("action: target-handoff");
            println!("session: {}", plan.source_session.id);
            println!("target: {}", plan.target_cli);
            println!("handoff_label: {}", plan.handoff_label);
            println!(
                "capsule: {}",
                plan.capsule_path.as_deref().unwrap_or("generated")
            );
            println!("preflight_ready: {}", plan.verification.ready);
            println!("scope: structural and semantic preflight; user review is still required");
            println!("status: {}", plan.verification.status);
            print_continuation_protocol(&plan.continuation);
            println!("command: {}", plan.command);
            println!("program: {}", plan.target_command.program);
            print_checks(&plan.verification.checks);
        }
    } else {
        println!("No session selected");
    }
    Ok(())
}

fn print_verify_report(args: cli::LaunchArgs) -> Result<()> {
    let target = launch_target(args.target);
    let report = core::workbench::verify_launch_with_options(
        args.session.as_deref(),
        target,
        args.capsule.as_deref(),
        continuation_options(&args),
    )?;
    if let Some(report) = report {
        if args.json {
            println!("{}", serde_json::to_string_pretty(&report)?);
        } else {
            println!("preflight_ready: {}", report.ready);
            println!("scope: structural and semantic preflight; user review is still required");
            println!("status: {}", report.status);
            print_checks(&report.checks);
        }
    } else {
        println!("No session selected");
    }
    Ok(())
}

fn continuation_options(args: &cli::LaunchArgs) -> core::model::ContinuationOptions {
    core::model::ContinuationOptions::new(args.continuation, args.workspace_restore)
}

fn print_continuation_protocol(protocol: &core::model::ContinuationProtocol) {
    println!("continuation: {}", protocol.requested_level);
    println!("target_input_level: {}", protocol.target_input_level);
    println!(
        "package_import: {}",
        if protocol.package_import.requested {
            protocol.package_import.reason.as_str()
        } else {
            "not requested"
        }
    );
    println!(
        "workspace_restore: {} {}",
        protocol.workspace_restore.mode, protocol.workspace_restore.reason
    );
    if !protocol.workspace_restore.commands.is_empty() {
        println!("workspace_restore_preview:");
        for command in &protocol.workspace_restore.commands {
            println!("- {command}");
        }
    }
    if !protocol.workspace_restore.cleanup_commands.is_empty() {
        println!("workspace_restore_cleanup:");
        for command in &protocol.workspace_restore.cleanup_commands {
            println!("- {command}");
        }
    }
}

fn print_replay_eval(args: cli::JsonArgs) -> Result<()> {
    let report = core::replay_eval::evaluate_fixture_replay()?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("replay-eval: fixture-only");
        println!("compiler: {}", report.compiler);
        println!(
            "cases: {} source(s) x {} target(s) = {}",
            report.source_count, report.target_count, report.case_count
        );
        println!(
            "pipeline: {}",
            replay_pipeline_status(report.pipeline_passed)
        );
        println!(
            "status: PASS {} / WARN {} / FAIL {}",
            report.status_counts.pass, report.status_counts.warn, report.status_counts.fail
        );
        for case in report.cases {
            println!(
                "- {} -> {} {} session={} rewind={} checks={}",
                case.source_cli,
                case.target_cli,
                case.status,
                case.source_session,
                case.rewind_event_id,
                case.check_count
            );
        }
    }
    Ok(())
}

fn print_redaction_report(report: &core::model::RedactionReport) {
    println!(
        "redaction: {} secrets={} paths={} events_removed={}",
        report.policy, report.secrets_redacted, report.paths_redacted, report.events_removed
    );
}

fn print_docs_snapshot(args: cli::DocsSnapshotArgs) -> Result<()> {
    let svg = tui::docs_screenshot_svg(args.width, args.height)?;
    if let Some(path) = args.output {
        fs::write(path, svg)?;
    } else {
        print!("{svg}");
    }
    Ok(())
}

fn launch_target(target: Option<core::model::CliTool>) -> core::model::CliTool {
    target
        .or_else(core::config::load_last_target)
        .unwrap_or(core::model::CliTool::Hermes)
}

fn launch_status(status: core::model::LaunchExecutionStatus) -> &'static str {
    match status {
        core::model::LaunchExecutionStatus::Success => "success",
        core::model::LaunchExecutionStatus::Failed => "failed",
    }
}

fn replay_pipeline_status(passed: bool) -> &'static str {
    if passed { "passed" } else { "failed" }
}

fn print_checks(checks: &[core::model::VerificationCheck]) {
    for check in checks {
        println!("- {} {}: {}", check.status, check.name, check.detail);
    }
}
