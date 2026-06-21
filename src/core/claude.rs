use std::{
    collections::HashMap,
    env, fs,
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
        event_id, extract_timeline_image_markup, find_token_count, human_timestamp,
        is_moonbox_handoff_control_text, is_provider_context_text, max_timestamp, open_reader,
        push_timeline_event, read_error, sort_paths_by_modified_desc, stable_text_digest,
        stable_value_digest, text_from_value, title_case, truncate, truncate_timeline_detail,
    },
    model::{
        CanonicalTimeline, CliTool, ContextHealth, EvidenceConfidence, SessionRuntimeStatus,
        SessionStatus, SessionSummary, SourceFidelity, SourceFidelityStatus, SourceProvenance,
        TimelineAttachment, TimelineCostMetadata, TimelineEvent, TimelineEventMetadata,
        TimelineEventRawRef, TimelineFileChange, TimelineKind, TimelineRuntimeMetadata,
        TimelineToolCall, TimelineToolResult, TokenBreakdown, unknown_runtime_reason,
    },
    model_context::resolve_model_context_window,
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
    subtype: Option<String>,
    #[serde(rename = "sessionId")]
    session_id: Option<String>,
    #[serde(rename = "session_id")]
    session_id_snake: Option<String>,
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
    #[serde(default)]
    result: Value,
    #[serde(default)]
    error: Value,
    #[serde(rename = "toolUseResult", default)]
    tool_use_result: Value,
    #[serde(rename = "total_cost_usd")]
    total_cost_usd: Option<f64>,
    #[serde(rename = "duration_ms")]
    duration_ms: Option<u64>,
    #[serde(rename = "duration_api_ms")]
    duration_api_ms: Option<u64>,
    #[serde(rename = "num_turns")]
    num_turns: Option<u64>,
    #[serde(rename = "is_error")]
    is_error: Option<bool>,
    #[serde(rename = "parentSessionId")]
    parent_session_id_camel: Option<String>,
    #[serde(rename = "parent_session_id")]
    parent_session_id_snake: Option<String>,
    #[serde(rename = "forkedFromSessionId")]
    forked_from_session_id_camel: Option<String>,
    #[serde(rename = "forked_from_session_id")]
    forked_from_session_id_snake: Option<String>,
    #[serde(flatten)]
    extra: HashMap<String, Value>,
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
    model: Option<String>,
    token_count: Option<usize>,
    context_used_tokens: Option<usize>,
    previous_context_used_tokens: Option<usize>,
    compact_markers: usize,
    derived_compact_markers: usize,
    handoff_markers: usize,
    event_count: usize,
    malformed_lines: usize,
    summary_truncated: bool,
    latest_ai_outcome_error: bool,
    metadata: ClaudeMetadataSummary,
}

#[derive(Debug, Default)]
struct ClaudeMetadataSummary {
    has_sdk_init: bool,
    has_result: bool,
    total_cost_usd: Option<f64>,
    duration_ms: Option<u64>,
    duration_api_ms: Option<u64>,
    num_turns: Option<u64>,
    hook_event_count: usize,
    partial_event_count: usize,
    remote_surface_count: usize,
    fork_parent_session: Option<String>,
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
        let mut summary = self.parse_summary_limited(path, self.summary_line_limit)?;
        if self.summary_line_limit.is_some() {
            let full_summary = self.parse_summary_limited(path, None)?;
            summary.token_count = full_summary.token_count;
            summary.context_health = full_summary.context_health;
        }
        Ok(summary)
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
                        metadata: TimelineEventMetadata {
                            raw_refs: vec![TimelineEventRawRef {
                                source_cli: Some(CLAUDE_TOOL),
                                source_session: Some(session_id.into()),
                                source_path: Some(path.display().to_string()),
                                line_number: Some(line_index + 1),
                                record_type: Some("malformed".into()),
                                digest: Some(stable_text_digest(&line)),
                                ..TimelineEventRawRef::default()
                            }],
                            ..TimelineEventMetadata::default()
                        },
                    };
                    if push_timeline_event(&mut events, event, event_limit) {
                        break;
                    }
                    continue;
                }
            };

            if let Some(event) =
                timeline_event(record, events.len() + 1, session_id, path, line_index + 1)
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
                    fidelity: Some(claude_local_fidelity(true)),
                    capabilities: None,
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
                fidelity: Some(claude_local_fidelity(false)),
                capabilities: None,
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
    if title.is_empty()
        || title.starts_with('/')
        || is_provider_context_text(title)
        || is_moonbox_handoff_control_text(title)
        || is_claude_internal_event_text(title)
    {
        None
    } else {
        Some(truncate(title, 160))
    }
}

impl ClaudeRecord {
    fn record_type(&self) -> &str {
        self.record_type.as_deref().unwrap_or_default()
    }

    fn subtype(&self) -> &str {
        self.subtype.as_deref().unwrap_or_default()
    }

    fn session_id(&self) -> Option<&str> {
        self.session_id
            .as_deref()
            .or(self.session_id_snake.as_deref())
    }

    fn fork_parent_session(&self) -> Option<&str> {
        self.parent_session_id_camel
            .as_deref()
            .or(self.parent_session_id_snake.as_deref())
            .or(self.forked_from_session_id_camel.as_deref())
            .or(self.forked_from_session_id_snake.as_deref())
    }

    fn string_extra(&self, keys: &[&str]) -> Option<String> {
        keys.iter()
            .filter_map(|key| self.extra.get(*key))
            .find_map(text_from_value)
    }

    fn count_extra_items(&self, keys: &[&str]) -> Option<usize> {
        keys.iter()
            .filter_map(|key| self.extra.get(*key))
            .find_map(|value| {
                if let Some(items) = value.as_array() {
                    return Some(items.len());
                }
                if let Some(object) = value.as_object() {
                    return Some(object.len());
                }
                None
            })
    }
}

impl ClaudeMetadataSummary {
    fn observe(&mut self, record: &ClaudeRecord) {
        self.has_sdk_init |= is_sdk_init_record(record);
        self.has_result |= is_result_record(record);
        self.total_cost_usd = record.total_cost_usd.or(self.total_cost_usd);
        self.duration_ms = record.duration_ms.or(self.duration_ms);
        self.duration_api_ms = record.duration_api_ms.or(self.duration_api_ms);
        self.num_turns = record.num_turns.or(self.num_turns);
        if is_hook_event_record(record) {
            self.hook_event_count += 1;
        }
        if is_partial_event_record(record) {
            self.partial_event_count += 1;
        }
        if is_remote_surface_record(record) {
            self.remote_surface_count += 1;
        }
        if self.fork_parent_session.is_none()
            && let Some(parent) = record.fork_parent_session()
            && !parent.trim().is_empty()
        {
            self.fork_parent_session = Some(parent.to_owned());
        }
    }

