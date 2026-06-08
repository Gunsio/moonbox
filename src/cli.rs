use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};
use clap_complete::Shell;

use crate::core::model::{CliTool, ContinuationLevel, WorkspaceRestoreMode};

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
    Sessions(SessionListArgs),
    /// Print the command for opening an original session.
    Open(OpenArgs),
    /// Print the selected Work Capsule.
    Capsule(CompileArgs),
    /// Print the compiler request contract.
    CompileRequest(CompileArgs),
    /// Print the compiler output contract.
    CompileOutput(CompileArgs),
    /// List configured compiler skill presets.
    Compilers(JsonArgs),
    /// List configured SSH hosts without connecting.
    Ssh(JsonArgs),
    /// Diagnose Moonbox configuration without opening sessions.
    Doctor(JsonArgs),
    /// Capture a workspace continuation snapshot without opening sessions.
    Snapshot(SnapshotArgs),
    /// Generate shell completion scripts.
    Completions(CompletionsArgs),
    /// Dry-run a target launch plan and verification report.
    Launch(LaunchArgs),
    /// Verify the selected Work Capsule without launching.
    Verify(LaunchArgs),
    /// Replay embedded fixtures through compile and verify without opening sessions.
    ReplayEval(JsonArgs),
    /// Generate deterministic documentation assets.
    #[command(name = "docs-snapshot", hide = true)]
    DocsSnapshot(DocsSnapshotArgs),
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

#[derive(Debug, Args, Clone, Default)]
pub struct SessionListArgs {
    #[arg(long)]
    pub json: bool,
    /// Filter listed sessions by source CLI. Defaults to all sources.
    #[arg(long, value_enum)]
    pub filter: Option<CliTool>,
    /// Backward-compatible alias for --filter.
    #[arg(long, value_enum, hide = true)]
    pub source: Option<CliTool>,
}

#[derive(Debug, Args, Clone, Default)]
pub struct DocsSnapshotArgs {
    /// Snapshot terminal width in cells.
    #[arg(long, default_value_t = 160)]
    pub width: u16,
    /// Snapshot terminal height in cells.
    #[arg(long, default_value_t = 44)]
    pub height: u16,
    /// Write the SVG to this path instead of stdout.
    #[arg(long)]
    pub output: Option<PathBuf>,
}

#[derive(Debug, Args, Clone)]
pub struct SnapshotArgs {
    /// Workspace path to inspect. Defaults to the current directory.
    #[arg(long, default_value = ".")]
    pub path: PathBuf,
    /// Maximum diff preview lines per staged/unstaged section. 0 keeps full diffs.
    #[arg(long, default_value_t = 240)]
    pub diff_lines: usize,
    /// Explicit test or verification command to run and record. Repeatable.
    #[arg(long = "test-command")]
    pub test_commands: Vec<String>,
    /// Print JSON output.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args, Clone, Default)]
pub struct CompileArgs {
    #[arg(long)]
    pub json: bool,
    /// Source session id. Defaults to the newest discovered session.
    #[arg(long)]
    pub session: Option<String>,
    /// Target CLI. Defaults to the last confirmed target.
    #[arg(long, value_enum)]
    pub target: Option<CliTool>,
    /// Rewind event id. Defaults to Moonbox's recommended rewind point.
    #[arg(long)]
    pub rewind: Option<String>,
    /// Compiler id to use. Defaults to the configured external compiler or engineering-handoff.
    #[arg(long)]
    pub compiler: Option<String>,
}

#[derive(Debug, Args, Clone)]
pub struct CompletionsArgs {
    /// Shell to generate completions for.
    #[arg(value_enum)]
    pub shell: Shell,
    /// Binary name to generate completions for. Defaults to the invoked binary.
    #[arg(long = "bin", value_enum)]
    pub binary: Option<CompletionBinary>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum CompletionBinary {
    Moonbox,
    Moon,
}

impl CompletionBinary {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Moonbox => "moonbox",
            Self::Moon => "moon",
        }
    }
}

#[derive(Debug, Args, Clone)]
pub struct OpenArgs {
    /// Session id to open. Dry-runs default to the newest discovered session; --execute requires it.
    #[arg(long)]
    pub session: Option<String>,
    /// Execute the original CLI resume command instead of printing a dry-run plan.
    #[arg(long)]
    pub execute: bool,
    /// Print JSON output.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args, Clone, Default)]
pub struct LaunchArgs {
    /// Source session id. Dry-runs default to the newest discovered session; --execute requires it.
    #[arg(long)]
    pub session: Option<String>,
    /// Target CLI. Defaults to the last confirmed target.
    #[arg(long, value_enum)]
    pub target: Option<CliTool>,
    /// Work Capsule JSON file to read and validate. Defaults to a generated dry-run capsule.
    #[arg(long)]
    pub capsule: Option<String>,
    /// Execute the verified target command instead of printing a dry-run plan.
    #[arg(long)]
    pub execute: bool,
    /// Allow executing a real-session handoff produced by the built-in draft compiler.
    #[arg(long)]
    pub allow_draft: bool,
    /// Requested continuation level. Defaults to prompt-only handoff.
    #[arg(long, value_enum)]
    pub continuation: Option<ContinuationLevel>,
    /// Preview a reversible workspace restore path. Implies --continuation workspace-restore.
    #[arg(long = "workspace-restore", value_enum)]
    pub workspace_restore: Option<WorkspaceRestoreMode>,
    /// Print JSON output.
    #[arg(long)]
    pub json: bool,
}
