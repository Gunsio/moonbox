use std::{
    collections::HashMap,
    env,
    io::BufRead,
    path::{Path, PathBuf},
};

use serde::Deserialize;
use serde_json::Value;
use time::OffsetDateTime;

use super::{
    adapter::{AdapterError, SourceAdapter, SourceReportMeta, SourceScanStats},
    local_jsonl::{
        collect_project_jsonl_files, configured_session_limit, configured_session_scan_entry_limit,
        configured_session_summary_line_limit, discover_project_jsonl_files, display_time,
        event_id, find_token_count, human_timestamp, is_provider_context_text, max_timestamp,
        open_reader, push_timeline_event, read_error, sort_paths_by_modified_desc, text_from_value,
        title_case, truncate,
    },
    model::{
        CanonicalTimeline, CliTool, SessionStatus, SessionSummary, SourceProvenance, TimelineEvent,
        TimelineKind,
    },
};

const CLAUDE_TOOL: CliTool = CliTool::Claude;

#[derive(Debug, Clone)]
pub struct ClaudeSourceAdapter {
    root: PathBuf,
    list_limit: Option<usize>,
    scan_entry_limit: Option<usize>,
    summary_line_limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct ClaudeRecord {
    timestamp: Option<String>,
    #[serde(rename = "type")]
    record_type: Option<String>,
    #[serde(rename = "sessionId")]
    session_id: Option<String>,
    cwd: Option<String>,
    #[serde(rename = "gitBranch")]
    git_branch: Option<String>,
    #[serde(rename = "aiTitle")]
    ai_title: Option<String>,
    #[serde(rename = "permissionMode")]
    permission_mode: Option<String>,
    #[serde(rename = "lastPrompt", default)]
    last_prompt: Value,
    #[serde(rename = "leafUuid")]
    leaf_uuid: Option<String>,
    #[serde(default)]
    message: Value,
    #[serde(rename = "toolUseResult", default)]
    tool_use_result: Value,
}

#[derive(Debug, Deserialize)]
struct ClaudeHistoryRecord {
    display: Option<String>,
    timestamp: Option<i64>,
    project: Option<String>,
    #[serde(rename = "sessionId")]
    session_id: Option<String>,
}

#[derive(Debug, Clone)]
struct ClaudeHistoryEntry {
    session_id: String,
    project: String,
    path: PathBuf,
    timestamp_ms: i64,
    updated_at: String,
    display_title: Option<String>,
}

#[derive(Debug)]
struct SummaryBuilder {
    path: PathBuf,
    id: Option<String>,
    title: Option<String>,
    cwd: Option<String>,
    updated_at: Option<String>,
    branch: Option<String>,
    token_count: Option<usize>,
    event_count: usize,
    malformed_lines: usize,
    summary_truncated: bool,
    has_error: bool,
}

impl ClaudeSourceAdapter {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            list_limit: configured_session_limit(),
            scan_entry_limit: configured_session_scan_entry_limit(),
            summary_line_limit: configured_session_summary_line_limit(),
        }
    }

    #[cfg(test)]
    fn with_session_limit(root: impl Into<PathBuf>, list_limit: Option<usize>) -> Self {
        Self::with_limits(root, list_limit, None)
    }

    #[cfg(test)]
    fn with_limits(
        root: impl Into<PathBuf>,
        list_limit: Option<usize>,
        scan_entry_limit: Option<usize>,
    ) -> Self {
        Self::with_all_limits(root, list_limit, scan_entry_limit, None)
    }

    #[cfg(test)]
    fn with_all_limits(
        root: impl Into<PathBuf>,
        list_limit: Option<usize>,
        scan_entry_limit: Option<usize>,
        summary_line_limit: Option<usize>,
    ) -> Self {
        Self {
            root: root.into(),
            list_limit,
            scan_entry_limit,
            summary_line_limit,
        }
    }

    #[cfg(not(test))]
    pub fn from_default_home() -> Option<Self> {
        if let Some(path) = env::var_os("MOONBOX_CLAUDE_HOME") {
            return Some(Self::new(path));
        }
        if let Some(path) = env::var_os("CLAUDE_HOME") {
            return Some(Self::new(path));
        }
        env::var_os("HOME").map(|home| Self::new(PathBuf::from(home).join(".claude")))
    }

    #[cfg(not(test))]
    pub fn has_session_store(&self) -> bool {
        self.history_path().is_file() || self.projects_dir().is_dir()
    }

    #[cfg(not(test))]
    pub(crate) fn session_store_path(&self) -> PathBuf {
        if self.history_path().is_file() {
            self.history_path()
        } else {
            self.projects_dir()
        }
    }

    fn projects_dir(&self) -> PathBuf {
        self.root.join("projects")
    }

    fn history_path(&self) -> PathBuf {
        self.root.join("history.jsonl")
    }

    fn has_history_index(&self) -> bool {
        self.history_path().is_file()
    }

    fn listed_history_entries(&self) -> Result<Option<Vec<ClaudeHistoryEntry>>, AdapterError> {
        if !self.has_history_index() {
            return Ok(None);
        }
        let mut entries = self.history_entries()?;
        entries.sort_by(|left, right| {
            right
                .timestamp_ms
                .cmp(&left.timestamp_ms)
                .then_with(|| right.session_id.cmp(&left.session_id))
        });
        entries.retain(|entry| entry.path.is_file());
        if let Some(limit) = self.list_limit {
            entries.truncate(limit);
        }
        Ok(Some(entries))
    }

    fn history_entries(&self) -> Result<Vec<ClaudeHistoryEntry>, AdapterError> {
        let path = self.history_path();
        let reader = open_reader(CLAUDE_TOOL, &path)?;
        let mut entries = HashMap::<String, ClaudeHistoryEntry>::new();

        for line in reader.lines() {
            let line = line.map_err(|error| read_error(CLAUDE_TOOL, &path, error))?;
            if line.trim().is_empty() {
                continue;
            }
            let Ok(record) = serde_json::from_str::<ClaudeHistoryRecord>(&line) else {
                continue;
            };
            let (Some(session_id), Some(project), Some(timestamp_ms)) =
                (record.session_id, record.project, record.timestamp)
            else {
                continue;
            };
            let Some(updated_at) = timestamp_millis_to_rfc3339(timestamp_ms) else {
                continue;
            };
            let display_title = record.display.as_deref().and_then(history_display_title);
            let path = self.history_session_path(&project, &session_id);
            entries
                .entry(session_id.clone())
                .and_modify(|entry| {
                    if timestamp_ms >= entry.timestamp_ms {
                        entry.project = project.clone();
                        entry.path = path.clone();
                        entry.timestamp_ms = timestamp_ms;
                        entry.updated_at = updated_at.clone();
                    }
                    if display_title.is_some() {
                        entry.display_title = display_title.clone();
                    }
                })
                .or_insert(ClaudeHistoryEntry {
                    session_id,
                    project,
                    path,
                    timestamp_ms,
                    updated_at,
                    display_title,
                });
        }

        Ok(entries.into_values().collect())
    }

    fn history_entry(&self, session_id: &str) -> Result<Option<ClaudeHistoryEntry>, AdapterError> {
        if !self.has_history_index() {
            return Ok(None);
        }
        Ok(self
            .history_entries()?
            .into_iter()
            .find(|entry| entry.session_id == session_id))
    }

    fn history_session_path(&self, project: &str, session_id: &str) -> PathBuf {
        self.projects_dir()
            .join(claude_project_dir_name(project))
            .join(format!("{session_id}.jsonl"))
    }

    fn session_files(&self, limit: Option<usize>) -> Result<Vec<PathBuf>, AdapterError> {
        let mut files = collect_project_jsonl_files(CLAUDE_TOOL, &self.projects_dir())?;
        sort_paths_by_modified_desc(&mut files);
        if let Some(limit) = limit {
            files.truncate(limit);
        }
        Ok(files)
    }

    fn listed_session_files(&self) -> Result<Vec<PathBuf>, AdapterError> {
        Ok(self.listed_session_discovery()?.files)
    }

    fn listed_session_discovery(&self) -> Result<super::local_jsonl::JsonlDiscovery, AdapterError> {
        discover_project_jsonl_files(
            CLAUDE_TOOL,
            &self.projects_dir(),
            self.list_limit,
            self.scan_entry_limit,
        )
    }

    fn all_session_files(&self) -> Result<Vec<PathBuf>, AdapterError> {
        self.session_files(None)
    }

    fn parse_summary(&self, path: &Path) -> Result<SessionSummary, AdapterError> {
        self.parse_summary_limited(path, None)
    }

    fn parse_list_summary(&self, path: &Path) -> Result<SessionSummary, AdapterError> {
        self.parse_summary_limited(path, self.summary_line_limit)
    }

    fn parse_summary_limited(
        &self,
        path: &Path,
        line_limit: Option<usize>,
    ) -> Result<SessionSummary, AdapterError> {
        let mut builder = SummaryBuilder::new(path);
        let reader = open_reader(CLAUDE_TOOL, path)?;

        for (line_index, line) in reader.lines().enumerate() {
            if let Some(limit) = line_limit
                && line_index >= limit
            {
                builder.summary_truncated = true;
                break;
            }
            let line = line.map_err(|error| read_error(CLAUDE_TOOL, path, error))?;
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<ClaudeRecord>(&line) {
                Ok(record) => builder.observe(record),
                Err(_) => builder.malformed_lines += 1,
            }
        }

        Ok(builder.finish())
    }

    fn find_session_path(&self, session_id: &str) -> Result<Option<PathBuf>, AdapterError> {
        if let Some(entry) = self.history_entry(session_id)?
            && entry.path.is_file()
        {
            return Ok(Some(entry.path));
        }
        self.find_project_session_path(session_id)
    }

    fn find_project_session_path(&self, session_id: &str) -> Result<Option<PathBuf>, AdapterError> {
        for path in self.all_session_files()? {
            if id_from_path(&path) == session_id {
                return Ok(Some(path));
            }
            let summary = self.parse_summary(&path)?;
            if summary.id == session_id {
                return Ok(Some(path));
            }
        }
        Ok(None)
    }

    fn parse_history_summary(
        &self,
        entry: &ClaudeHistoryEntry,
    ) -> Result<SessionSummary, AdapterError> {
        let mut summary = self.parse_list_summary(&entry.path)?;
        summary.updated_at = entry.updated_at.clone();
        summary.updated = human_timestamp(&entry.updated_at);
        if summary.title.starts_with("Claude session ")
            && let Some(title) = entry.display_title.as_deref()
        {
            summary.title = title.to_owned();
        }
        summary.health_reason = Some(match summary.health_reason.take() {
            Some(reason) => format!("{reason}; indexed by Claude history"),
            None => "real Claude JSONL session; indexed by Claude history".into(),
        });
        Ok(summary)
    }

    fn parse_timeline(
        &self,
        session_id: &str,
        path: &Path,
        event_limit: Option<usize>,
    ) -> Result<CanonicalTimeline, AdapterError> {
        let reader = open_reader(CLAUDE_TOOL, path)?;
        let mut events = Vec::new();

        for (line_index, line) in reader.lines().enumerate() {
            let line = line.map_err(|error| read_error(CLAUDE_TOOL, path, error))?;
            if line.trim().is_empty() {
                continue;
            }

            let record = match serde_json::from_str::<ClaudeRecord>(&line) {
                Ok(record) => record,
                Err(error) => {
                    let event = TimelineEvent {
                        id: event_id(events.len() + 1),
                        time: "??:??".into(),
                        kind: TimelineKind::Error,
                        title: "Malformed event".into(),
                        detail: format!("line {}: {}", line_index + 1, error),
                    };
                    if push_timeline_event(&mut events, event, event_limit) {
                        break;
                    }
                    continue;
                }
            };

            if let Some(event) = timeline_event(record, events.len() + 1)
                && push_timeline_event(&mut events, event, event_limit)
            {
                break;
            }
        }

        Ok(CanonicalTimeline {
            version: 1,
            source_cli: CLAUDE_TOOL,
            source_session: session_id.into(),
            events,
        })
    }
}

