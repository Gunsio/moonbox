mod app;
mod cli;
pub mod core;
mod tui;

use clap::{CommandFactory, Parser};
use cli::{Cli, Command};
use color_eyre::Result;
use std::{fs, io, path::Path};

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
    let mut app = app::App::new(source, target)?;
    if let Some(filter) = filter {
        app.apply_session_filter(app::SessionFilter::Tool(filter));
    }
    let mut terminal = ratatui::init();
    let result = tui::run(&mut terminal, app);
    ratatui::restore();
    result
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
        for session in sessions {
            println!(
                "{:<8} {:<28} {:<24} {}",
                session.cli, session.title, session.cwd, session.updated
            );
        }
    }
    Ok(())
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

fn print_capsule(args: cli::JsonArgs) -> Result<()> {
    let capsule =
        core::workbench::capsule(core::model::CliTool::Codex, core::model::CliTool::Hermes)?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&capsule)?);
    } else {
        println!("goal: {}", capsule.goal);
        println!("state: {}", capsule.state);
        println!("rewind: {}", capsule.rewind_point);
        println!("target: {}", capsule.target_branch);
    }
    Ok(())
}

fn print_compile_request(args: cli::CompileArgs) -> Result<()> {
    let request = core::workbench::compile_request(
        core::model::CliTool::Codex,
        core::model::CliTool::Hermes,
        "evt-091",
        args.compiler.as_deref(),
    )?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&request)?);
    } else {
        println!("source: {}", request.source_cli);
        println!("target: {}", request.target_cli);
        println!("session: {}", request.source_session.id);
        println!("rewind: {}", request.rewind_event_id);
        println!("compiler: {}", request.compiler);
    }
    Ok(())
}

fn print_compile_output(args: cli::CompileArgs) -> Result<()> {
    let output = core::workbench::compile_output(
        core::model::CliTool::Codex,
        core::model::CliTool::Hermes,
        args.compiler.as_deref(),
    )?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        println!("version: {}", output.version);
        println!("goal: {}", output.capsule.goal);
        println!("target: {}", output.capsule.target_branch);
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