    fn health_note(&self) -> Option<String> {
        let mut notes = Vec::new();
        if self.has_sdk_init || self.has_result {
            notes.push("Claude stream-json/SDK metadata parsed".to_owned());
        }
        let mut result_parts = Vec::new();
        if let Some(cost) = self.total_cost_usd {
            result_parts.push(format!("cost_usd={cost:.6}"));
        }
        if let Some(duration_ms) = self.duration_ms {
            result_parts.push(format!("duration_ms={duration_ms}"));
        }
        if let Some(duration_api_ms) = self.duration_api_ms {
            result_parts.push(format!("duration_api_ms={duration_api_ms}"));
        }
        if let Some(num_turns) = self.num_turns {
            result_parts.push(format!("turns={num_turns}"));
        }
        if !result_parts.is_empty() {
            notes.push(format!("result {}", result_parts.join(", ")));
        }
        if self.hook_event_count > 0 {
            notes.push(format!("hook_events={}", self.hook_event_count));
        }
        if self.partial_event_count > 0 {
            notes.push(format!("partial_events={}", self.partial_event_count));
        }
        if let Some(parent) = self.fork_parent_session.as_deref() {
            notes.push(format!("forked_from={parent}"));
        }
        if self.remote_surface_count > 0 {
            notes.push(format!(
                "remote_surface_records={} kept separate from local resume rows",
                self.remote_surface_count
            ));
        }
        if notes.is_empty() {
            None
        } else {
            Some(notes.join("; "))
        }
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
            model: None,
            token_count: None,
            context_used_tokens: None,
            previous_context_used_tokens: None,
            compact_markers: 0,
            derived_compact_markers: 0,
            handoff_markers: 0,
            event_count: 0,
            malformed_lines: 0,
            summary_truncated: false,
            latest_ai_outcome_error: false,
            metadata: ClaudeMetadataSummary::default(),
        }
    }

    fn observe(&mut self, record: ClaudeRecord) {
        self.event_count += 1;
        if let Some(session_id) = record.session_id() {
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
        if self.model.is_none()
            && let Some(model) = claude_record_model(&record)
        {
            self.model = Some(model);
        }
        if let Some(title) = record.ai_title.as_deref().and_then(normalized_text) {
            self.title = Some(truncate(&title, 160));
        }

        let record_type = record.record_type();
        if is_ai_outcome_record(&record) {
            self.latest_ai_outcome_error = record_is_error(&record);
        }
        if claude_record_has_compact_marker(record_type, &record) {
            self.compact_markers += 1;
        }
        if claude_record_has_handoff_marker(&record) {
            self.handoff_markers += 1;
        }
        if self.title.is_none()
            && record_type == "user"
            && !has_tool_result(&record)
            && let Some(text) = message_text(&record)
            && !is_provider_context_text(&text)
            && !is_moonbox_handoff_control_text(&text)
            && !is_claude_internal_event_text(&text)
        {
            self.title = Some(truncate(&text, 160));
        }
        if record_type == "assistant"
            && let Some(count) = usage_token_count(&record.message)
        {
            self.token_count = Some(self.token_count.unwrap_or(0).max(count));
            if count > 0 {
                if context_usage_drop_indicates_compact(self.previous_context_used_tokens, count) {
                    self.compact_markers += 1;
                    self.derived_compact_markers += 1;
                }
                self.previous_context_used_tokens = Some(count);
                self.context_used_tokens = Some(count);
            }
        }
        self.metadata.observe(&record);
    }

    fn finish(self) -> SessionSummary {
        let id = self.id.unwrap_or_else(|| id_from_path(&self.path));
        let updated_at = self
            .updated_at
            .unwrap_or_else(|| "1970-01-01T00:00:00+00:00".into());
        let status = if self.latest_ai_outcome_error {
            SessionStatus::Failed
        } else if self.malformed_lines > 0 {
            SessionStatus::Warning
        } else {
            SessionStatus::Healthy
        };
        let mut health_reason = if self.summary_truncated && self.malformed_lines > 0 {
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
        if let Some(metadata_note) = self.metadata.health_note() {
            health_reason.push_str("; ");
            health_reason.push_str(&metadata_note);
        }

        SessionSummary {
            id: id.clone(),
            cli: CLAUDE_TOOL,
            title: self
                .title
                .unwrap_or_else(|| format!("Claude session {}", short_id(&id))),
            cwd: self.cwd.unwrap_or_else(|| "~".into()),
            updated: human_timestamp(&updated_at),
            updated_at,
            runtime_status: SessionRuntimeStatus::Unknown,
            runtime_reason: Some(unknown_runtime_reason(CLAUDE_TOOL)),
            status,
            branch: self.branch,
            token_count: self.token_count,
            health_reason: Some(health_reason),
            event_count: self.event_count,
            resume_command: format!("claude --resume {id}"),
            source_provenance: SourceProvenance::Real,
            source_path: Some(self.path.display().to_string()),
            source_size_bytes: source_size_bytes(&self.path),
            parse_skip_count: self.malformed_lines,
            provider_metadata: None,
            context_health: claude_context_health(
                self.context_used_tokens.or(self.token_count),
                self.model.as_deref(),
                self.compact_markers,
                self.derived_compact_markers,
                self.handoff_markers,
            ),
            anatomy: None,
        }
    }
}

fn claude_context_health(
    used_tokens: Option<usize>,
    model: Option<&str>,
    compact_markers: usize,
    derived_compact_markers: usize,
    handoff_markers: usize,
) -> Option<ContextHealth> {
    let used_tokens = used_tokens.filter(|tokens| *tokens > 0);
    let window_resolution =
        model.and_then(|model| resolve_model_context_window(CLAUDE_TOOL, model));
    let window_tokens = window_resolution
        .as_ref()
        .map(|resolution| resolution.window_tokens);
    let quality_cliff_tokens = window_resolution
        .as_ref()
        .and_then(|resolution| resolution.quality_cliff_tokens);
    let mut source = match (
        model.filter(|model| !model.trim().is_empty()),
        window_resolution,
    ) {
        (Some(model), Some(resolution)) => {
            format!("claude usage event · model {model} · {}", resolution.source)
        }
        (Some(model), None) => format!("claude usage event · model {model}"),
        (None, _) => "claude usage event".into(),
    };
    if derived_compact_markers > 0 {
        source.push_str(&format!(
            " · usage drop compact inference {derived_compact_markers}"
        ));
    }
    (used_tokens.is_some() || compact_markers > 0 || handoff_markers > 0).then(|| ContextHealth {
        used_tokens,
        window_tokens,
        quality_cliff_tokens,
        compact_layers: compact_markers,
        handoff_markers,
        confidence: if used_tokens.is_some() {
            EvidenceConfidence::Derived
        } else {
            EvidenceConfidence::Estimated
        },
        source,
    })
}

fn context_usage_drop_indicates_compact(previous: Option<usize>, current: usize) -> bool {
    let Some(previous) = previous else {
        return false;
    };
    current > 0
        && previous > current.saturating_add(10_000)
        && current.saturating_mul(100) < previous.saturating_mul(80)
}

fn claude_record_model(record: &ClaudeRecord) -> Option<String> {
    [
        record.string_extra(&["model", "model_name", "modelName"]),
        string_field(&record.message, &["model", "model_name", "modelName"]),
    ]
    .into_iter()
    .flatten()
    .find(|model| is_real_model_name(model))
}

fn string_field(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .filter_map(|key| value.get(*key))
        .find_map(text_from_value)
}

fn is_real_model_name(model: &str) -> bool {
    let model = model.trim();
    !model.is_empty() && !model.eq_ignore_ascii_case("<synthetic>") && model != "synthetic"
}

fn claude_record_has_compact_marker(record_type: &str, record: &ClaudeRecord) -> bool {
    record_type == "summary"
        || record_type.contains("compact")
        || record
            .subtype
            .as_deref()
            .is_some_and(|subtype| subtype.contains("compact"))
}

fn claude_record_has_handoff_marker(record: &ClaudeRecord) -> bool {
    claude_record_text(record).is_some_and(|text| {
        [
            "moonbox-continuation-handoff",
            "Handoff skill",
            "Target CLI",
            "handoff artifact",
            "交接文档",
        ]
        .iter()
        .any(|needle| text.contains(needle))
    })
}

fn claude_record_text(record: &ClaudeRecord) -> Option<String> {
    serde_json::to_string(&[
        &record.message,
        &record.result,
        &record.error,
        &record.tool_use_result,
        &record.last_prompt,
    ])
    .ok()
}

fn source_size_bytes(path: &Path) -> Option<u64> {
    fs::metadata(path).ok().map(|metadata| metadata.len())
}

fn timeline_event(
    record: ClaudeRecord,
    number: usize,
    session_id: &str,
    path: &Path,
    line_number: usize,
) -> Option<TimelineEvent> {
    let record_type = record.record_type();
    let kind = timeline_kind(record_type, &record)?;
    let image_markup = extract_timeline_image_markup(&timeline_detail(record_type, &record));
    let detail = image_markup.text;
    if detail.is_empty()
        && image_markup.attachments.is_empty()
        && !matches!(kind, TimelineKind::Error)
    {
        return None;
    }
    if kind == TimelineKind::User
        && (is_provider_context_text(&detail) || is_moonbox_handoff_control_text(&detail))
    {
        return None;
    }
    let mut metadata = timeline_metadata(&record, session_id, path, line_number, kind);
    metadata.attachments.extend(image_markup.attachments);

    Some(TimelineEvent {
        id: event_id(number),
        time: display_time(record.timestamp.as_deref()),
        kind,
        title: timeline_title(record_type, &record),
        detail,
        metadata,
    })
}

fn timeline_metadata(
    record: &ClaudeRecord,
    session_id: &str,
    path: &Path,
    line_number: usize,
    kind: TimelineKind,
) -> TimelineEventMetadata {
    let message_ids = id_fields(
        &record.message,
        &["id", "message_id", "messageId", "uuid", "leafUuid"],
    );
    let mut provider_item_ids = message_ids.clone();
    if let Some(leaf_uuid) = record
        .leaf_uuid
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        push_unique(&mut provider_item_ids, leaf_uuid.to_owned());
    }

    TimelineEventMetadata {
        raw_refs: vec![TimelineEventRawRef {
            source_cli: Some(CLAUDE_TOOL),
            source_session: record
                .session_id()
                .map(str::to_owned)
                .or(Some(session_id.into())),
            source_path: Some(path.display().to_string()),
            line_number: Some(line_number),
            record_type: Some(record.record_type().into()),
            provider_kind: non_empty(record.subtype()),
            role: message_role(record),
            digest: Some(stable_value_digest(&record.message)),
            ..TimelineEventRawRef::default()
        }],
        message_ids,
        provider_item_ids,
        tool_calls: tool_calls_from_claude_record(record),
        tool_results: tool_results_from_claude_record(record),
        attachments: attachments_from_claude_record(record),
        file_changes: file_changes_from_claude_record(record, kind),
        runtime: runtime_from_claude_record(record),
        system_prompt_snapshot: system_prompt_snapshot(record),
        config_snapshot: config_snapshot(record),
        token_usage: token_usage_from_claude_record(record),
        cost: cost_from_claude_record(record),
        ..TimelineEventMetadata::default()
    }
}

