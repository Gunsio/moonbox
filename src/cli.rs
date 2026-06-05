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
    /// Print the current Work Capsule. Currently uses demo data.
    Capsule(JsonArgs),
}

impl Default for Command {
    fn default() -> Self {
        Self::Tui(TuiArgs::default())
    }
}

#[derive(Debug, Args, Clone)]
pub struct TuiArgs {
    #[arg(long, value_enum, default_value_t = CliTool::Codex)]
    pub source: CliTool,
    #[arg(long, value_enum, default_value_t = CliTool::Hermes)]
    pub target: CliTool,
}

impl Default for TuiArgs {
    fn default() -> Self {
        Self {
            source: CliTool::Codex,
            target: CliTool::Hermes,
        }
    }
}

#[derive(Debug, Args, Clone, Default)]
pub struct JsonArgs {
    #[arg(long)]
    pub json: bool,
}
