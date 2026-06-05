use std::{error::Error, fmt};

use super::model::{CanonicalTimeline, CliTool, SessionSummary};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdapterError {
    SessionNotFound { tool: CliTool, session_id: String },
}

impl fmt::Display for AdapterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SessionNotFound { tool, session_id } => {
                write!(f, "{tool} session not found: {session_id}")
            }
        }
    }
}

impl Error for AdapterError {}

pub trait SourceAdapter {
    fn tool(&self) -> CliTool;
    fn list_sessions(&self) -> Vec<SessionSummary>;
    fn load_timeline(&self, session_id: &str) -> Result<CanonicalTimeline, AdapterError>;
}

pub fn collect_sessions(adapters: &[&dyn SourceAdapter]) -> Vec<SessionSummary> {
    let mut sessions = Vec::new();
    for adapter in adapters {
        let tool = adapter.tool();
        sessions.extend(adapter.list_sessions().into_iter().inspect(|session| {
            debug_assert_eq!(session.cli, tool);
        }));
    }
    sessions.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
    sessions
}