impl SourceAdapter for ClaudeSourceAdapter {
    fn tool(&self) -> CliTool {
        CLAUDE_TOOL
    }

    fn provenance(&self) -> SourceProvenance {
        SourceProvenance::Real
    }

    fn store_path(&self) -> Option<String> {
        Some(
            if self.has_history_index() {
                self.history_path()
            } else {
                self.projects_dir()
            }
            .display()
            .to_string(),
        )
    }

    fn list_sessions(&self) -> Result<Vec<SessionSummary>, AdapterError> {
        if let Some(entries) = self.listed_history_entries()? {
            return entries
                .iter()
                .map(|entry| self.parse_history_summary(entry))
                .collect();
        }
        let mut sessions = Vec::new();
        for path in self.listed_session_files()? {
            sessions.push(self.parse_list_summary(&path)?);
        }
        Ok(sessions)
    }

    fn list_sessions_with_report(
        &self,
        filter_status: &str,
        reason: &str,
    ) -> Result<(Vec<SessionSummary>, super::model::SourceAdapterReport), AdapterError> {
        if let Some(entries) = self.listed_history_entries()? {
            let sessions = entries
                .iter()
                .map(|entry| self.parse_history_summary(entry))
                .collect::<Result<Vec<_>, _>>()?;
            let report = super::adapter::report_from_sessions_with_scan(
                SourceReportMeta {
                    cli: self.tool(),
                    provenance: self.provenance(),
                    active: true,
                    store_path: self.store_path(),
                    filter_status: filter_status.into(),
                    reason: reason.into(),
                },
                &sessions,
                SourceScanStats {
                    list_limit: self.list_limit,
                    scan_entry_count: sessions.len(),
                    summary_line_limit: self.summary_line_limit,
                    ..SourceScanStats::default()
                },
            );
            return Ok((sessions, report));
        }
        let discovery = self.listed_session_discovery()?;
        let mut sessions = Vec::new();
        for path in discovery.files {
            sessions.push(self.parse_list_summary(&path)?);
        }
        let report = super::adapter::report_from_sessions_with_scan(
            SourceReportMeta {
                cli: self.tool(),
                provenance: self.provenance(),
                active: true,
                store_path: self.store_path(),
                filter_status: filter_status.into(),
                reason: reason.into(),
            },
            &sessions,
            super::adapter::SourceScanStats {
                summary_line_limit: self.summary_line_limit,
                ..discovery.scan_stats
            },
        );
        Ok((sessions, report))
    }