fn timeline_kind(record_type: &str, record: &ClaudeRecord) -> Option<TimelineKind> {
    if record_is_error(record) {
        return Some(TimelineKind::Error);
    }
    if is_result_record(record)
        || is_sdk_init_record(record)
        || is_hook_event_record(record)
        || is_partial_event_record(record)
        || is_remote_surface_record(record)
    {
        return Some(TimelineKind::Tool);
    }
    if record_type == "summary" || record_type.contains("compact") {
        return Some(TimelineKind::Compact);
    }
    match record_type {
        "user" if has_tool_result(record) => Some(TimelineKind::Tool),
        "user" if record_has_claude_internal_message(record) => Some(TimelineKind::Tool),
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
    if is_result_record(record) {
        return if record_is_error(record) {
            "SDK result error".into()
        } else {
            "SDK result".into()
        };
    }
    if is_sdk_init_record(record) {
        return "SDK init".into();
    }
    if is_hook_event_record(record) {
        return "Hook event".into();
    }
    if is_partial_event_record(record) {
        return "Partial stream event".into();
    }
    if is_remote_surface_record(record) {
        return "Remote surface".into();
    }
    match record_type {
        "user" if has_tool_result(record) => "Tool result".into(),
        "user" if record_has_claude_internal_message(record) => "Internal event".into(),
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
    if is_result_record(record) {
        return result_detail(record);
    }
    if is_sdk_init_record(record) {
        return sdk_init_detail(record);
    }
    if is_hook_event_record(record) {
        return hook_event_detail(record);
    }
    if is_partial_event_record(record) {
        return partial_event_detail(record);
    }
    if is_remote_surface_record(record) {
        return remote_surface_detail(record);
    }
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
            .map(|text| truncate_timeline_detail(&text))
            .unwrap_or_else(|| "tool result".into()),
        _ => message_text(record)
            .map(|text| truncate_timeline_detail(&text))
            .or_else(|| record.cwd.as_deref().map(|cwd| format!("cwd: {cwd}")))
            .unwrap_or_default(),
    }
}

fn message_text(record: &ClaudeRecord) -> Option<String> {
    text_from_value(record.message.get("content").unwrap_or(&Value::Null))
}

fn non_empty(value: &str) -> Option<String> {
    (!value.trim().is_empty()).then_some(value.to_owned())
}

fn push_unique(values: &mut Vec<String>, value: String) {
    if !values.contains(&value) {
        values.push(value);
    }
}

fn id_fields(value: &Value, keys: &[&str]) -> Vec<String> {
    keys.iter()
        .filter_map(|key| value.get(*key).and_then(Value::as_str))
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned)
        .fold(Vec::new(), |mut values, value| {
            push_unique(&mut values, value);
            values
        })
}

