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
    /// List discovered sessions. Currently uses demo data.
    Sessions(JsonArgs),
    /// Print the command for opening an original session.
    Open(OpenArgs),
    /// Print the current Work Capsule. Currently uses demo data.
    Capsule(JsonArgs),
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
    /// Session id to open. Defaults to the first demo session.
    #[arg(long)]
    pub session: Option<String>,
}