    fn find_session(&self, session_id: &str) -> Result<Option<SessionSummary>, AdapterError> {
        let Some(path) = self.find_session_path(session_id)? else {
            return Ok(None);
        };
        self.parse_summary(&path).map(Some)
    }

    fn load_timeline(&self, session_id: &str) -> Result<CanonicalTimeline, AdapterError> {
        let Some(path) = self.find_session_path(session_id)? else {
            return Err(AdapterError::SessionNotFound {
                tool: CLAUDE_TOOL,
                session_id: session_id.into(),
            });
        };
        self.parse_timeline(session_id, &path, None)
    }

    fn load_timeline_limited(
        &self,
        session: &SessionSummary,
        event_limit: Option<usize>,
    ) -> Result<CanonicalTimeline, AdapterError> {
        if let Some(path) = session
            .source_path
            .as_deref()
            .map(PathBuf::from)
            .filter(|path| path.is_file())
        {
            return self.parse_timeline(&session.id, &path, event_limit);
        }
        let Some(path) = self.find_session_path(&session.id)? else {
            return Err(AdapterError::SessionNotFound {
                tool: CLAUDE_TOOL,
                session_id: session.id.clone(),
            });
        };
        self.parse_timeline(&session.id, &path, event_limit)
    }
}

fn timestamp_millis_to_rfc3339(timestamp_ms: i64) -> Option<String> {
    let seconds = timestamp_ms.div_euclid(1000);
    let millis = timestamp_ms.rem_euclid(1000);
    let time = OffsetDateTime::from_unix_timestamp(seconds).ok()?;
    Some(format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        time.year(),
        u8::from(time.month()),
        time.day(),
        time.hour(),
        time.minute(),
        time.second(),
        millis,
    ))
}