fn clone_non_null(value: Option<&Value>) -> Option<Value> {
    value.filter(|value| !value.is_null()).cloned()
}

fn message_role(record: &ClaudeRecord) -> Option<String> {
    if let Some(role) = record
        .message
        .get("role")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
    {
        return Some(role.to_owned());
    }
    non_empty(record.record_type())
}

fn tool_calls_from_claude_record(record: &ClaudeRecord) -> Vec<TimelineToolCall> {
    record
        .message
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|item| item.get("type").and_then(Value::as_str) == Some("tool_use"))
        .map(|item| TimelineToolCall {
            id: item
                .get("id")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .map(str::to_owned),
            name: item
                .get("name")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .map(str::to_owned),
            arguments: clone_non_null(item.get("input")),
            raw: Some(item.clone()),
        })
        .collect()
}

fn tool_results_from_claude_record(record: &ClaudeRecord) -> Vec<TimelineToolResult> {
    let mut results = record
        .message
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|item| item.get("type").and_then(Value::as_str) == Some("tool_result"))
        .map(|item| TimelineToolResult {
            call_id: item
                .get("tool_use_id")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .map(str::to_owned),
            name: None,
            content: text_from_value(item.get("content").unwrap_or(&Value::Null))
                .map(|text| truncate_timeline_detail(&text)),
            is_error: item.get("is_error").and_then(Value::as_bool),
            raw: Some(item.clone()),
        })
        .collect::<Vec<_>>();
    if !record.tool_use_result.is_null() {
        results.push(TimelineToolResult {
            content: text_from_value(&record.tool_use_result)
                .map(|text| truncate_timeline_detail(&text)),
            is_error: record.is_error,
            raw: Some(record.tool_use_result.clone()),
            ..TimelineToolResult::default()
        });
    }
    results
}

fn attachments_from_claude_record(record: &ClaudeRecord) -> Vec<TimelineAttachment> {
    if record.record_type() != "attachment" {
        return Vec::new();
    }
    vec![TimelineAttachment {
        id: record
            .string_extra(&["id", "attachment_id", "attachmentId"])
            .or_else(|| record.leaf_uuid.clone()),
        name: record.string_extra(&["name", "filename", "fileName"]),
        path: record.string_extra(&["path", "file_path", "filePath"]),
        mime_type: record.string_extra(&["mime_type", "mimeType"]),
        size_bytes: record
            .extra
            .get("size_bytes")
            .or_else(|| record.extra.get("sizeBytes"))
            .and_then(Value::as_u64),
        raw: Some(record.message.clone()),
    }]
}

fn file_changes_from_claude_record(
    record: &ClaudeRecord,
    kind: TimelineKind,
) -> Vec<TimelineFileChange> {
    if kind != TimelineKind::GitDiff {
        return Vec::new();
    }
    vec![TimelineFileChange {
        path: record.string_extra(&["path", "file_path", "filePath"]),
        operation: record.string_extra(&["operation", "op", "change_type", "changeType"]),
        summary: message_text(record).map(|text| truncate_timeline_detail(&text)),
        diff: message_text(record),
        raw: Some(record.message.clone()),
    }]
}

fn runtime_from_claude_record(record: &ClaudeRecord) -> Option<TimelineRuntimeMetadata> {
    if record.duration_ms.is_none()
        && record.duration_api_ms.is_none()
        && record.num_turns.is_none()
    {
        return None;
    }
    Some(TimelineRuntimeMetadata {
        status: if is_result_record(record) && !record_is_error(record) {
            SessionRuntimeStatus::Inactive
        } else {
            SessionRuntimeStatus::Unknown
        },
        reason: is_result_record(record).then_some("Claude stream-json result".into()),
        duration_ms: record.duration_ms,
        api_duration_ms: record.duration_api_ms,
        turn_count: record.num_turns,
    })
}

fn system_prompt_snapshot(record: &ClaudeRecord) -> Option<String> {
    record.string_extra(&[
        "system_prompt",
        "systemPrompt",
        "system_prompt_snapshot",
        "systemPromptSnapshot",
    ])
}

fn config_snapshot(record: &ClaudeRecord) -> Option<Value> {
    clone_non_null(
        record
            .extra
            .get("model_config")
            .or_else(|| record.extra.get("modelConfig"))
            .or_else(|| record.extra.get("config"))
            .or_else(|| record.extra.get("tools"))
            .or_else(|| record.extra.get("mcp_servers"))
            .or_else(|| record.extra.get("mcpServers")),
    )
}

fn token_usage_from_claude_record(record: &ClaudeRecord) -> Option<TokenBreakdown> {
    usage_token_count(&record.message).map(|total| TokenBreakdown {
        total,
        ..TokenBreakdown::default()
    })
}

fn cost_from_claude_record(record: &ClaudeRecord) -> Option<TimelineCostMetadata> {
    record
        .total_cost_usd
        .map(|total_cost_usd| TimelineCostMetadata {
            total_cost_usd: Some(total_cost_usd),
            currency: Some("USD".into()),
            billing_source: Some("claude_stream_json_result".into()),
        })
}

fn record_is_error(record: &ClaudeRecord) -> bool {
    record.is_error.unwrap_or(false)
        || record.record_type().contains("error")
        || record.subtype().contains("error")
        || !record.error.is_null()
}

fn is_ai_outcome_record(record: &ClaudeRecord) -> bool {
    record.record_type() == "assistant" || is_result_record(record)
}

fn is_result_record(record: &ClaudeRecord) -> bool {
    record.record_type() == "result"
        || record
            .string_extra(&["event_type", "eventType"])
            .is_some_and(|event| event == "result")
}

fn is_sdk_init_record(record: &ClaudeRecord) -> bool {
    (record.record_type() == "system" && record.subtype() == "init")
        || record.record_type() == "init"
        || record.record_type() == "system_init"
}

fn is_hook_event_record(record: &ClaudeRecord) -> bool {
    contains_surface_name(record.record_type(), "hook")
        || contains_surface_name(record.subtype(), "hook")
        || record.extra.contains_key("hook_event_name")
        || record.extra.contains_key("hookEventName")
        || record
            .string_extra(&[
                "event_type",
                "eventType",
                "hook_event_name",
                "hookEventName",
            ])
            .is_some_and(|value| contains_surface_name(&value, "hook"))
}

fn is_partial_event_record(record: &ClaudeRecord) -> bool {
    contains_surface_name(record.record_type(), "partial")
        || contains_surface_name(record.subtype(), "partial")
        || value_has_type(&record.message, "text_delta")
        || value_has_type(&record.message, "input_json_delta")
        || value_has_type(&record.message, "content_block_delta")
}

