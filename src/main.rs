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
        Command::Capsule(args) => print_capsule(args),
    }
}

fn run_tui(args: cli::TuiArgs) -> Result<()> {
    let app = app::App::new(args.source, args.target);
    let mut terminal = ratatui::init();
    let result = tui::run(&mut terminal, app);
    ratatui::restore();
    result
}

fn print_sessions(args: cli::JsonArgs) -> Result<()> {
    let data = core::demo::demo_data(core::model::CliTool::Codex, core::model::CliTool::Hermes);
    if args.json {
        println!("{}", serde_json::to_string_pretty(&data.sessions)?);
    } else {
        for session in data.sessions {
            println!(
                "{:<8} {:<28} {:<24} {}",
                session.cli, session.title, session.cwd, session.updated
            );
        }
    }
    Ok(())
}

fn print_capsule(args: cli::JsonArgs) -> Result<()> {
    let data = core::demo::demo_data(core::model::CliTool::Codex, core::model::CliTool::Hermes);
    if args.json {
        println!("{}", serde_json::to_string_pretty(&data.capsule)?);
    } else {
        println!("goal: {}", data.capsule.goal);
        println!("state: {}", data.capsule.state);
        println!("rewind: {}", data.capsule.rewind_point);
        println!("target: {}", data.capsule.target_branch);
    }
    Ok(())
}