fn claude_project_dir_name(project: &str) -> String {
    project
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .collect()
}

fn history_display_title(display: &str) -> Option<String> {
    let title = normalized_text(display)?;
    let title = title.trim();
    if title.is_empty() || title.starts_with('/') || is_provider_context_text(title) {
        None
    } else {
        Some(truncate(title, 72))
    }
}

impl SummaryBuilder {
    fn new(path: &Path) -> Self {
        Self {
            path: path.into(),
            id: Some(id_from_path(path)),
            title: None,
            cwd: None,
            updated_at: None,
            branch: None,
            token_count: None,
            event_count: 0,
            malformed_lines: 0,
            summary_truncated: false,
            has_error: false,
        }
    }

    fn observe(&mut self, record: ClaudeRecord) {
        self.event_count += 1;
        if let Some(session_id) = record.session_id.as_deref() {
            self.id = Some(session_id.into());
        }
        if let Some(timestamp) = record.timestamp.as_deref() {
            self.updated_at = Some(max_timestamp(self.updated_at.take(), timestamp));
        }
        if self.cwd.is_none() {
            self.cwd = record.cwd.clone();
        }
        if self.branch.is_none() {
            self.branch = record.git_branch.clone();
        }
        if let Some(title) = record.ai_title.as_deref().and_then(normalized_text) {
            self.title = Some(truncate(&title, 72));
        }

        let record_type = record.record_type.as_deref().unwrap_or_default();
        self.has_error |= record_type.contains("error");
        if self.title.is_none()
            && record_type == "user"
            && !has_tool_result(&record)
            && let Some(text) = message_text(&record)
            && !is_provider_context_text(&text)
        {
            self.title = Some(truncate(&text, 72));
        }
        if record_type == "assistant"
            && let Some(count) = usage_token_count(&record.message)
        {
            self.token_count = Some(self.token_count.unwrap_or(0).max(count));
        }
    }