fn is_remote_surface_record(record: &ClaudeRecord) -> bool {
    contains_surface_name(record.record_type(), "remote")
        || contains_surface_name(record.subtype(), "remote")
        || record
            .string_extra(&["surface", "source_surface", "sourceSurface"])
            .is_some_and(|value| contains_surface_name(&value, "remote"))
}

fn contains_surface_name(value: &str, name: &str) -> bool {
    if value.to_ascii_lowercase().contains(name) {
        return true;
    }
    value
        .split(|character: char| !character.is_ascii_alphanumeric())
        .any(|part| part.eq_ignore_ascii_case(name))
}

fn result_detail(record: &ClaudeRecord) -> String {
    let mut parts = Vec::new();
    if !record.subtype().is_empty() {
        parts.push(format!("subtype: {}", record.subtype()));
    }
    if let Some(session_id) = record.session_id() {
        parts.push(format!("session_id: {session_id}"));
    }
    if let Some(parent) = record.fork_parent_session() {
        parts.push(format!("forked_from: {parent}"));
    }
    if let Some(cost) = record.total_cost_usd {
        parts.push(format!("cost_usd: {cost:.6}"));
    }
    if let Some(duration_ms) = record.duration_ms {
        parts.push(format!("duration_ms: {duration_ms}"));
    }
    if let Some(duration_api_ms) = record.duration_api_ms {
        parts.push(format!("duration_api_ms: {duration_api_ms}"));
    }
    if let Some(num_turns) = record.num_turns {
        parts.push(format!("turns: {num_turns}"));
    }
    if let Some(text) = text_from_value(&record.result)
        .or_else(|| text_from_value(&record.error))
        .or_else(|| message_text(record))
    {
        parts.push(format!("result: {}", truncate_timeline_detail(&text)));
    }
    if parts.is_empty() {
        "Claude stream-json result metadata".into()
    } else {
        parts.join("; ")
    }
}

fn sdk_init_detail(record: &ClaudeRecord) -> String {
    let mut parts = Vec::new();
    if let Some(session_id) = record.session_id() {
        parts.push(format!("session_id: {session_id}"));
    }
    if let Some(cwd) = record.cwd.as_deref() {
        parts.push(format!("cwd: {cwd}"));
    }
    if let Some(model) = record.string_extra(&["model"]) {
        parts.push(format!("model: {model}"));
    }
    if let Some(mode) = record.permission_mode.as_deref() {
        parts.push(format!("permission_mode: {mode}"));
    }
    if let Some(source) = record.string_extra(&["apiKeySource", "api_key_source"]) {
        parts.push(format!("api_key_source: {source}"));
    }
    if let Some(tool_count) = record.count_extra_items(&["tools"]) {
        parts.push(format!("tools: {tool_count}"));
    }
    if let Some(mcp_count) = record.count_extra_items(&["mcp_servers", "mcpServers"]) {
        parts.push(format!("mcp_servers: {mcp_count}"));
    }
    if parts.is_empty() {
        "Claude SDK init metadata".into()
    } else {
        parts.join("; ")
    }
}

fn hook_event_detail(record: &ClaudeRecord) -> String {
    let mut parts = Vec::new();
    if let Some(name) = record.string_extra(&["hook_event_name", "hookEventName", "name"]) {
        parts.push(format!("name: {name}"));
    }
    if !record.subtype().is_empty() {
        parts.push(format!("subtype: {}", record.subtype()));
    }
    if let Some(text) = text_from_value(&record.message).or_else(|| text_from_value(&record.result))
    {
        parts.push(truncate_timeline_detail(&text));
    }
    if parts.is_empty() {
        "Claude hook event metadata".into()
    } else {
        parts.join("; ")
    }
}

fn partial_event_detail(record: &ClaudeRecord) -> String {
    text_from_value(&record.message)
        .map(|text| truncate_timeline_detail(&text))
        .unwrap_or_else(|| "Claude partial stream event".into())
}

fn remote_surface_detail(record: &ClaudeRecord) -> String {
    let mut parts = vec!["remote / remote-control surface record".to_owned()];
    if !record.subtype().is_empty() {
        parts.push(format!("subtype: {}", record.subtype()));
    }
    if let Some(session_id) = record.session_id() {
        parts.push(format!("session_id: {session_id}"));
    }
    parts.push("kept separate from local resume rows".into());
    parts.join("; ")
}

fn record_has_claude_internal_message(record: &ClaudeRecord) -> bool {
    message_text(record).is_some_and(|text| is_claude_internal_event_text(&text))
}

