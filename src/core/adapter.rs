use std::{error::Error, fmt};

use super::model::{
    CanonicalTimeline, CliTool, SessionSummary, SourceAdapterReport, SourceProvenance,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdapterError {
    SessionNotFound {
        tool: CliTool,
        session_id: String,
    },
    ReadSource {
        tool: CliTool,
        path: String,
        reason: String,
    },
    InvalidFixture {
        tool: CliTool,
        path: String,
        reason: String,
    },
}

impl fmt::Display for AdapterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SessionNotFound { tool, session_id } => {
                write!(f, "{tool} session not found: {session_id}")
            }
            Self::ReadSource { tool, path, reason } => {
                write!(f, "cannot read {tool} source {path}: {reason}")
            }
            Self::InvalidFixture { tool, path, reason } => {
                write!(f, "{tool} fixture {path} is invalid: {reason}")
            }
        }
    }
}

impl Error for AdapterError {}

pub trait SourceAdapter {
    fn tool(&self) -> CliTool;
    fn provenance(&self) -> SourceProvenance;
    fn store_path(&self) -> Option<String>;
    fn list_sessions(&self) -> Result<Vec<SessionSummary>, AdapterError>;
    fn find_session(&self, session_id: &str) -> Result<Option<SessionSummary>, AdapterError> {
        Ok(self
            .list_sessions()?
            .into_iter()
            .find(|session| session.id == session_id))
    }
    fn load_timeline(&self, session_id: &str) -> Result<CanonicalTimeline, AdapterError>;
}

pub fn adapter_report(
    adapter: &dyn SourceAdapter,
    filter_status: impl Into<String>,
    reason: impl Into<String>,
) -> Result<SourceAdapterReport, AdapterError> {
    let sessions = adapter.list_sessions()?;
    Ok(report_from_sessions(
        adapter.tool(),
        adapter.provenance(),
        true,
        adapter.store_path(),
        filter_status,
        reason,
        &sessions,
    ))
}

pub fn report_from_sessions(
    cli: CliTool,
    provenance: SourceProvenance,
    active: bool,
    store_path: Option<String>,
    filter_status: impl Into<String>,
    reason: impl Into<String>,
    sessions: &[SessionSummary],
) -> SourceAdapterReport {
    SourceAdapterReport {
        cli,
        provenance,
        active,
        store_path,
        session_count: sessions.len(),
        skipped_record_count: sessions
            .iter()
            .map(|session| session.parse_skip_count)
            .sum(),
        last_indexed_at: sessions
            .iter()
            .map(|session| session.updated_at.as_str())
            .max()
            .map(str::to_owned),
        filter_status: filter_status.into(),
        reason: reason.into(),
    }
}

pub fn collect_sessions(
    adapters: &[&dyn SourceAdapter],
) -> Result<Vec<SessionSummary>, AdapterError> {
    let mut sessions = Vec::new();
    for adapter in adapters {
        let tool = adapter.tool();
        sessions.extend(adapter.list_sessions()?.into_iter().inspect(|session| {
            debug_assert_eq!(session.cli, tool);
        }));
    }
    sessions.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
    Ok(sessions)
}