    fn finish(self) -> SessionSummary {
        let id = self.id.unwrap_or_else(|| id_from_path(&self.path));
        let updated_at = self
            .updated_at
            .unwrap_or_else(|| "1970-01-01T00:00:00+00:00".into());
        let status = if self.has_error {
            SessionStatus::Failed
        } else if self.malformed_lines > 0 {
            SessionStatus::Warning
        } else {
            SessionStatus::Healthy
        };
        let health_reason = if self.summary_truncated && self.malformed_lines > 0 {
            format!(
                "real Claude JSONL session; summary preview truncated; skipped {} malformed line(s)",
                self.malformed_lines
            )
        } else if self.summary_truncated {
            "real Claude JSONL session; summary preview truncated".into()
        } else if self.malformed_lines > 0 {
            format!(
                "real Claude JSONL session; skipped {} malformed line(s)",
                self.malformed_lines
            )
        } else {
            "real Claude JSONL session".into()
        };

        SessionSummary {
            id: id.clone(),
            cli: CLAUDE_TOOL,
            title: self
                .title
                .unwrap_or_else(|| format!("Claude session {}", short_id(&id))),
            cwd: self.cwd.unwrap_or_else(|| "~".into()),
            updated: human_timestamp(&updated_at),
            updated_at,
            status,
            branch: self.branch,
            token_count: self.token_count,
            health_reason: Some(health_reason),
            event_count: self.event_count,
            resume_command: format!("claude --resume {id}"),
            source_provenance: SourceProvenance::Real,
            source_path: Some(self.path.display().to_string()),
            parse_skip_count: self.malformed_lines,
        }
    }
}

fn timeline_event(record: ClaudeRecord, number: usize) -> Option<TimelineEvent> {
    let record_type = record.record_type.as_deref().unwrap_or_default();
    let kind = timeline_kind(record_type, &record)?;
    let detail = timeline_detail(record_type, &record);
    if detail.is_empty() && !matches!(kind, TimelineKind::Error) {
        return None;
    }
    if kind == TimelineKind::User && is_provider_context_text(&detail) {
        return None;
    }

    Some(TimelineEvent {
        id: event_id(number),
        time: display_time(record.timestamp.as_deref()),
        kind,
        title: timeline_title(record_type, &record),
        detail,
    })
}

fn timeline_kind(record_type: &str, record: &ClaudeRecord) -> Option<TimelineKind> {
    if record_type.contains("error") {
        return Some(TimelineKind::Error);
    }
    if record_type == "summary" || record_type.contains("compact") {
        return Some(TimelineKind::Compact);
    }
    match record_type {
        "user" if has_tool_result(record) => Some(TimelineKind::Tool),
        "user" => Some(TimelineKind::User),
        "assistant" if message_content_has_type(record, "tool_use") => Some(TimelineKind::Tool),
        "assistant" => Some(TimelineKind::Assistant),
        "ai-title" | "attachment" | "permission-mode" | "last-prompt" => Some(TimelineKind::Tool),
        "file-history-snapshot" => None,
        _ if !record_type.is_empty() => Some(TimelineKind::Tool),
        _ => None,
    }
}

fn timeline_title(record_type: &str, record: &ClaudeRecord) -> String {
    match record_type {
        "user" if has_tool_result(record) => "Tool result".into(),
        "user" => "User".into(),
        "assistant" if message_content_has_type(record, "tool_use") => "Tool call".into(),
        "assistant" => "Assistant".into(),
        "ai-title" => "Session title".into(),
        "permission-mode" => "Permission mode".into(),
        "last-prompt" => "Last prompt".into(),
        "attachment" => "Attachment".into(),
        "summary" => "Compact".into(),
        _ if record_type.contains("error") => "Error".into(),
        _ => title_case(record_type),
    }
}

