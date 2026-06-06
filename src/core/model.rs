use std::fmt::{Display, Formatter};

use clap::ValueEnum;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum CliTool {
    Codex,
    Claude,
    Hermes,
}

impl CliTool {
    pub const ALL: [Self; 3] = [Self::Codex, Self::Claude, Self::Hermes];

    pub fn id(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
            Self::Hermes => "hermes",
        }
    }

    pub fn next(self) -> Self {
        let current = Self::ALL.iter().position(|tool| *tool == self).unwrap_or(0);
        Self::ALL[(current + 1) % Self::ALL.len()]
    }

    pub fn previous(self) -> Self {
        let current = Self::ALL.iter().position(|tool| *tool == self).unwrap_or(0);
        Self::ALL[(current + Self::ALL.len() - 1) % Self::ALL.len()]
    }
}

impl Display for CliTool {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            CliTool::Codex => f.write_str("Codex"),
            CliTool::Claude => f.write_str("Claude"),
            CliTool::Hermes => f.write_str("Hermes"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Healthy,
    Warning,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    pub id: String,
    pub cli: CliTool,
    pub title: String,
    pub cwd: String,
    pub updated_at: String,
    pub updated: String,
    pub status: SessionStatus,
    pub branch: Option<String>,
    pub token_count: Option<usize>,
    pub health_reason: Option<String>,
    pub event_count: usize,
    pub resume_command: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimelineKind {
    User,
    Assistant,
    Tool,
    Compact,
    Error,
    GitDiff,
    RewindPoint,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineEvent {
    pub id: String,
    pub time: String,
    pub kind: TimelineKind,
    pub title: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkCapsule {
    pub version: u16,
    pub source_cli: CliTool,
    pub target_cli: CliTool,
    pub source_session: String,
    pub rewind_point: String,
    pub compiler: String,
    pub target_branch: String,
    pub goal: String,
    pub state: String,
    pub decisions: Vec<String>,
    pub todo: Vec<ChecklistItem>,
    pub evidence: Vec<String>,
    pub risks: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChecklistItem {
    pub done: bool,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchNode {
    pub id: String,
    pub label: String,
    pub detail: String,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkbenchData {
    pub source: CliTool,
    pub target: CliTool,
    pub sessions: Vec<SessionSummary>,
    pub timeline: Vec<TimelineEvent>,
    pub capsule: WorkCapsule,
    pub branches: Vec<BranchNode>,
    pub compilers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalTimeline {
    pub version: u16,
    pub source_cli: CliTool,
    pub source_session: String,
    pub events: Vec<TimelineEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapsuleCompileRequest {
    pub version: u16,
    pub source_cli: CliTool,
    pub target_cli: CliTool,
    pub source_session: SessionSummary,
    pub rewind_event_id: String,
    pub token_budget: usize,
    pub compiler: String,
    pub timeline: CanonicalTimeline,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapsuleCompileOutput {
    pub version: u16,
    pub capsule: WorkCapsule,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationStatus {
    Pass,
    Warn,
    Fail,
}

impl Display for VerificationStatus {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            VerificationStatus::Pass => f.write_str("PASS"),
            VerificationStatus::Warn => f.write_str("WARN"),
            VerificationStatus::Fail => f.write_str("FAIL"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationCheck {
    pub name: String,
    pub status: VerificationStatus,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationReport {
    pub version: u16,
    pub status: VerificationStatus,
    pub ready: bool,
    pub checks: Vec<VerificationCheck>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaunchValidationState {
    Ready,
    Warning,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaunchValidation {
    pub state: LaunchValidationState,
    pub reasons: Vec<String>,
}

impl LaunchValidation {
    pub fn ready() -> Self {
        Self {
            state: LaunchValidationState::Ready,
            reasons: vec!["Ready".into()],
        }
    }

    pub fn warning(reasons: Vec<String>) -> Self {
        Self {
            state: LaunchValidationState::Warning,
            reasons,
        }
    }

    pub fn blocked(reasons: Vec<String>) -> Self {
        Self {
            state: LaunchValidationState::Blocked,
            reasons,
        }
    }

    pub fn summary(&self) -> String {
        self.reasons.join("; ")
    }

    pub fn is_blocked(&self) -> bool {
        self.state == LaunchValidationState::Blocked
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaunchPlan {
    pub version: u16,
    pub dry_run: bool,
    pub source_session: SessionSummary,
    pub target_cli: CliTool,
    pub target_branch: String,
    pub capsule_path: Option<String>,
    pub command: String,
    pub target_command: TargetLaunchCommand,
    pub verification: VerificationReport,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetLaunchCommand {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<String>,
    pub display: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaunchExecutionStatus {
    Success,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaunchExecution {
    pub version: u16,
    pub status: LaunchExecutionStatus,
    pub exit_code: Option<i32>,
    pub plan: LaunchPlan,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OriginalSessionPlan {
    pub version: u16,
    pub dry_run: bool,
    pub source_session: SessionSummary,
    pub command: TargetLaunchCommand,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OriginalSessionExecution {
    pub version: u16,
    pub status: LaunchExecutionStatus,
    pub exit_code: Option<i32>,
    pub plan: OriginalSessionPlan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompilerPresetKind {
    Builtin,
    Environment,
    Config,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompilerPresetStatus {
    Ready,
    Warning,
    Disabled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompilerPresetInfo {
    pub id: String,
    pub kind: CompilerPresetKind,
    pub status: CompilerPresetStatus,
    pub score: u8,
    pub command: Option<String>,
    pub args: Vec<String>,
    pub timeout_ms: Option<u64>,
    pub reason: String,
}
