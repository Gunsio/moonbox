use std::{error::Error, fmt};

use super::{
    capability,
    model::{CanonicalTimeline, CliTool, SessionSummary, SourceAdapterReport, SourceProvenance},
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SourceScanStats {
    pub list_limit: Option<usize>,
    pub scan_entry_limit: Option<usize>,
    pub summary_line_limit: Option<usize>,
    pub scan_entry_count: usize,
    pub scan_truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceReportMeta {
    pub cli: CliTool,
    pub provenance: SourceProvenance,
    pub active: bool,
    pub store_path: Option<String>,
    pub filter_status: String,
    pub reason: String,
}

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
    fn list_sessions_with_report(
        &self,
        filter_status: &str,
        reason: &str,
    ) -> Result<(Vec<SessionSummary>, SourceAdapterReport), AdapterError> {
        let sessions = self.list_sessions()?;
        let report = report_from_sessions(
            self.tool(),
            self.provenance(),
            true,
            self.store_path(),
            filter_status,
            reason,
            &sessions,
        );
        Ok((sessions, report))
    }
    fn find_session(&self, session_id: &str) -> Result<Option<SessionSummary>, AdapterError> {
        Ok(self
            .list_sessions()?
            .into_iter()
            .find(|session| session.id == session_id))
    }
    fn load_timeline(&self, session_id: &str) -> Result<CanonicalTimeline, AdapterError>;
    fn load_timeline_limited(
        &self,
        session: &SessionSummary,
        event_limit: Option<usize>,
    ) -> Result<CanonicalTimeline, AdapterError> {
        let _ = event_limit;
        self.load_timeline(&session.id)
    }
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
    report_from_sessions_with_scan(
        SourceReportMeta {
            cli,
            provenance,
            active,
            store_path,
            filter_status: filter_status.into(),
            reason: reason.into(),
        },
        sessions,
        SourceScanStats {
            scan_entry_count: sessions.len(),
            ..SourceScanStats::default()
        },
    )
}

pub fn report_from_sessions_with_scan(
    meta: SourceReportMeta,
    sessions: &[SessionSummary],
    scan_stats: SourceScanStats,
) -> SourceAdapterReport {
    SourceAdapterReport {
        cli: meta.cli,
        provenance: meta.provenance,
        active: meta.active,
        store_path: meta.store_path,
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
        filter_status: meta.filter_status,
        reason: meta.reason,
        capabilities: capability::source_capabilities(meta.cli, meta.provenance),
        list_limit: scan_stats.list_limit,
        scan_entry_limit: scan_stats.scan_entry_limit,
        summary_line_limit: scan_stats.summary_line_limit,
        scan_entry_count: scan_stats.scan_entry_count,
        scan_truncated: scan_stats.scan_truncated,
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