fn timeline_detail(record_type: &str, record: &ClaudeRecord) -> String {
    match record_type {
        "ai-title" => record.ai_title.clone().unwrap_or_default(),
        "permission-mode" => record
            .permission_mode
            .as_deref()
            .map(|mode| format!("mode: {mode}"))
            .unwrap_or_default(),
        "last-prompt" => text_from_value(&record.last_prompt)
            .or_else(|| record.leaf_uuid.clone())
            .unwrap_or_default(),
        _ if has_tool_result(record) => text_from_value(&record.tool_use_result)
            .or_else(|| message_text(record))
            .map(|text| truncate(&text, 220))
            .unwrap_or_else(|| "tool result".into()),
        _ => message_text(record)
            .map(|text| truncate(&text, 220))
            .or_else(|| record.cwd.as_deref().map(|cwd| format!("cwd: {cwd}")))
            .unwrap_or_default(),
    }
}

fn message_text(record: &ClaudeRecord) -> Option<String> {
    text_from_value(record.message.get("content").unwrap_or(&Value::Null))
}

fn usage_token_count(message: &Value) -> Option<usize> {
    let usage = message.get("usage")?;
    let direct_sum = [
        "input_tokens",
        "cache_creation_input_tokens",
        "cache_read_input_tokens",
        "output_tokens",
    ]
    .iter()
    .filter_map(|key| usage.get(key).and_then(Value::as_u64))
    .try_fold(0usize, |sum, count| {
        usize::try_from(count)
            .ok()
            .and_then(|count| sum.checked_add(count))
    });
    direct_sum
        .filter(|sum| *sum > 0)
        .or_else(|| find_token_count(usage))
}

fn has_tool_result(record: &ClaudeRecord) -> bool {
    !record.tool_use_result.is_null() || message_content_has_type(record, "tool_result")
}

fn message_content_has_type(record: &ClaudeRecord, content_type: &str) -> bool {
    value_has_type(
        record.message.get("content").unwrap_or(&Value::Null),
        content_type,
    )
}

fn value_has_type(value: &Value, content_type: &str) -> bool {
    match value {
        Value::Array(items) => items.iter().any(|item| value_has_type(item, content_type)),
        Value::Object(object) => object
            .get("type")
            .and_then(Value::as_str)
            .is_some_and(|value| value == content_type),
        _ => false,
    }
}

fn normalized_text(text: &str) -> Option<String> {
    text_from_value(&Value::String(text.into()))
}

fn id_from_path(path: &Path) -> String {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("claude-session")
        .to_owned()
}

