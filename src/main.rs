mod app;
mod cli;
mod core;
mod tui;

use clap::Parser;
use cli::{Cli, Command};
use color_eyre::Result;

fn main() -> Result<()> {
    color_eyre::install()?;

    let cli = Cli::parse();
    match cli.command.unwrap_or_default() {
        Command::Tui(args) => run_tui(args),
        Command::Sessions(args) => print_sessions(args),
        Command::Open(args) => print_open_command(args),
        Command::Capsule(args) => print_capsule(args),
        Command::CompileRequest(args) => print_compile_request(args),
        Command::CompileOutput(args) => print_compile_output(args),
    }
}

fn run_tui(args: cli::TuiArgs) -> Result<()> {
    let target = args
        .target
        .or_else(core::config::load_last_target)
        .unwrap_or(core::model::CliTool::Hermes);
    let filter = args.filter.or(args.source);
    let source = filter.unwrap_or(core::model::CliTool::Codex);
    let mut app = app::App::new(source, target);
    if let Some(filter) = filter {
        app.apply_session_filter(app::SessionFilter::Tool(filter));
    }
    let mut terminal = ratatui::init();
    let result = tui::run(&mut terminal, app);
    ratatui::restore();
    result
}

fn print_sessions(args: cli::JsonArgs) -> Result<()> {
    let sessions = core::workbench::list_sessions();
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
    if let Some(command) = core::workbench::open_command(args.session.as_deref()) {
        println!("{command}");
    }
    Ok(())
}

fn print_capsule(args: cli::JsonArgs) -> Result<()> {
    let capsule =
        core::workbench::capsule(core::model::CliTool::Codex, core::model::CliTool::Hermes);
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

fn print_compile_request(args: cli::JsonArgs) -> Result<()> {
    let request = core::workbench::compile_request(
        core::model::CliTool::Codex,
        core::model::CliTool::Hermes,
        "evt-091",
    );
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

fn print_compile_output(args: cli::JsonArgs) -> Result<()> {
    let output =
        core::workbench::compile_output(core::model::CliTool::Codex, core::model::CliTool::Hermes);
    if args.json {
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        println!("version: {}", output.version);
        println!("goal: {}", output.capsule.goal);
        println!("target: {}", output.capsule.target_branch);
    }
    Ok(())
}