fn is_claude_internal_event_text(text: &str) -> bool {
    let trimmed = text.trim_start();
    [
        "<local-command",
        "<local-command-stdout>",
        "<local-command-stderr>",
        "<local-command-caveat>",
        "<command-name>",
    ]
    .iter()
    .any(|prefix| trimmed.starts_with(prefix))
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

fn claude_local_fidelity(history_indexed: bool) -> SourceFidelity {
    SourceFidelity {
        status: SourceFidelityStatus::Partial,
        primary_surface: if history_indexed {
            "claude_history_jsonl_index".into()
        } else {
            "claude_project_jsonl".into()
        },
        fallback_surface: None,
        detail: "local Claude transcript JSONL may include stream-json/SDK metadata; cloud and remote-control surfaces are not probed".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::super::model::SourceCapabilityStatus;
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
        let context_health = sessions[0].context_health.as_ref().expect("context health");
        assert_eq!(context_health.used_tokens, Some(35));
        assert_eq!(context_health.window_tokens, None);
        assert_eq!(context_health.confidence, EvidenceConfidence::Derived);
        assert_eq!(sessions[0].resume_command, "claude --resume claude-real-1");
        assert_eq!(
            sessions[0].source_size_bytes,
            Some(
                fs::metadata(root.join("projects/repo/claude-real-1.jsonl"))
                    .expect("session metadata")
                    .len()
            )
        );
    }

    #[test]
    fn derives_claude_context_window_from_real_model_name() {
        let root = test_root("model-window");
        write_session(
            &root,
            "repo/claude-window.jsonl",
            r#"{"type":"assistant","sessionId":"claude-window","timestamp":"2026-05-19T07:56:10.994Z","cwd":"/repo","message":{"role":"assistant","model":"claude-sonnet-4-20250514","content":[{"type":"text","text":"Done"}],"usage":{"input_tokens":40000,"cache_read_input_tokens":50000,"output_tokens":3000}}}
"#,
        );

        let sessions = ClaudeSourceAdapter::new(&root)
            .list_sessions()
            .expect("sessions");

        let context_health = sessions[0].context_health.as_ref().expect("context health");
        assert_eq!(context_health.used_tokens, Some(93_000));
        assert_eq!(context_health.window_tokens, Some(200_000));
        assert!(context_health.source.contains("claude-sonnet-4-20250514"));
    }

    #[test]
    fn derives_claude_context_from_modern_message_usage_shape() {
        let root = test_root("modern-usage-shape");
        write_session(
            &root,
            "repo/claude-modern-usage.jsonl",
            r#"{"type":"assistant","sessionId":"claude-modern-usage","timestamp":"2026-05-19T07:56:10.994Z","cwd":"/repo","message":{"role":"assistant","model":"gpt-5.5-2026-04-24","content":[{"type":"text","text":"Done"}],"usage":{"input_tokens":21002,"cache_creation_input_tokens":0,"cache_read_input_tokens":0,"output_tokens":750,"server_tool_use":{"web_search_requests":0,"web_fetch_requests":0},"service_tier":"standard","cache_creation":{"ephemeral_1h_input_tokens":0,"ephemeral_5m_input_tokens":0},"inference_geo":"","iterations":[],"speed":"standard"}}}
"#,
        );

        let sessions = ClaudeSourceAdapter::new(&root)
            .list_sessions()
            .expect("sessions");

        let context_health = sessions[0].context_health.as_ref().expect("context health");
        assert_eq!(context_health.used_tokens, Some(21_752));
        assert_eq!(context_health.window_tokens, Some(1_000_000));
    }

    #[test]
    fn ignores_synthetic_zero_usage_after_real_claude_usage() {
        let root = test_root("synthetic-zero-usage");
        write_session(
            &root,
            "repo/claude-synthetic-zero.jsonl",
            r#"{"type":"assistant","sessionId":"claude-synthetic-zero","timestamp":"2026-05-19T07:56:10.994Z","cwd":"/repo","message":{"role":"assistant","model":"gpt-5.5-2026-04-24","content":[{"type":"text","text":"Done"}],"usage":{"input_tokens":21002,"cache_creation_input_tokens":0,"cache_read_input_tokens":0,"output_tokens":750}}}
{"type":"assistant","sessionId":"claude-synthetic-zero","timestamp":"2026-05-19T08:56:10.994Z","cwd":"/repo","error":"rate_limit","message":{"role":"assistant","model":"<synthetic>","content":[{"type":"text","text":"API Error"}],"usage":{"input_tokens":0,"cache_creation_input_tokens":0,"cache_read_input_tokens":0,"output_tokens":0}}}
"#,
        );

        let sessions = ClaudeSourceAdapter::new(&root)
            .list_sessions()
            .expect("sessions");

        let context_health = sessions[0].context_health.as_ref().expect("context health");
        assert_eq!(context_health.used_tokens, Some(21_752));
        assert_eq!(context_health.window_tokens, Some(1_000_000));
        assert_eq!(context_health.compact_layers, 0);
    }

    #[test]
    fn infers_claude_compact_from_context_usage_drop() {
        let root = test_root("compact-usage-drop");
        write_session(
            &root,
            "repo/claude-compact-drop.jsonl",
            r#"{"type":"assistant","sessionId":"claude-compact-drop","timestamp":"2026-05-19T07:56:10.994Z","cwd":"/repo","message":{"role":"assistant","model":"gpt-5.5-2026-04-24","content":[{"type":"text","text":"Before compact"}],"usage":{"input_tokens":100000,"cache_read_input_tokens":10000,"output_tokens":6245}}}
{"type":"assistant","sessionId":"claude-compact-drop","timestamp":"2026-05-19T08:56:10.994Z","cwd":"/repo","message":{"role":"assistant","model":"gpt-5.5-2026-04-24","content":[{"type":"text","text":"After compact"}],"usage":{"input_tokens":50000,"cache_read_input_tokens":5000,"output_tokens":5675}}}
"#,
        );

        let sessions = ClaudeSourceAdapter::new(&root)
            .list_sessions()
            .expect("sessions");

        let context_health = sessions[0].context_health.as_ref().expect("context health");
        assert_eq!(context_health.used_tokens, Some(60_675));
        assert_eq!(context_health.window_tokens, Some(1_000_000));
        assert_eq!(context_health.quality_cliff_tokens, Some(500_000));
        assert_eq!(context_health.compact_layers, 1);
        assert_eq!(context_health.confidence, EvidenceConfidence::Derived);
    }

    #[test]
    fn keeps_unknown_window_for_unresolved_model_name() {
        let root = test_root("unknown-model-window");
        write_session(
            &root,
            "repo/claude-unknown-window.jsonl",
            r#"{"type":"assistant","sessionId":"claude-unknown-window","timestamp":"2026-05-19T07:56:10.994Z","cwd":"/repo","message":{"role":"assistant","model":"private-experimental-model","content":[{"type":"text","text":"Done"}],"usage":{"input_tokens":90000,"output_tokens":3000}}}
"#,
        );

        let sessions = ClaudeSourceAdapter::new(&root)
            .list_sessions()
            .expect("sessions");

        let context_health = sessions[0].context_health.as_ref().expect("context health");
        assert_eq!(context_health.used_tokens, Some(93_000));
        assert_eq!(context_health.window_tokens, None);
        assert_eq!(context_health.quality_cliff_tokens, None);
        assert!(context_health.source.contains("private-experimental-model"));
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
    fn history_index_ignores_claude_internal_display_titles() {
        let root = test_root("history-index-internal-display");
        write_session(
            &root,
            "-repo-internal/session-internal.jsonl",
            r#"{"type":"user","sessionId":"session-internal","timestamp":"2026-06-01T09:00:00.000Z","cwd":"/repo/internal","message":{"content":"<local-command-caveat>Claude Code may run local commands</local-command-caveat>"}}
"#,
        );
        write_history(
            &root,
            r#"{"display":"<command-name>git status</command-name>","timestamp":1780300000000,"project":"/repo/internal","sessionId":"session-internal"}
"#,
        );

        let sessions = ClaudeSourceAdapter::new(&root)
            .list_sessions()
            .expect("sessions");

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, "session-internal");
        assert_eq!(sessions[0].title, "Claude session session-");
    }

    #[test]
    fn loads_canonical_timeline_from_claude_jsonl_store() {
        let root = test_root("timeline");
        write_session(
            &root,
            "repo/claude-real-2.jsonl",
            r#"{"type":"user","sessionId":"claude-real-2","timestamp":"2026-05-19T07:55:10.994Z","cwd":"/repo","message":{"role":"user","content":"Start here"}}
{"type":"assistant","sessionId":"claude-real-2","timestamp":"2026-05-19T07:55:20.994Z","cwd":"/repo","message":{"role":"assistant","content":[{"type":"text","text":"Working"}]}}
{"type":"assistant","sessionId":"claude-real-2","timestamp":"2026-05-19T07:55:30.994Z","cwd":"/repo","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu-1","name":"Read","input":{"file_path":"src/main.rs"}}]}}
{"type":"user","sessionId":"claude-real-2","timestamp":"2026-05-19T07:55:40.994Z","cwd":"/repo","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu-1","content":"ok"}]},"toolUseResult":{"stdout":"ok"}}
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
        assert_eq!(timeline.events[2].metadata.tool_calls.len(), 1);
        assert_eq!(
            timeline.events[2].metadata.tool_calls[0].name.as_deref(),
            Some("Read")
        );
        assert_eq!(
            timeline.events[2].metadata.tool_calls[0]
                .arguments
                .as_ref()
                .expect("tool args")["file_path"],
            "src/main.rs"
        );
        assert_eq!(
            timeline.events[3].metadata.tool_results[0]
                .call_id
                .as_deref(),
            Some("toolu-1")
        );
    }

    #[test]
    fn local_command_tags_are_internal_tool_events_not_user_anchors() {
        let root = test_root("timeline-local-command");
        write_session(
            &root,
            "repo/claude-local-command.jsonl",
            r#"{"type":"user","sessionId":"claude-local-command","timestamp":"2026-05-19T07:55:10.994Z","cwd":"/repo","message":{"role":"user","content":"<local-command-caveat>Claude Code may run local commands</local-command-caveat>"}}
{"type":"user","sessionId":"claude-local-command","timestamp":"2026-05-19T07:55:20.994Z","cwd":"/repo","message":{"role":"user","content":"<command-name>git status</command-name>"}}
{"type":"user","sessionId":"claude-local-command","timestamp":"2026-05-19T07:55:30.994Z","cwd":"/repo","message":{"role":"user","content":"Plan the next milestone"}}
"#,
        );

        let adapter = ClaudeSourceAdapter::new(&root);
        let session = adapter
            .find_session("claude-local-command")
            .expect("find")
            .expect("session");
        let timeline = adapter
            .load_timeline("claude-local-command")
            .expect("timeline");

        assert_eq!(session.title, "Plan the next milestone");
        assert_eq!(timeline.events[0].kind, TimelineKind::Tool);
        assert_eq!(timeline.events[0].title, "Internal event");
        assert_eq!(timeline.events[1].kind, TimelineKind::Tool);
        assert_eq!(timeline.events[1].title, "Internal event");
        assert_eq!(timeline.events[2].kind, TimelineKind::User);
        assert_eq!(timeline.events[2].detail, "Plan the next milestone");
    }

    #[test]
    fn timeline_promotes_inline_image_markup_to_attachment() {
        let root = test_root("timeline-inline-image");
        write_session(
            &root,
            "repo/claude-inline-image.jsonl",
            r##"{"type":"user","sessionId":"claude-inline-image","timestamp":"2026-05-19T07:55:10.994Z","cwd":"/repo","message":{"role":"user","content":"<image name=[Image #1]> </image> [Image #1]\n看下这个问题"}}
"##,
        );

        let timeline = ClaudeSourceAdapter::new(&root)
            .load_timeline("claude-inline-image")
            .expect("timeline");
        let event = timeline
            .events
            .iter()
            .find(|event| event.kind == TimelineKind::User)
            .expect("user event");

        assert_eq!(event.detail, "看下这个问题");
        assert!(!event.detail.contains("<image"));
        assert_eq!(event.metadata.attachments.len(), 1);
        assert_eq!(
            event.metadata.attachments[0].name.as_deref(),
            Some("Image #1")
        );
    }

    #[test]
    fn parses_stream_json_sdk_metadata_when_captured_in_transcript() {
        let root = test_root("stream-json-sdk");
        write_session(
            &root,
            "repo/sdk-child.jsonl",
            r#"{"type":"system","subtype":"init","session_id":"sdk-child","cwd":"/repo","model":"claude-sonnet-4-20250514","permissionMode":"default","apiKeySource":"login","tools":["Read","Edit"],"mcp_servers":{"fs":{}}}
{"type":"user","session_id":"sdk-child","timestamp":"2026-06-08T09:00:00.000Z","cwd":"/repo","message":{"content":"Implement M63"}}
{"type":"assistant","session_id":"sdk-child","timestamp":"2026-06-08T09:01:00.000Z","cwd":"/repo","message":{"content":[{"type":"text","text":"Working"}],"usage":{"input_tokens":12,"output_tokens":5}}}
{"type":"result","subtype":"success","session_id":"sdk-child","total_cost_usd":0.00321,"duration_ms":1234,"duration_api_ms":987,"num_turns":4,"result":"done"}"#,
        );

        let adapter = ClaudeSourceAdapter::new(&root);
        let session = adapter
            .find_session("sdk-child")
            .expect("find")
            .expect("session");
        let timeline = adapter.load_timeline("sdk-child").expect("timeline");

        assert_eq!(session.id, "sdk-child");
        assert_eq!(session.title, "Implement M63");
        assert_eq!(session.token_count, Some(17));
        let health = session.health_reason.as_deref().expect("health");
        assert!(health.contains("stream-json/SDK metadata parsed"));
        assert!(health.contains("cost_usd=0.003210"));
        assert!(health.contains("duration_ms=1234"));
        assert!(health.contains("duration_api_ms=987"));
        assert!(health.contains("turns=4"));
        assert_eq!(timeline.events[0].kind, TimelineKind::Tool);
        assert_eq!(timeline.events[0].title, "SDK init");
        assert!(timeline.events[0].detail.contains("model: claude-sonnet"));
        assert!(timeline.events[0].detail.contains("tools: 2"));
        assert!(timeline.events[0].metadata.config_snapshot.is_some());
        assert_eq!(
            timeline.events[2]
                .metadata
                .token_usage
                .as_ref()
                .expect("token usage")
                .total,
            17
        );
        assert_eq!(timeline.events[3].title, "SDK result");
        assert!(timeline.events[3].detail.contains("session_id: sdk-child"));
        assert!(timeline.events[3].detail.contains("cost_usd: 0.003210"));
        assert_eq!(
            timeline.events[3]
                .metadata
                .cost
                .as_ref()
                .expect("cost")
                .total_cost_usd,
            Some(0.00321)
        );
        assert_eq!(
            timeline.events[3]
                .metadata
                .runtime
                .as_ref()
                .expect("runtime")
                .duration_ms,
            Some(1234)
        );
    }

    #[test]
    fn hook_partial_and_remote_surface_records_are_observability_events() {
        let root = test_root("stream-json-surfaces");
        write_session(
            &root,
            "repo/claude-surfaces.jsonl",
            r#"{"type":"hook","subtype":"PreToolUse","session_id":"claude-surfaces","timestamp":"2026-06-08T09:00:00.000Z","hook_event_name":"PreToolUse","message":{"content":"allow Read"}}
{"type":"assistant","subtype":"partial","session_id":"claude-surfaces","timestamp":"2026-06-08T09:00:01.000Z","message":{"content":[{"type":"text_delta","text":"partial assistant text"}]}}
{"type":"remote-control","subtype":"attach","session_id":"claude-surfaces","timestamp":"2026-06-08T09:00:02.000Z","message":{"content":"remote-control attach"}}
{"type":"user","session_id":"claude-surfaces","timestamp":"2026-06-08T09:00:03.000Z","message":{"content":"Real rewind anchor"}}"#,
        );

        let adapter = ClaudeSourceAdapter::new(&root);
        let session = adapter
            .find_session("claude-surfaces")
            .expect("find")
            .expect("session");
        let timeline = adapter.load_timeline("claude-surfaces").expect("timeline");

        assert_eq!(session.title, "Real rewind anchor");
        let health = session.health_reason.as_deref().expect("health");
        assert!(health.contains("hook_events=1"));
        assert!(health.contains("partial_events=1"));
        assert!(health.contains("remote_surface_records=1"));
        assert_eq!(timeline.events[0].kind, TimelineKind::Tool);
        assert_eq!(timeline.events[0].title, "Hook event");
        assert_eq!(timeline.events[1].kind, TimelineKind::Tool);
        assert_eq!(timeline.events[1].title, "Partial stream event");
        assert_eq!(timeline.events[2].kind, TimelineKind::Tool);
        assert_eq!(timeline.events[2].title, "Remote surface");
        assert_eq!(timeline.events[3].kind, TimelineKind::User);
        assert_eq!(timeline.events[3].detail, "Real rewind anchor");
    }

    #[test]
    fn result_errors_and_fork_metadata_are_preserved_without_changing_resume() {
        let root = test_root("stream-json-fork-error");
        write_session(
            &root,
            "repo/child-session.jsonl",
            r#"{"type":"user","sessionId":"child-session","timestamp":"2026-06-08T09:00:00.000Z","cwd":"/repo","message":{"content":"Continue from fork"}}
{"type":"result","subtype":"error","session_id":"child-session","parent_session_id":"parent-session","is_error":true,"duration_ms":222,"error":{"message":"permission denied"}}"#,
        );

        let adapter = ClaudeSourceAdapter::new(&root);
        let session = adapter
            .find_session("child-session")
            .expect("find")
            .expect("session");
        let timeline = adapter.load_timeline("child-session").expect("timeline");

        assert_eq!(session.status, SessionStatus::Failed);
        assert_eq!(session.resume_command, "claude --resume child-session");
        let health = session.health_reason.as_deref().expect("health");
        assert!(health.contains("forked_from=parent-session"));
        assert!(health.contains("duration_ms=222"));
        assert_eq!(timeline.events[1].kind, TimelineKind::Error);
        assert_eq!(timeline.events[1].title, "SDK result error");
        assert!(
            timeline.events[1]
                .detail
                .contains("forked_from: parent-session")
        );
        assert!(timeline.events[1].detail.contains("permission denied"));
    }

    #[test]
    fn historical_result_error_does_not_fail_session_after_later_ai_success() {
        let root = test_root("stream-json-recovered-error");
        write_session(
            &root,
            "repo/recovered-session.jsonl",
            r#"{"type":"user","session_id":"recovered-session","timestamp":"2026-06-08T09:00:00.000Z","cwd":"/repo","message":{"content":"retry this"}}
{"type":"result","subtype":"error","session_id":"recovered-session","timestamp":"2026-06-08T09:01:00.000Z","is_error":true,"error":{"message":"timeout"}}
{"type":"assistant","session_id":"recovered-session","timestamp":"2026-06-08T09:02:00.000Z","message":{"content":[{"type":"text","text":"Recovered after retry"}],"usage":{"input_tokens":10,"output_tokens":4}}}"#,
        );

        let adapter = ClaudeSourceAdapter::new(&root);
        let session = adapter
            .find_session("recovered-session")
            .expect("find")
            .expect("session");
        let timeline = adapter
            .load_timeline("recovered-session")
            .expect("timeline");

        assert_eq!(session.status, SessionStatus::Healthy);
        assert_eq!(timeline.events[1].kind, TimelineKind::Error);
        assert_eq!(timeline.events[2].kind, TimelineKind::Assistant);
    }

    #[test]
    fn report_capabilities_describe_claude_m63_surface_boundaries() {
        let root = test_root("m63-capabilities");
        write_session(
            &root,
            "repo/session.jsonl",
            r#"{"type":"user","sessionId":"session","timestamp":"2026-06-08T09:00:00.000Z","cwd":"/repo","message":{"content":"hello"}}"#,
        );
        let adapter = ClaudeSourceAdapter::new(&root);

        let (_sessions, report) = adapter
            .list_sessions_with_report("included_real_store", "test")
            .expect("report");

        assert_eq!(
            report.capabilities.rich_local_rpc.status,
            SourceCapabilityStatus::Available
        );
        assert_eq!(report.fidelity.status, SourceFidelityStatus::Partial);
        assert_eq!(report.fidelity.primary_surface, "claude_project_jsonl");
        assert!(report.fidelity.fallback_surface.is_none());
        assert!(report.fidelity.detail.contains("remote-control"));
        assert!(
            report
                .capabilities
                .rich_local_rpc
                .detail
                .contains("stream-json/SDK")
        );
        assert_eq!(
            report.capabilities.remote_control.status,
            SourceCapabilityStatus::Unavailable
        );
        assert!(
            report
                .capabilities
                .remote_control
                .detail
                .contains("not launched")
        );
        assert!(
            report
                .capabilities
                .fork_resume
                .detail
                .contains("fork parent metadata")
        );
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

    #[test]
    fn list_summary_keeps_context_health_beyond_summary_line_limit() {
        let root = test_root("summary-line-limit-context");
        write_session(
            &root,
            "repo/claude-limited-context.jsonl",
            r#"{"type":"user","sessionId":"claude-limited-context","timestamp":"2026-06-06T10:00:00.000Z","cwd":"/repo","message":{"content":"visible title"}}
{"type":"assistant","sessionId":"claude-limited-context","timestamp":"2026-06-06T10:01:00.000Z","cwd":"/repo","message":{"role":"assistant","model":"gpt-5.5-2026-04-24","content":[{"type":"text","text":"before"}],"usage":{"input_tokens":100000,"cache_read_input_tokens":10000,"output_tokens":6245}}}
{"type":"assistant","sessionId":"claude-limited-context","timestamp":"2026-06-06T10:02:00.000Z","cwd":"/repo","message":{"role":"assistant","model":"gpt-5.5-2026-04-24","content":[{"type":"text","text":"after"}],"usage":{"input_tokens":50000,"cache_read_input_tokens":5000,"output_tokens":5675}}}"#,
        );
        let adapter = ClaudeSourceAdapter::with_all_limits(&root, Some(10), None, Some(1));

        let sessions = adapter.list_sessions().expect("sessions");

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].title, "visible title");
        assert!(
            sessions[0]
                .health_reason
                .as_deref()
                .expect("health")
                .contains("summary preview truncated")
        );
        let context_health = sessions[0].context_health.as_ref().expect("context health");
        assert_eq!(context_health.used_tokens, Some(60_675));
        assert_eq!(context_health.window_tokens, Some(1_000_000));
        assert_eq!(context_health.compact_layers, 1);
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
