use clap::{Args, Parser, Subcommand};

use crate::core::model::CliTool;

#[derive(Debug, Parser)]
#[command(
    name = "moonbox",
    version,
    about = "Cross-CLI session rewind workbench"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Open the Moonbox TUI workbench.
    Tui(TuiArgs),
    /// List discovered sessions.
    Sessions(JsonArgs),
    /// Print the command for opening an original session.
    Open(OpenArgs),
    /// Print the current Work Capsule.
    Capsule(JsonArgs),
    /// Print the compiler request contract fixture.
    CompileRequest(JsonArgs),
    /// Print the compiler output contract fixture.
    CompileOutput(JsonArgs),
    /// Dry-run a target launch plan and verification report.
    Launch(LaunchArgs),
    /// Verify the selected Work Capsule without launching.
    Verify(LaunchArgs),
}

impl Default for Command {
    fn default() -> Self {
        Self::Tui(TuiArgs::default())
    }
}

#[derive(Debug, Args, Clone, Default)]
pub struct TuiArgs {
    /// Initial session source filter. Defaults to all sessions.
    #[arg(long, value_enum)]
    pub filter: Option<CliTool>,
    /// Backward-compatible alias for --filter.
    #[arg(long, value_enum, hide = true)]
    pub source: Option<CliTool>,
    /// Initial target CLI. Defaults to the last confirmed target.
    #[arg(long, value_enum)]
    pub target: Option<CliTool>,
}

#[derive(Debug, Args, Clone, Default)]
pub struct JsonArgs {
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args, Clone)]
pub struct OpenArgs {
    /// Session id to open. Defaults to the newest discovered session.
    #[arg(long)]
    pub session: Option<String>,
}

#[derive(Debug, Args, Clone, Default)]
pub struct LaunchArgs {
    /// Source session id. Defaults to the newest discovered session.
    #[arg(long)]
    pub session: Option<String>,
    /// Target CLI. Defaults to the last confirmed target.
    #[arg(long, value_enum)]
    pub target: Option<CliTool>,
    /// Work Capsule JSON file to read and validate. Defaults to a generated dry-run capsule.
    #[arg(long)]
    pub capsule: Option<String>,
    /// Print JSON output.
    #[arg(long)]
    pub json: bool,
}
