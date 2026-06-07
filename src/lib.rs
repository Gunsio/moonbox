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
        Command::Capsule(args) => print_capsule(args),
        Command::CompileRequest(args) => print_compile_request(args),
        Command::CompileOutput(args) => print_compile_output(args),
        Command::Compilers(args) => print_compilers(args),
        Command::Doctor(args) => print_doctor(args),
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
            core::launcher::handoff_original_plan(plan)?;
        }
        Some(app::TuiExitAction::TargetHandoff(plan)) => {
            core::launcher::execute_plan(plan)?;
        }
        None => {}
    }
    Ok(())
}

fn print_sessions(args: cli::SessionListArgs) -> Result<()> {
    let filter = args.filter.or(args.source);
    let sessions = core::workbench::list_sessions()?
        .into_iter()
        .filter(|session| filter.is_none_or(|filter| session.cli == filter))
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
            println!("target: {}", capsule.target_branch);
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
            println!("target: {}", output.capsule.target_branch);
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
    if args.execute {
        let execution = core::workbench::execute_launch(
            args.session.as_deref(),
            target,
            args.capsule.as_deref(),
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

    let plan =
        core::workbench::launch_plan(args.session.as_deref(), target, args.capsule.as_deref())?;
    if let Some(plan) = plan {
        if args.json {
            println!("{}", serde_json::to_string_pretty(&plan)?);
        } else {
            println!("launch: dry-run");
            println!("action: target-handoff");
            println!("session: {}", plan.source_session.id);
            println!("target: {}", plan.target_cli);
            println!("branch: {}", plan.target_branch);
            println!(
                "capsule: {}",
                plan.capsule_path.as_deref().unwrap_or("generated")
            );
            println!("ready: {}", plan.verification.ready);
            println!("status: {}", plan.verification.status);
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
    let report =
        core::workbench::verify_launch(args.session.as_deref(), target, args.capsule.as_deref())?;
    if let Some(report) = report {
        if args.json {
            println!("{}", serde_json::to_string_pretty(&report)?);
        } else {
            println!("ready: {}", report.ready);
            println!("status: {}", report.status);
            print_checks(&report.checks);
        }
    } else {
        println!("No session selected");
    }
    Ok(())
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
