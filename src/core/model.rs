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
pub struct DemoData {
    pub source: CliTool,
    pub target: CliTool,
    pub sessions: Vec<SessionSummary>,
    pub timeline: Vec<TimelineEvent>,
    pub capsule: WorkCapsule,
    pub branches: Vec<BranchNode>,
    pub compilers: Vec<String>,
}