fn short_id(id: &str) -> String {
    id.chars().take(8).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, io::Write};

    #[test]
    fn lists_claude_sessions_from_project_jsonl_store() {
        let root = test_root("list");
        write_session(
            &root,
            "repo/claude-real-1.jsonl",
            r#"{"type":"user","sessionId":"claude-real-1","timestamp":"2026-05-19T07:55:10.994Z","cwd":"/repo","gitBranch":"main","message":{"role":"user","content":"Fix a cross CLI handoff"}}
{"type":"ai-title","sessionId":"claude-real-1","aiTitle":"Cross CLI handoff"}
{"type":"assistant","sessionId":"claude-real-1","timestamp":"2026-05-19T07:56:10.994Z","cwd":"/repo","gitBranch":"main","message":{"role":"assistant","content":[{"type":"text","text":"Done"}],"usage":{"input_tokens":10,"cache_read_input_tokens":20,"output_tokens":5}}}
"#,
        );

        let sessions = ClaudeSourceAdapter::new(&root)
            .list_sessions()
            .expect("sessions");

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, "claude-real-1");
        assert_eq!(sessions[0].title, "Cross CLI handoff");
        assert_eq!(sessions[0].cwd, "/repo");
        assert_eq!(sessions[0].branch.as_deref(), Some("main"));
        assert_eq!(sessions[0].token_count, Some(35));
        assert_eq!(sessions[0].resume_command, "claude --resume claude-real-1");
    }

    #[test]
    fn lists_claude_sessions_from_history_index_when_available() {
        let root = test_root("history-index");
        write_session(
            &root,
            "-repo-a/session-a.jsonl",
            r#"{"type":"user","sessionId":"session-a","timestamp":"2026-06-01T09:00:00.000Z","cwd":"/repo/a","message":{"content":"old jsonl"}}
{"type":"ai-title","sessionId":"session-a","aiTitle":"Session A title"}
"#,
        );
        write_session(
            &root,
            "-repo-b/session-b.jsonl",
            r#"{"type":"user","sessionId":"session-b","timestamp":"2026-06-01T08:00:00.000Z","cwd":"/repo/b","message":{"content":"newer history"}}
"#,
        );
        write_history(
            &root,
            r#"{"display":"Session A prompt","timestamp":1780300000000,"project":"/repo/a","sessionId":"session-a"}
{"display":"Session B prompt","timestamp":1780400000000,"project":"/repo/b","sessionId":"session-b"}
{"display":"/resume","timestamp":1780500000000,"project":"/repo/current","sessionId":"missing-current"}
"#,
        );

        let sessions = ClaudeSourceAdapter::new(&root)
            .list_sessions()
            .expect("sessions");

        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].id, "session-b");
        assert_eq!(sessions[0].title, "newer history");
        assert_eq!(sessions[0].updated_at, "2026-06-02T11:33:20.000Z");
        assert_eq!(sessions[1].id, "session-a");
        assert_eq!(sessions[1].title, "Session A title");
    }

    #[test]
    fn loads_canonical_timeline_from_claude_jsonl_store() {
        let root = test_root("timeline");
        write_session(
            &root,
            "repo/claude-real-2.jsonl",
            r#"{"type":"user","sessionId":"claude-real-2","timestamp":"2026-05-19T07:55:10.994Z","cwd":"/repo","message":{"role":"user","content":"Start here"}}
{"type":"assistant","sessionId":"claude-real-2","timestamp":"2026-05-19T07:55:20.994Z","cwd":"/repo","message":{"role":"assistant","content":[{"type":"text","text":"Working"}]}}
{"type":"assistant","sessionId":"claude-real-2","timestamp":"2026-05-19T07:55:30.994Z","cwd":"/repo","message":{"role":"assistant","content":[{"type":"tool_use","name":"Read","input":{"file_path":"src/main.rs"}}]}}
{"type":"user","sessionId":"claude-real-2","timestamp":"2026-05-19T07:55:40.994Z","cwd":"/repo","message":{"role":"user","content":[{"type":"tool_result","content":"ok"}]},"toolUseResult":{"stdout":"ok"}}
{"type":"summary","sessionId":"claude-real-2","timestamp":"2026-05-19T07:56:00.994Z","cwd":"/repo","message":{"content":"Compacted summary"}}
"#,
        );

        let timeline = ClaudeSourceAdapter::new(&root)
            .load_timeline("claude-real-2")
            .expect("timeline");

        assert_eq!(timeline.source_cli, CliTool::Claude);
        assert_eq!(timeline.source_session, "claude-real-2");
        assert_eq!(timeline.events[0].kind, TimelineKind::User);
        assert_eq!(timeline.events[1].kind, TimelineKind::Assistant);
        assert_eq!(timeline.events[2].kind, TimelineKind::Tool);
        assert_eq!(timeline.events[3].kind, TimelineKind::Tool);
        assert_eq!(timeline.events[4].kind, TimelineKind::Compact);
    }

    #[test]
    fn load_timeline_deduplicates_adjacent_duplicate_messages() {
        let root = test_root("timeline-dedup");
        write_session(
            &root,
            "repo/claude-dedup.jsonl",
            r#"{"type":"user","sessionId":"claude-dedup","timestamp":"2026-05-19T07:55:10.994Z","cwd":"/repo","message":{"role":"user","content":"Repeat once"}}
{"type":"user","sessionId":"claude-dedup","timestamp":"2026-05-19T07:55:10.994Z","cwd":"/repo","message":{"role":"user","content":"Repeat once"}}
"#,
        );

        let timeline = ClaudeSourceAdapter::new(&root)
            .load_timeline("claude-dedup")
            .expect("timeline");

        assert_eq!(
            timeline
                .events
                .iter()
                .filter(|event| event.detail == "Repeat once")
                .count(),
            1
        );
    }

    #[test]
    fn loads_explicit_claude_session_outside_list_limit() {
        let root = test_root("explicit-outside-limit");
        write_session(
            &root,
            "repo/session-z-new.jsonl",
            r#"{"type":"user","sessionId":"session-z-new","timestamp":"2026-06-06T09:00:00.000Z","cwd":"/repo","message":{"content":"new"}}"#,
        );
        write_session(
            &root,
            "repo/session-a-old.jsonl",
            r#"{"type":"user","sessionId":"session-a-old","timestamp":"2026-06-05T09:00:00.000Z","cwd":"/repo","message":{"content":"old"}}"#,
        );
        let adapter = ClaudeSourceAdapter::with_session_limit(&root, Some(1));

        let listed = adapter.list_sessions().expect("sessions");
        let found = adapter
            .find_session("session-a-old")
            .expect("find session")
            .expect("old session");
        let timeline = adapter
            .load_timeline("session-a-old")
            .expect("old timeline");

        assert_eq!(listed.len(), 1);
        assert_eq!(found.id, "session-a-old");
        assert_eq!(timeline.source_session, "session-a-old");
        assert_eq!(timeline.events[0].detail, "old");
    }

    #[test]
    fn list_report_exposes_scan_budget_truncation() {
        let root = test_root("scan-budget");
        for id in ["session-a-old", "session-b-new"] {
            write_session(
                &root,
                &format!("repo/{id}.jsonl"),
                &format!(
                    r#"{{"type":"user","sessionId":"{id}","timestamp":"2026-06-06T09:00:00.000Z","cwd":"/repo","message":{{"content":"{id}"}}}}"#
                ),
            );
        }
        let adapter = ClaudeSourceAdapter::with_limits(&root, Some(5), Some(2));

        let (sessions, report) = adapter
            .list_sessions_with_report("included_real_store", "test")
            .expect("report");

        assert_eq!(sessions.len(), 1);
        assert_eq!(report.session_count, 1);
        assert_eq!(report.list_limit, Some(5));
        assert_eq!(report.scan_entry_limit, Some(2));
        assert_eq!(report.scan_entry_count, 2);
        assert!(report.scan_truncated);
    }

    #[test]
    fn timeline_preview_stops_at_event_limit() {
        let root = test_root("timeline-preview-limit");
        write_session(
            &root,
            "repo/claude-long.jsonl",
            r#"{"type":"user","sessionId":"claude-long","timestamp":"2026-06-06T10:00:00.000Z","cwd":"/repo","message":{"content":"first"}}
{"type":"assistant","sessionId":"claude-long","timestamp":"2026-06-06T10:01:00.000Z","cwd":"/repo","message":{"content":[{"type":"text","text":"second"}]}}
{"type":"assistant","sessionId":"claude-long","timestamp":"2026-06-06T10:02:00.000Z","cwd":"/repo","message":{"content":[{"type":"text","text":"third should not parse"}]}}"#,
        );
        let adapter = ClaudeSourceAdapter::new(&root);
        let session = adapter
            .find_session("claude-long")
            .expect("find")
            .expect("session");

        let timeline = adapter
            .load_timeline_limited(&session, Some(2))
            .expect("timeline");

        assert_eq!(timeline.events.len(), 3);
        assert_eq!(timeline.events[0].detail, "first");
        assert_eq!(timeline.events[1].detail, "second");
        assert_eq!(timeline.events[2].title, "Timeline preview truncated");
    }

    #[test]
    fn list_summary_stops_at_summary_line_limit() {
        let root = test_root("summary-line-limit");
        write_session(
            &root,
            "repo/claude-limited.jsonl",
            r#"{"type":"user","sessionId":"claude-limited","timestamp":"2026-06-06T10:00:00.000Z","cwd":"/repo","message":{"content":"visible"}}
not-json-after-limit"#,
        );
        let adapter = ClaudeSourceAdapter::with_all_limits(&root, Some(10), None, Some(1));

        let sessions = adapter.list_sessions().expect("sessions");

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, "claude-limited");
        assert_eq!(sessions[0].parse_skip_count, 0);
        assert!(
            sessions[0]
                .health_reason
                .as_deref()
                .expect("health")
                .contains("summary preview truncated")
        );
    }

    fn test_root(name: &str) -> PathBuf {
        let root = env::temp_dir().join(format!(
            "moonbox-claude-adapter-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("root");
        root
    }

    fn write_session(root: &Path, relative_path: &str, contents: &str) {
        let path = root.join("projects").join(relative_path);
        fs::create_dir_all(path.parent().expect("parent")).expect("dirs");
        let mut file = fs::File::create(path).expect("file");
        file.write_all(contents.as_bytes()).expect("write");
    }

    fn write_history(root: &Path, contents: &str) {
        fs::write(root.join("history.jsonl"), contents).expect("history");
    }
}
