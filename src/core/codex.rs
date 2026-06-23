use std::{
    collections::{HashMap, HashSet},
    env, fs,
    io::{BufRead, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

use rusqlite::{Connection, MappedRows, OpenFlags, OptionalExtension, Row, named_params, params};
use serde::Deserialize;
use serde_json::Value;
use time::OffsetDateTime;

use super::{
    adapter::{AdapterError, SourceAdapter, SourceReportMeta, SourceScanStats},
    codex_app_server::{CodexAppServerSource, app_thread_summary},
    local_jsonl::{
        DiscoveryOrder, collect_jsonl_files, configured_session_limit,
        configured_session_scan_entry_limit, configured_session_summary_line_limit,
        discover_jsonl_files, display_time, event_id, extract_timeline_image_markup,
        find_token_count, human_timestamp, is_moonbox_handoff_control_text,
        is_provider_context_text, max_timestamp, open_reader, push_timeline_event, read_error,
        replace_time_dashes, stable_text_digest, stable_value_digest, string_field,
        text_from_value, title_case, truncate, truncate_timeline_detail,
    },
    model::{
        CanonicalTimeline, CliTool, ContextHealth, EvidenceConfidence, ProviderSessionMetadata,
        SessionRuntimeStatus, SessionStatus, SessionSummary, SourceCapabilities, SourceCapability,
        SourceCapabilityStatus, SourceFidelity, SourceFidelityStatus, SourceProvenance,
        TimelineApproval, TimelineEvent, TimelineEventMetadata, TimelineEventRawRef,
        TimelineFileChange, TimelineKind, TimelineRuntimeMetadata, TimelineToolCall,
        TimelineToolResult, TokenBreakdown, unknown_runtime_reason,
    },
};

const CODEX_TOOL: CliTool = CliTool::Codex;
const CODEX_CONTEXT_TAIL_BYTES: u64 = 2 * 1024 * 1024;
const K2_SOURCE_PREFIX: &str = "k2-session://";
pub(crate) const CODEX_APP_PROVIDER_SOURCE: &str = "codex_app";
pub(crate) const K2_PROVIDER_SOURCE: &str = "k2";
#[cfg(not(test))]
const K2_CONFIG_DIR_ENV: &str = "K2_CONFIG_DIR";
#[cfg(not(test))]
const MOONBOX_K2_HOME_ENV: &str = "MOONBOX_K2_HOME";
#[cfg(not(test))]
const MOONBOX_K2_SESSIONS_ENV: &str = "MOONBOX_K2_SESSIONS";

#[derive(Debug, Clone)]
pub struct CodexSourceAdapter {
    root: PathBuf,
    k2_root: Option<PathBuf>,
    k2_sessions_enabled: bool,
    list_limit: Option<usize>,
    scan_entry_limit: Option<usize>,
    summary_line_limit: Option<usize>,
    app_server: Option<CodexAppServerSource>,
}

#[derive(Debug, Deserialize)]
struct CodexRecord {
    timestamp: Option<String>,
    #[serde(rename = "type")]
    record_type: Option<String>,
    #[serde(default)]
    payload: Value,
}

#[derive(Debug, Deserialize)]
struct CodexSessionIndexRecord {
    id: Option<String>,
    thread_name: Option<String>,
}

#[derive(Debug)]
struct SummaryBuilder {
    path: PathBuf,
    id: Option<String>,
    title: Option<String>,
    cwd: Option<String>,
    originator: Option<String>,
    parent_session_id: Option<String>,
    updated_at: Option<String>,
    branch: Option<String>,
    token_count: Option<usize>,
    context_used_tokens: Option<usize>,
    context_window_tokens: Option<usize>,
    compact_layers: usize,
    handoff_markers: usize,
    event_count: usize,
    malformed_lines: usize,
    summary_truncated: bool,
    has_error: bool,
}

#[derive(Debug, Clone)]
struct CodexThreadRow {
    id: String,
    rollout_path: String,
    updated_at: String,
    cwd: String,
    title: String,
    preview: String,
    first_user_message: String,
    branch: Option<String>,
    token_count: usize,
    archived: bool,
}

#[derive(Debug, Default)]
struct CodexRolloutMetadata {
    provider_source: Option<&'static str>,
    originator: Option<String>,
    parent_session_id: Option<String>,
}

impl CodexSourceAdapter {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            k2_root: None,
            k2_sessions_enabled: false,
            list_limit: configured_session_limit(),
            scan_entry_limit: configured_session_scan_entry_limit(),
            summary_line_limit: configured_session_summary_line_limit(),
            app_server: None,
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
            k2_root: None,
            k2_sessions_enabled: false,
            list_limit,
            scan_entry_limit,
            summary_line_limit,
            app_server: None,
        }
    }

    #[cfg(test)]
    fn with_app_server_fixture(root: impl Into<PathBuf>, fixture_path: impl Into<PathBuf>) -> Self {
        let mut adapter = Self::with_all_limits(root, Some(200), None, None);
        adapter.app_server = Some(CodexAppServerSource::fixture(fixture_path));
        adapter
    }

    #[cfg(test)]
    fn with_k2_root(mut self, k2_root: impl Into<PathBuf>) -> Self {
        self.k2_root = Some(k2_root.into());
        self.k2_sessions_enabled = true;
        self
    }

    #[cfg(test)]
    fn with_k2_root_disabled(mut self, k2_root: impl Into<PathBuf>) -> Self {
        self.k2_root = Some(k2_root.into());
        self.k2_sessions_enabled = false;
        self
    }

    #[cfg(not(test))]
    pub fn from_default_home() -> Option<Self> {
        if let Some(path) = env::var_os("MOONBOX_CODEX_HOME") {
            return Some(
                Self::new(path)
                    .with_env_app_server()
                    .with_default_k2_store(),
            );
        }
        if let Some(path) = env::var_os("CODEX_HOME") {
            return Some(
                Self::new(path)
                    .with_env_app_server()
                    .with_default_k2_store(),
            );
        }
        env::var_os("HOME").map(|home| {
            Self::new(PathBuf::from(home).join(".codex"))
                .with_env_app_server()
                .with_default_k2_store()
        })
    }

    #[cfg(not(test))]
    fn with_env_app_server(mut self) -> Self {
        self.app_server = CodexAppServerSource::from_env();
        self
    }

    #[cfg(not(test))]
    fn with_default_k2_store(mut self) -> Self {
        self.k2_sessions_enabled = k2_sessions_enabled();
        self.k2_root = default_k2_root();
        self
    }

    #[cfg(not(test))]
    pub fn has_session_store(&self) -> bool {
        self.has_local_session_store()
            || self.app_server.is_some()
            || (self.k2_sessions_enabled && self.has_k2_session_store())
    }

    #[cfg(not(test))]
    pub(crate) fn session_store_path(&self) -> PathBuf {
        if self.state_db_path().is_file() {
            self.state_db_path()
        } else if let Some(app_server) = &self.app_server
            && let Some(path) = app_server.store_path()
        {
            PathBuf::from(path)
        } else if self.k2_sessions_enabled
            && let Some(path) = self.k2_sessions_index_path().filter(|path| path.is_file())
        {
            path
        } else {
            self.sessions_dir()
        }
    }

    fn sessions_dir(&self) -> PathBuf {
        self.root.join("sessions")
    }

    fn state_db_path(&self) -> PathBuf {
        self.root.join("state_5.sqlite")
    }

    fn session_index_path(&self) -> PathBuf {
        self.root.join("session_index.jsonl")
    }

    fn has_state_index(&self) -> bool {
        self.state_db_path().is_file()
    }

    fn has_local_session_store(&self) -> bool {
        self.state_db_path().is_file() || self.sessions_dir().is_dir()
    }

    #[cfg_attr(test, allow(dead_code))]
    fn has_k2_session_store(&self) -> bool {
        self.k2_sessions_index_path()
            .is_some_and(|path| path.is_file())
    }

    fn k2_sessions_dir(&self) -> Option<PathBuf> {
        self.k2_root
            .as_ref()
            .map(|root| root.join("chat").join("sessions"))
    }

    fn k2_sessions_index_path(&self) -> Option<PathBuf> {
        self.k2_sessions_dir().map(|dir| dir.join("index.json"))
    }

    fn local_store_path(&self) -> Option<String> {
        if self.has_local_session_store() {
            return Some(
                if self.has_state_index() {
                    self.state_db_path()
                } else {
                    self.sessions_dir()
                }
                .display()
                .to_string(),
            );
        }
        if self.k2_sessions_enabled
            && let Some(path) = self.k2_sessions_index_path().filter(|path| path.is_file())
        {
            return Some(path.display().to_string());
        }
        Some(self.sessions_dir().display().to_string())
    }

    fn open_connection(&self) -> Result<Connection, AdapterError> {
        Connection::open_with_flags(
            self.state_db_path(),
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|error| read_error(CODEX_TOOL, &self.state_db_path(), error))
    }

    fn list_thread_rows(&self, limit: Option<usize>) -> Result<Vec<CodexThreadRow>, AdapterError> {
        let db = self.open_connection()?;
        let query = format!(
            "{} {}",
            THREAD_SELECT,
            match limit {
                Some(_) =>
                    "where archived = 0 order by updated_at_ms_sort desc, id desc limit :limit",
                None => "where archived = 0 order by updated_at_ms_sort desc, id desc",
            }
        );
        let mut statement = db
            .prepare(&query)
            .map_err(|error| read_error(CODEX_TOOL, &self.state_db_path(), error))?;
        let rows = if let Some(limit) = limit {
            let limit = i64::try_from(limit).unwrap_or(i64::MAX);
            statement
                .query_map(named_params! {":limit": limit}, thread_row)
                .map_err(|error| read_error(CODEX_TOOL, &self.state_db_path(), error))?
        } else {
            statement
                .query_map([], thread_row)
                .map_err(|error| read_error(CODEX_TOOL, &self.state_db_path(), error))?
        };
        collect_rows(rows, &self.state_db_path())
    }

    fn find_thread_row(&self, session_id: &str) -> Result<Option<CodexThreadRow>, AdapterError> {
        if !self.has_state_index() {
            return Ok(None);
        }
        let db = self.open_connection()?;
        let mut statement = db
            .prepare(&format!("{THREAD_SELECT} where id = ?1"))
            .map_err(|error| read_error(CODEX_TOOL, &self.state_db_path(), error))?;
        statement
            .query_row(params![session_id], thread_row)
            .optional()
            .map_err(|error| read_error(CODEX_TOOL, &self.state_db_path(), error))
    }

    fn thread_name_overrides(&self) -> Result<HashMap<String, String>, AdapterError> {
        let path = self.session_index_path();
        if !path.is_file() {
            return Ok(HashMap::new());
        }
        let reader = open_reader(CODEX_TOOL, &path)?;
        let mut overrides = HashMap::new();
        for line in reader.lines() {
            let line = line.map_err(|error| read_error(CODEX_TOOL, &path, error))?;
            if line.trim().is_empty() {
                continue;
            }
            let Ok(record) = serde_json::from_str::<CodexSessionIndexRecord>(&line) else {
                continue;
            };
            let Some(id) = record.id.filter(|value| !value.trim().is_empty()) else {
                continue;
            };
            let Some(thread_name) = record.thread_name.filter(|value| {
                !value.trim().is_empty()
                    && !is_provider_context_text(value)
                    && !is_moonbox_handoff_control_text(value)
            }) else {
                continue;
            };
            overrides.insert(id, thread_name);
        }
        Ok(overrides)
    }

    fn summary_for_thread(&self, row: CodexThreadRow) -> SessionSummary {
        let k2_ids = self.k2_session_id_set().unwrap_or_default();
        self.summary_for_thread_with_k2_ids(row, &k2_ids)
    }

    fn summary_for_thread_with_k2_ids(
        &self,
        row: CodexThreadRow,
        k2_ids: &HashSet<String>,
    ) -> SessionSummary {
        let source_path = row.rollout_path.trim();
        let rollout_exists = !source_path.is_empty() && Path::new(source_path).is_file();
        let title = first_non_empty([&row.title, &row.preview, &row.first_user_message])
            .filter(|title| {
                !is_provider_context_text(title) && !is_moonbox_handoff_control_text(title)
            })
            .map(truncate_thread_title)
            .unwrap_or_else(|| format!("Codex session {}", short_id(&row.id)));
        let status = if !rollout_exists || row.archived {
            SessionStatus::Warning
        } else {
            SessionStatus::Healthy
        };
        let health_reason = if !rollout_exists {
            "real Codex SQLite thread index; rollout JSONL missing".into()
        } else if row.archived {
            "real Codex SQLite thread index; archived thread".into()
        } else {
            "real Codex SQLite thread index".into()
        };
        let context_health = if rollout_exists {
            context_health_from_rollout_tail(Path::new(source_path)).or_else(|| {
                context_health_from_token_count(
                    normalized_token_count(row.token_count),
                    None,
                    0,
                    0,
                    EvidenceConfidence::Derived,
                    "codex sqlite thread index",
                )
            })
        } else {
            context_health_from_token_count(
                normalized_token_count(row.token_count),
                None,
                0,
                0,
                EvidenceConfidence::Derived,
                "codex sqlite thread index",
            )
        };

        let mut summary = SessionSummary {
            id: row.id.clone(),
            cli: CODEX_TOOL,
            title,
            cwd: if row.cwd.trim().is_empty() {
                "~".into()
            } else {
                row.cwd
            },
            updated: human_timestamp(&row.updated_at),
            updated_at: row.updated_at,
            runtime_status: SessionRuntimeStatus::Unknown,
            runtime_reason: Some(unknown_runtime_reason(CODEX_TOOL)),
            status,
            branch: row.branch,
            token_count: normalized_token_count(row.token_count),
            health_reason: Some(health_reason),
            event_count: 0,
            resume_command: format!("codex resume {}", row.id),
            source_provenance: SourceProvenance::Real,
            source_path: rollout_exists.then(|| source_path.to_owned()),
            source_size_bytes: rollout_exists
                .then(|| source_size_bytes(Path::new(source_path)))
                .flatten(),
            parse_skip_count: 0,
            provider_metadata: None,
            context_health,
            anatomy: None,
        };
        annotate_codex_subsource(&mut summary, k2_ids);
        summary
    }

    fn session_files(&self, limit: Option<usize>) -> Result<Vec<PathBuf>, AdapterError> {
        let sessions_dir = self.sessions_dir();
        if !sessions_dir.exists() {
            return Ok(Vec::new());
        }

        let mut files = Vec::new();
        collect_jsonl_files(CODEX_TOOL, &sessions_dir, &mut files)?;
        files.sort_by(|left, right| right.cmp(left));
        if let Some(limit) = limit {
            files.truncate(limit);
        }
        Ok(files)
    }

    fn listed_session_files(&self) -> Result<Vec<PathBuf>, AdapterError> {
        Ok(self.listed_session_discovery()?.files)
    }

    fn listed_session_discovery(&self) -> Result<super::local_jsonl::JsonlDiscovery, AdapterError> {
        discover_jsonl_files(
            CODEX_TOOL,
            &self.sessions_dir(),
            self.list_limit,
            self.scan_entry_limit,
            DiscoveryOrder::PathDesc,
        )
    }

    fn all_session_files(&self) -> Result<Vec<PathBuf>, AdapterError> {
        self.session_files(None)
    }

    fn parse_summary(&self, path: &Path) -> Result<SessionSummary, AdapterError> {
        self.parse_summary_limited(path, None)
    }

    fn parse_summary_limited(
        &self,
        path: &Path,
        line_limit: Option<usize>,
    ) -> Result<SessionSummary, AdapterError> {
        let k2_ids = self.k2_session_id_set()?;
        self.parse_summary_limited_with_k2_ids(path, line_limit, &k2_ids)
    }

    fn parse_summary_limited_with_k2_ids(
        &self,
        path: &Path,
        line_limit: Option<usize>,
        k2_ids: &HashSet<String>,
    ) -> Result<SessionSummary, AdapterError> {
        let mut builder = SummaryBuilder::new(path);
        let reader = open_reader(CODEX_TOOL, path)?;

        for (line_index, line) in reader.lines().enumerate() {
            if let Some(limit) = line_limit
                && line_index >= limit
            {
                builder.summary_truncated = true;
                break;
            }
            let line = line.map_err(|error| read_error(CODEX_TOOL, path, error))?;
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<CodexRecord>(&line) {
                Ok(record) => builder.observe(record),
                Err(_) => builder.malformed_lines += 1,
            }
        }

        let mut summary = builder.finish();
        annotate_codex_subsource(&mut summary, k2_ids);
        Ok(summary)
    }

    fn find_session_path(&self, session_id: &str) -> Result<Option<PathBuf>, AdapterError> {
        if let Some(row) = self.find_thread_row(session_id)? {
            let path = PathBuf::from(row.rollout_path);
            if path.is_file() {
                return Ok(Some(path));
            }
        }
        for path in self.all_session_files()? {
            let summary = self.parse_summary(&path)?;
            if summary.id == session_id {
                return Ok(Some(path));
            }
        }
        Ok(None)
    }

    fn parse_timeline(
        &self,
        session_id: &str,
        path: &Path,
        event_limit: Option<usize>,
    ) -> Result<CanonicalTimeline, AdapterError> {
        let reader = open_reader(CODEX_TOOL, path)?;
        let mut events = Vec::new();

        for (line_index, line) in reader.lines().enumerate() {
            let line = line.map_err(|error| read_error(CODEX_TOOL, path, error))?;
            if line.trim().is_empty() {
                continue;
            }

            let record = match serde_json::from_str::<CodexRecord>(&line) {
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
                                source_cli: Some(CODEX_TOOL),
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
            source_cli: CODEX_TOOL,
            source_session: session_id.into(),
            events,
        })
    }

    fn list_app_server_sessions(&self) -> Result<Option<Vec<SessionSummary>>, AdapterError> {
        let Some(app_server) = &self.app_server else {
            return Ok(None);
        };
        Ok(Some(
            app_server
                .list_threads(self.list_limit)?
                .into_iter()
                .map(app_thread_summary)
                .collect(),
        ))
    }

    fn list_fallback_sessions(&self) -> Result<Vec<SessionSummary>, AdapterError> {
        let k2_ids = self.k2_session_id_set()?;
        let mut sessions = if self.has_state_index() {
            let overrides = self.thread_name_overrides()?;
            self.list_thread_rows(self.list_limit)?
                .into_iter()
                .map(|mut row| {
                    apply_thread_name_override(&mut row, &overrides);
                    self.summary_for_thread_with_k2_ids(row, &k2_ids)
                })
                .filter(|session| self.include_codex_summary(session))
                .collect()
        } else {
            let mut sessions = Vec::new();
            for path in self.listed_session_files()? {
                let session = self.parse_summary_limited_with_k2_ids(
                    &path,
                    self.summary_line_limit,
                    &k2_ids,
                )?;
                if self.include_codex_summary(&session) {
                    sessions.push(session);
                }
            }
            sessions
        };
        self.append_k2_sessions(&mut sessions)?;
        Ok(sessions)
    }

    fn append_k2_sessions(&self, sessions: &mut Vec<SessionSummary>) -> Result<(), AdapterError> {
        let mut existing = HashSet::new();
        for session in sessions.iter() {
            existing.extend(session_id_aliases(&session.id));
            if let Some(session_key) = session
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.session_key.as_deref())
            {
                existing.extend(session_id_aliases(session_key));
            }
        }
        sessions.extend(
            self.list_k2_sessions()?.into_iter().filter(|session| {
                session_id_aliases(&session.id).all(|id| !existing.contains(&id))
            }),
        );
        collapse_codex_fork_chains(sessions);
        sessions.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
        if let Some(limit) = self.list_limit {
            sessions.truncate(limit);
        }
        Ok(())
    }

    fn list_k2_sessions(&self) -> Result<Vec<SessionSummary>, AdapterError> {
        if !self.k2_sessions_enabled {
            return Ok(Vec::new());
        };
        let Some(index) = self.read_k2_index()? else {
            return Ok(Vec::new());
        };
        let mut sessions = index
            .entries
            .into_iter()
            .filter(|entry| entry.agent.as_deref().unwrap_or("codex") == "codex")
            .map(|entry| self.k2_summary_from_index_entry(entry))
            .collect::<Vec<_>>();
        sessions.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
        Ok(sessions)
    }

    fn k2_summary_from_index_entry(&self, entry: K2SessionEntry) -> SessionSummary {
        let id = entry.session_id;
        let updated_at = entry
            .updated_at
            .and_then(timestamp_millis_to_rfc3339)
            .unwrap_or_else(|| "1970-01-01T00:00:00.000Z".into());
        let path = self.k2_session_path_for_id(&id);
        let source_size_bytes = path.as_deref().and_then(source_size_bytes);
        let source_path = path.as_deref().map(k2_source_path);
        let status_text = entry.status.as_deref().unwrap_or("unknown");
        let latest_run_status = entry.latest_run_status.as_deref().unwrap_or_default();
        let status = if matches!(status_text, "failed" | "error")
            || matches!(latest_run_status, "failed" | "error")
        {
            SessionStatus::Failed
        } else {
            SessionStatus::Healthy
        };
        let runtime_status = match status_text {
            "running" | "active" => SessionRuntimeStatus::Active,
            "idle" | "completed" => SessionRuntimeStatus::Inactive,
            _ => SessionRuntimeStatus::Unknown,
        };
        let title = first_non_empty_str([
            entry.title.as_deref(),
            entry.last_message_preview.as_deref(),
            Some(id.as_str()),
        ])
        .map(truncate_thread_title)
        .unwrap_or_else(|| format!("K2 Codex session {}", short_id(&id)));

        SessionSummary {
            id: id.clone(),
            cli: CODEX_TOOL,
            title,
            cwd: entry.cwd.unwrap_or_else(|| "~".into()),
            updated: human_timestamp(&updated_at),
            updated_at,
            runtime_status,
            runtime_reason: Some(format!("K2 Codex chat session status: {status_text}")),
            status,
            branch: None,
            token_count: None,
            health_reason: Some("K2 local chat session backed by Codex agent".into()),
            event_count: 0,
            resume_command: format!("k2 go codex resume {id}"),
            source_provenance: SourceProvenance::Real,
            source_path,
            source_size_bytes,
            parse_skip_count: 0,
            provider_metadata: Some(ProviderSessionMetadata {
                source: Some(K2_PROVIDER_SOURCE.into()),
                session_key: Some(id.clone()),
                ..ProviderSessionMetadata::default()
            }),
            context_health: None,
            anatomy: None,
        }
    }

    fn read_k2_index(&self) -> Result<Option<K2SessionIndex>, AdapterError> {
        let Some(index_path) = self.k2_sessions_index_path().filter(|path| path.is_file()) else {
            return Ok(None);
        };
        let contents = fs::read_to_string(&index_path)
            .map_err(|error| read_error(CODEX_TOOL, &index_path, error))?;
        serde_json::from_str::<K2SessionIndex>(&contents)
            .map(Some)
            .map_err(|error| {
                read_error(
                    CODEX_TOOL,
                    &index_path,
                    format!("invalid K2 sessions index: {error}"),
                )
            })
    }

    fn k2_session_id_set(&self) -> Result<HashSet<String>, AdapterError> {
        let Some(index) = self.read_k2_index()? else {
            return Ok(HashSet::new());
        };
        Ok(index
            .entries
            .into_iter()
            .filter(|entry| entry.agent.as_deref().unwrap_or("codex") == "codex")
            .flat_map(|entry| session_id_aliases(&entry.session_id).collect::<Vec<_>>())
            .collect())
    }

    fn include_codex_summary(&self, session: &SessionSummary) -> bool {
        self.k2_sessions_enabled || !is_k2_session_summary(session)
    }

    fn find_k2_session(&self, session_id: &str) -> Result<Option<SessionSummary>, AdapterError> {
        Ok(self
            .list_k2_sessions()?
            .into_iter()
            .find(|session| session.id == session_id))
    }

    fn k2_session_path_for_id(&self, session_id: &str) -> Option<PathBuf> {
        let file_name = format!("{}.json", session_id.replace(':', "_"));
        self.k2_sessions_dir().map(|dir| dir.join(file_name))
    }

    fn parse_k2_timeline(
        &self,
        session_id: &str,
        path: &Path,
        event_limit: Option<usize>,
    ) -> Result<CanonicalTimeline, AdapterError> {
        let contents =
            fs::read_to_string(path).map_err(|error| read_error(CODEX_TOOL, path, error))?;
        let session = serde_json::from_str::<K2SessionFile>(&contents).map_err(|error| {
            read_error(
                CODEX_TOOL,
                path,
                format!("invalid K2 session JSON: {error}"),
            )
        })?;
        let mut events = Vec::new();
        for (index, message) in session.messages.iter().enumerate() {
            if let Some(event) = k2_timeline_event(message, events.len() + 1, session_id, path)
                && push_timeline_event(&mut events, event, event_limit)
            {
                break;
            }
            if index + 1 == session.messages.len() {
                break;
            }
        }

        Ok(CanonicalTimeline {
            version: 1,
            source_cli: CODEX_TOOL,
            source_session: session_id.into(),
            events,
        })
    }

    fn fallback_report(
        &self,
        filter_status: &str,
        reason: &str,
        app_server_error: Option<&AdapterError>,
    ) -> Result<(Vec<SessionSummary>, super::model::SourceAdapterReport), AdapterError> {
        if self.has_state_index() {
            let sessions = self.list_fallback_sessions()?;
            let report = super::adapter::report_from_sessions_with_scan(
                SourceReportMeta {
                    cli: self.tool(),
                    provenance: self.provenance(),
                    active: true,
                    store_path: self.local_store_path(),
                    filter_status: fallback_filter_status(filter_status, app_server_error),
                    reason: fallback_report_reason(reason, app_server_error),
                    fidelity: Some(codex_fallback_fidelity(app_server_error)),
                    capabilities: app_server_error
                        .map(|error| app_server_unavailable_capabilities(true, error)),
                },
                &sessions,
                SourceScanStats {
                    list_limit: self.list_limit,
                    scan_entry_count: sessions.len(),
                    ..SourceScanStats::default()
                },
            );
            return Ok((sessions, report));
        }

        let discovery = self.listed_session_discovery()?;
        let k2_ids = self.k2_session_id_set()?;
        let mut sessions = Vec::new();
        for path in discovery.files {
            let session =
                self.parse_summary_limited_with_k2_ids(&path, self.summary_line_limit, &k2_ids)?;
            if self.include_codex_summary(&session) {
                sessions.push(session);
            }
        }
        self.append_k2_sessions(&mut sessions)?;
        let report = super::adapter::report_from_sessions_with_scan(
            SourceReportMeta {
                cli: self.tool(),
                provenance: self.provenance(),
                active: true,
                store_path: self.local_store_path(),
                filter_status: fallback_filter_status(filter_status, app_server_error),
                reason: fallback_report_reason(reason, app_server_error),
                fidelity: Some(codex_fallback_fidelity(app_server_error)),
                capabilities: app_server_error
                    .map(|error| app_server_unavailable_capabilities(true, error)),
            },
            &sessions,
            super::adapter::SourceScanStats {
                summary_line_limit: self.summary_line_limit,
                ..discovery.scan_stats
            },
        );
        Ok((sessions, report))
    }
}

impl SourceAdapter for CodexSourceAdapter {
    fn tool(&self) -> CliTool {
        CODEX_TOOL
    }

    fn provenance(&self) -> SourceProvenance {
        SourceProvenance::Real
    }

    fn store_path(&self) -> Option<String> {
        if let Some(app_server) = &self.app_server {
            return app_server.store_path();
        }
        self.local_store_path()
    }

    fn list_sessions(&self) -> Result<Vec<SessionSummary>, AdapterError> {
        if let Some(app_server) = &self.app_server {
            match app_server.list_threads(self.list_limit) {
                Ok(threads) => {
                    let mut sessions = threads.into_iter().map(app_thread_summary).collect();
                    self.append_k2_sessions(&mut sessions)?;
                    return Ok(sessions);
                }
                Err(error) if self.has_local_session_store() => {
                    let _ = error;
                }
                Err(error) => return Err(error),
            }
        }
        self.list_fallback_sessions()
    }

    fn list_sessions_with_report(
        &self,
        filter_status: &str,
        reason: &str,
    ) -> Result<(Vec<SessionSummary>, super::model::SourceAdapterReport), AdapterError> {
        if let Some(app_server) = &self.app_server {
            match self.list_app_server_sessions() {
                Ok(Some(sessions)) => {
                    let mut sessions = sessions;
                    self.append_k2_sessions(&mut sessions)?;
                    let report = super::adapter::report_from_sessions_with_scan(
                        SourceReportMeta {
                            cli: self.tool(),
                            provenance: self.provenance(),
                            active: true,
                            store_path: app_server.store_path(),
                            filter_status: "included_codex_app_server".into(),
                            reason:
                                "Codex app-server thread/list source; SQLite/JSONL remains fallback"
                                    .into(),
                            fidelity: Some(codex_app_server_fidelity(
                                self.has_local_session_store(),
                            )),
                            capabilities: Some(app_server_capabilities(
                                self.has_local_session_store(),
                            )),
                        },
                        &sessions,
                        SourceScanStats {
                            list_limit: self.list_limit,
                            scan_entry_count: sessions.len(),
                            ..SourceScanStats::default()
                        },
                    );
                    return Ok((sessions, report));
                }
                Ok(None) => {}
                Err(error) if self.has_local_session_store() => {
                    return self.fallback_report(filter_status, reason, Some(&error));
                }
                Err(error) => return Err(error),
            }
        }
        self.fallback_report(filter_status, reason, None)
    }

    fn find_session(&self, session_id: &str) -> Result<Option<SessionSummary>, AdapterError> {
        if let Some(app_server) = &self.app_server {
            match app_server.read_thread(session_id) {
                Ok(thread) => return Ok(Some(app_thread_summary(thread))),
                Err(error) if self.has_local_session_store() => {
                    let _ = error;
                }
                Err(_) => return Ok(None),
            }
        }
        if let Some(mut row) = self.find_thread_row(session_id)? {
            let overrides = self.thread_name_overrides()?;
            apply_thread_name_override(&mut row, &overrides);
            let session = self.summary_for_thread(row);
            return Ok(self.include_codex_summary(&session).then_some(session));
        }
        let Some(path) = self.find_session_path(session_id)? else {
            return self.find_k2_session(session_id);
        };
        let session = self.parse_summary(&path)?;
        Ok(self.include_codex_summary(&session).then_some(session))
    }

    fn load_timeline(&self, session_id: &str) -> Result<CanonicalTimeline, AdapterError> {
        if let Some(path) = self
            .k2_session_path_for_id(session_id)
            .filter(|path| path.is_file())
        {
            return self.parse_k2_timeline(session_id, &path, None);
        }
        if let Some(app_server) = &self.app_server {
            match app_server.load_timeline_limited(session_id, None) {
                Ok(timeline) => return Ok(timeline),
                Err(error) if self.has_local_session_store() => {
                    let _ = error;
                }
                Err(error) => return Err(error),
            }
        }
        let Some(path) = self.find_session_path(session_id)? else {
            return Err(AdapterError::SessionNotFound {
                tool: CODEX_TOOL,
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
            .and_then(k2_source_path_to_path)
            .filter(|path| path.is_file())
        {
            return self.parse_k2_timeline(&session.id, &path, event_limit);
        }
        if let Some(app_server) = &self.app_server
            && session
                .source_path
                .as_deref()
                .is_some_and(CodexAppServerSource::is_thread_source_path)
        {
            match app_server.load_timeline_limited(&session.id, event_limit) {
                Ok(timeline) => return Ok(timeline),
                Err(error) if self.has_local_session_store() => {
                    let _ = error;
                }
                Err(error) => return Err(error),
            }
        }
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
                tool: CODEX_TOOL,
                session_id: session.id.clone(),
            });
        };
        self.parse_timeline(&session.id, &path, event_limit)
    }
}

fn fallback_filter_status(filter_status: &str, app_server_error: Option<&AdapterError>) -> String {
    if app_server_error.is_some() {
        "included_codex_app_server_fallback".into()
    } else {
        filter_status.into()
    }
}

fn fallback_report_reason(reason: &str, app_server_error: Option<&AdapterError>) -> String {
    app_server_error
        .map(|error| format!("Codex app-server unavailable ({error}); {reason}"))
        .unwrap_or_else(|| reason.into())
}

fn codex_app_server_fidelity(local_store_available: bool) -> SourceFidelity {
    SourceFidelity {
        status: SourceFidelityStatus::FullFidelity,
        primary_surface: "codex_app_server_thread_api".into(),
        fallback_surface: local_store_available.then_some("codex_sqlite_jsonl_read_only".into()),
        detail: "documented opt-in app-server thread APIs are active; local store remains read-only fallback".into(),
    }
}

fn codex_fallback_fidelity(app_server_error: Option<&AdapterError>) -> SourceFidelity {
    let detail = match app_server_error {
        Some(error) => format!(
            "Codex app-server was configured but unavailable; using read-only SQLite/JSONL fallback: {error}"
        ),
        None => "read-only Codex SQLite/JSONL fallback; app-server rich API is not active".into(),
    };
    SourceFidelity {
        status: SourceFidelityStatus::Fallback,
        primary_surface: "codex_sqlite_jsonl_read_only".into(),
        fallback_surface: app_server_error
            .is_some()
            .then_some("codex_app_server_thread_api".into()),
        detail,
    }
}

fn app_server_capabilities(local_store_available: bool) -> SourceCapabilities {
    let mut capabilities =
        super::capability::source_capabilities(CODEX_TOOL, SourceProvenance::Real);
    capabilities.local_store = if local_store_available {
        cap(
            SourceCapabilityStatus::Available,
            "read-only state_5.sqlite or rollout JSONL fallback is also available",
        )
    } else {
        cap(
            SourceCapabilityStatus::Unavailable,
            "no Codex state_5.sqlite or rollout JSONL fallback discovered",
        )
    };
    capabilities.rich_local_rpc = cap(
        SourceCapabilityStatus::Available,
        "Codex app-server thread/list, thread/read, and thread/turns/list are configured",
    );
    capabilities.deep_link = cap(
        SourceCapabilityStatus::Available,
        "open-app can preview codex://threads/<id> deep links without launching",
    );
    capabilities.cloud_metadata = cap(
        SourceCapabilityStatus::Unknown,
        "Codex cloud task metadata is modeled separately and is not mixed into local threads",
    );
    capabilities.remote_control = cap(
        SourceCapabilityStatus::Unavailable,
        "Moonbox does not start Codex remote-control or app-server daemons",
    );
    capabilities
}

fn app_server_unavailable_capabilities(
    local_store_available: bool,
    error: &AdapterError,
) -> SourceCapabilities {
    let mut capabilities = app_server_capabilities(local_store_available);
    capabilities.rich_local_rpc = cap(
        SourceCapabilityStatus::Unavailable,
        format!("Codex app-server configured but unavailable; fallback used: {error}"),
    );
    capabilities
}

fn cap(status: SourceCapabilityStatus, detail: impl Into<String>) -> SourceCapability {
    SourceCapability {
        status,
        detail: detail.into(),
    }
}

#[derive(Debug, Deserialize)]
struct K2SessionIndex {
    #[serde(default)]
    entries: Vec<K2SessionEntry>,
}

#[derive(Debug, Deserialize)]
struct K2SessionEntry {
    #[serde(rename = "sessionId")]
    session_id: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default, rename = "updatedAt")]
    updated_at: Option<i64>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    agent: Option<String>,
    #[serde(default, rename = "lastMessagePreview")]
    last_message_preview: Option<String>,
    #[serde(default, rename = "latestRunStatus")]
    latest_run_status: Option<String>,
}

#[derive(Debug, Deserialize)]
struct K2SessionFile {
    #[serde(default)]
    messages: Vec<Value>,
}

#[cfg(not(test))]
fn default_k2_root() -> Option<PathBuf> {
    if let Some(path) = env::var_os(MOONBOX_K2_HOME_ENV).or_else(|| env::var_os(K2_CONFIG_DIR_ENV))
    {
        return Some(path.into());
    }
    env::var_os("HOME").map(|home| PathBuf::from(home).join(".k2"))
}

#[cfg(not(test))]
fn k2_sessions_enabled() -> bool {
    env_flag(MOONBOX_K2_SESSIONS_ENV)
        .unwrap_or_else(|| super::config::load_codex_config().k2_sessions)
}

#[cfg(not(test))]
fn env_flag(key: &str) -> Option<bool> {
    let value = env::var(key).ok()?;
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

pub(crate) fn is_k2_source_path(source_path: &str) -> bool {
    source_path.starts_with(K2_SOURCE_PREFIX)
}

pub(crate) fn is_k2_session_summary(session: &SessionSummary) -> bool {
    session
        .source_path
        .as_deref()
        .is_some_and(is_k2_source_path)
        || session_provider_source_is(session, K2_PROVIDER_SOURCE)
}

pub(crate) fn is_codex_app_session_summary(session: &SessionSummary) -> bool {
    session
        .source_path
        .as_deref()
        .is_some_and(CodexAppServerSource::is_thread_source_path)
        || session_provider_source_is(session, CODEX_APP_PROVIDER_SOURCE)
}

pub(crate) fn k2_resume_session_id(session: &SessionSummary) -> String {
    session
        .provider_metadata
        .as_ref()
        .and_then(|metadata| metadata.session_key.as_deref())
        .filter(|session_key| !session_key.trim().is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| canonical_k2_session_id(&session.id))
}

fn session_provider_source_is(session: &SessionSummary, source: &str) -> bool {
    session
        .provider_metadata
        .as_ref()
        .and_then(|metadata| metadata.source.as_deref())
        == Some(source)
}

fn k2_source_path(path: &Path) -> String {
    format!("{K2_SOURCE_PREFIX}{}", path.display())
}

fn k2_source_path_to_path(source_path: &str) -> Option<PathBuf> {
    source_path
        .strip_prefix(K2_SOURCE_PREFIX)
        .map(PathBuf::from)
}

fn annotate_codex_subsource(summary: &mut SessionSummary, k2_ids: &HashSet<String>) {
    let rollout_metadata = rollout_metadata(summary.source_path.as_deref());
    if let Some(parent_session_id) = rollout_metadata
        .parent_session_id
        .filter(|parent_session_id| !parent_session_id.trim().is_empty())
    {
        let metadata = summary
            .provider_metadata
            .get_or_insert_with(ProviderSessionMetadata::default);
        metadata.parent_session_id = Some(parent_session_id);
    }
    if let Some(originator) = rollout_metadata
        .originator
        .filter(|originator| !originator.trim().is_empty())
    {
        let metadata = summary
            .provider_metadata
            .get_or_insert_with(ProviderSessionMetadata::default);
        metadata.origin = Some(serde_json::json!({ "originator": originator }));
    }

    if let Some(k2_session_id) = matching_k2_session_id(&summary.id, k2_ids).or_else(|| {
        matches!(rollout_metadata.provider_source, Some(K2_PROVIDER_SOURCE))
            .then(|| canonical_k2_session_id(&summary.id))
    }) {
        let metadata = summary
            .provider_metadata
            .get_or_insert_with(ProviderSessionMetadata::default);
        metadata.source = Some(K2_PROVIDER_SOURCE.into());
        metadata.session_key = Some(k2_session_id.clone());
        summary.resume_command = format!("k2 go codex resume {k2_session_id}");
        return;
    }

    if matches!(
        rollout_metadata.provider_source,
        Some(CODEX_APP_PROVIDER_SOURCE)
    ) {
        let metadata = summary
            .provider_metadata
            .get_or_insert_with(ProviderSessionMetadata::default);
        metadata.source = Some(CODEX_APP_PROVIDER_SOURCE.into());
    }
}

fn matching_k2_session_id(session_id: &str, k2_ids: &HashSet<String>) -> Option<String> {
    session_id_aliases(session_id)
        .find(|alias| k2_ids.contains(alias))
        .map(|alias| {
            if alias.starts_with("codex:") {
                alias
            } else {
                canonical_k2_session_id(&alias)
            }
        })
}

fn session_id_aliases(session_id: &str) -> impl Iterator<Item = String> + '_ {
    let trimmed = session_id.trim();
    let without_prefix = trimmed.strip_prefix("codex:").unwrap_or(trimmed);
    [
        without_prefix.to_owned(),
        canonical_k2_session_id(without_prefix),
    ]
    .into_iter()
}

fn canonical_k2_session_id(session_id: &str) -> String {
    let trimmed = session_id.trim();
    if trimmed.starts_with("codex:") {
        trimmed.into()
    } else {
        format!("codex:{trimmed}")
    }
}

fn rollout_metadata(source_path: Option<&str>) -> CodexRolloutMetadata {
    let Some(source_path) = source_path else {
        return CodexRolloutMetadata::default();
    };
    if is_k2_source_path(source_path) {
        return CodexRolloutMetadata {
            provider_source: Some(K2_PROVIDER_SOURCE),
            originator: None,
            parent_session_id: None,
        };
    }
    if CodexAppServerSource::is_thread_source_path(source_path) {
        return CodexRolloutMetadata {
            provider_source: Some(CODEX_APP_PROVIDER_SOURCE),
            originator: None,
            parent_session_id: None,
        };
    }
    let path = Path::new(source_path);
    if !path.is_file() {
        return CodexRolloutMetadata::default();
    }
    let Ok(mut file) = fs::File::open(path) else {
        return CodexRolloutMetadata::default();
    };
    let mut buffer = vec![0; 256 * 1024];
    let Ok(bytes) = file.read(&mut buffer) else {
        return CodexRolloutMetadata::default();
    };
    let Ok(prefix) = std::str::from_utf8(&buffer[..bytes]) else {
        return CodexRolloutMetadata::default();
    };
    let provider_source = if prefix.contains(r#""model_provider":"k2_airouter""#)
        || prefix.contains(r#""originator":"cli-server-bridge""#)
    {
        Some(K2_PROVIDER_SOURCE)
    } else if prefix.contains(r#""originator":"Codex Desktop""#) {
        Some(CODEX_APP_PROVIDER_SOURCE)
    } else {
        None
    };
    CodexRolloutMetadata {
        provider_source,
        originator: json_string_field_in_text(prefix, "originator"),
        parent_session_id: json_string_field_in_text(prefix, "forked_from_id"),
    }
}

fn json_string_field_in_text(text: &str, key: &str) -> Option<String> {
    let pattern = format!(r#""{key}""#);
    let after_key = text.get(text.find(&pattern)? + pattern.len()..)?;
    let after_colon = after_key.get(after_key.find(':')? + 1..)?.trim_start();
    if !after_colon.starts_with('"') {
        return None;
    }
    let mut deserializer = serde_json::Deserializer::from_str(after_colon);
    String::deserialize(&mut deserializer).ok()
}

fn collapse_codex_fork_chains(sessions: &mut Vec<SessionSummary>) {
    let parents: HashMap<String, String> = sessions
        .iter()
        .filter_map(|session| {
            let parent = session
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.parent_session_id.as_deref())?
                .trim();
            (!parent.is_empty()).then(|| (session.id.clone(), parent.to_owned()))
        })
        .collect();
    if parents.is_empty() {
        return;
    }

    sessions.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
    let mut seen_roots = HashSet::new();
    sessions.retain(|session| seen_roots.insert(codex_fork_root(&session.id, &parents)));
}

fn codex_fork_root(session_id: &str, parents: &HashMap<String, String>) -> String {
    let mut current = session_id.to_owned();
    let mut seen = HashSet::new();
    while seen.insert(current.clone()) {
        let Some(parent) = parents
            .get(&current)
            .filter(|parent| !parent.trim().is_empty())
        else {
            return current;
        };
        current = parent.clone();
    }
    current
}

fn codex_provider_metadata(
    originator: Option<String>,
    parent_session_id: Option<String>,
) -> Option<ProviderSessionMetadata> {
    if originator.is_none() && parent_session_id.is_none() {
        return None;
    }
    Some(ProviderSessionMetadata {
        parent_session_id,
        origin: originator.map(|originator| serde_json::json!({ "originator": originator })),
        ..ProviderSessionMetadata::default()
    })
}

fn k2_timeline_event(
    message: &Value,
    number: usize,
    session_id: &str,
    path: &Path,
) -> Option<TimelineEvent> {
    let role = string_field(message, "role").unwrap_or_default();
    let message_type = string_field(message, "type");
    let kind = match (message_type, role) {
        (Some("status"), _) => TimelineKind::Tool,
        (_, "user") => TimelineKind::User,
        (_, "assistant") => TimelineKind::Assistant,
        _ => TimelineKind::Tool,
    };
    let detail = string_field(message, "text")
        .filter(|text| !text.trim().is_empty())
        .map(str::to_owned)
        .or_else(|| k2_process_detail(message))
        .unwrap_or_default();
    if detail.trim().is_empty() {
        return None;
    }
    let title = string_field(message, "title")
        .map(str::to_owned)
        .unwrap_or_else(|| match kind {
            TimelineKind::User => "User".into(),
            TimelineKind::Assistant => "Assistant".into(),
            TimelineKind::Tool => "K2 Process".into(),
            _ => "K2 Event".into(),
        });
    let timestamp = message
        .get("createdAt")
        .and_then(Value::as_i64)
        .and_then(timestamp_millis_to_rfc3339);
    let message_id = string_field(message, "id").map(str::to_owned);
    let metadata = TimelineEventMetadata {
        raw_refs: vec![TimelineEventRawRef {
            source_cli: Some(CODEX_TOOL),
            source_session: Some(session_id.into()),
            source_path: Some(path.display().to_string()),
            row_id: message_id.clone(),
            record_type: Some("k2_message".into()),
            provider_kind: message_type.map(str::to_owned),
            role: (!role.is_empty()).then(|| role.to_owned()),
            digest: Some(stable_value_digest(message)),
            ..TimelineEventRawRef::default()
        }],
        message_ids: message_id.iter().cloned().collect(),
        provider_item_ids: message_id.into_iter().collect(),
        runtime: k2_runtime_metadata(message),
        ..TimelineEventMetadata::default()
    };

    Some(TimelineEvent {
        id: event_id(number),
        time: display_time(timestamp.as_deref()),
        kind,
        title,
        detail: truncate_timeline_detail(&detail),
        metadata,
    })
}

fn k2_process_detail(message: &Value) -> Option<String> {
    let process = message
        .get("metadata")
        .and_then(|metadata| metadata.get("process"))?;
    let events = process.get("events").and_then(Value::as_array)?;
    let mut lines = Vec::new();
    if let Some(title) = string_field(message, "title") {
        lines.push(title.to_owned());
    }
    if let Some(run_id) = string_field(message.get("metadata")?, "runId") {
        lines.push(format!("run: {run_id}"));
    }
    lines.push(format!("events: {}", events.len()));
    for event in events.iter().take(20) {
        let label = string_field(event, "label")
            .or_else(|| string_field(event, "kind"))
            .unwrap_or("event");
        let status = string_field(event, "status").unwrap_or("unknown");
        lines.push(format!("- [{status}] {label}"));
        if let Some(output) = event
            .get("command")
            .and_then(|command| string_field(command, "output"))
            .filter(|output| !output.trim().is_empty())
        {
            lines.push(truncate(output.trim(), 800));
        }
    }
    if events.len() > 20 {
        lines.push(format!("... {} more K2 process events", events.len() - 20));
    }
    Some(lines.join("\n"))
}

fn k2_runtime_metadata(message: &Value) -> Option<TimelineRuntimeMetadata> {
    let process = message
        .get("metadata")
        .and_then(|metadata| metadata.get("process"))?;
    let started_at = process.get("startedAt").and_then(Value::as_i64);
    let ended_at = process.get("endedAt").and_then(Value::as_i64);
    let is_running = process
        .get("isRunning")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let duration_ms = started_at
        .zip(ended_at)
        .and_then(|(start, end)| u64::try_from(end.saturating_sub(start)).ok());
    Some(TimelineRuntimeMetadata {
        status: if is_running {
            SessionRuntimeStatus::Active
        } else {
            SessionRuntimeStatus::Inactive
        },
        reason: Some("K2 process message".into()),
        duration_ms,
        api_duration_ms: None,
        turn_count: process
            .get("events")
            .and_then(Value::as_array)
            .and_then(|events| u64::try_from(events.len()).ok()),
    })
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

fn first_non_empty_str<'a>(values: impl IntoIterator<Item = Option<&'a str>>) -> Option<&'a str> {
    values
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|value| !value.is_empty() && *value != "-")
}

impl SummaryBuilder {
    fn new(path: &Path) -> Self {
        Self {
            path: path.into(),
            id: None,
            title: None,
            cwd: None,
            originator: None,
            parent_session_id: None,
            updated_at: timestamp_from_filename(path),
            branch: None,
            token_count: None,
            context_used_tokens: None,
            context_window_tokens: None,
            compact_layers: 0,
            handoff_markers: 0,
            event_count: 0,
            malformed_lines: 0,
            summary_truncated: false,
            has_error: false,
        }
    }

    fn observe(&mut self, record: CodexRecord) {
        self.event_count += 1;
        if let Some(timestamp) = record.timestamp.as_deref() {
            self.updated_at = Some(max_timestamp(self.updated_at.take(), timestamp));
        }

        let record_type = record.record_type.as_deref().unwrap_or_default();
        let payload_type = string_field(&record.payload, "type").unwrap_or_default();
        self.has_error |= is_error_record(record_type, payload_type);
        if record_type == "compacted" {
            self.compact_layers += 1;
        }
        if codex_record_has_handoff_marker(&record) {
            self.handoff_markers += 1;
        }

        match record_type {
            "session_meta" => self.observe_session_meta(&record.payload),
            "turn_context" => self.observe_turn_context(&record.payload),
            "response_item" => self.observe_response_item(&record.payload),
            "event_msg" => self.observe_event_msg(&record.payload),
            _ => {}
        }
    }

    fn observe_session_meta(&mut self, payload: &Value) {
        if self.id.is_none() {
            self.id = string_field(payload, "id").map(str::to_owned);
        }
        if self.cwd.is_none() {
            self.cwd = string_field(payload, "cwd").map(str::to_owned);
        }
        if self.originator.is_none() {
            self.originator = string_field(payload, "originator").map(str::to_owned);
        }
        if self.parent_session_id.is_none() {
            self.parent_session_id = string_field(payload, "forked_from_id").map(str::to_owned);
        }
        if self.branch.is_none() {
            self.branch = payload
                .get("git")
                .and_then(|git| string_field(git, "branch"))
                .map(str::to_owned);
        }
    }

    fn observe_turn_context(&mut self, payload: &Value) {
        if self.cwd.is_none() {
            self.cwd = string_field(payload, "cwd").map(str::to_owned);
        }
    }

    fn observe_response_item(&mut self, payload: &Value) {
        if self.title.is_none()
            && string_field(payload, "role") == Some("user")
            && let Some(text) = text_from_value(payload.get("content").unwrap_or(&Value::Null))
            && !is_provider_context_text(&text)
            && !is_moonbox_handoff_control_text(&text)
        {
            self.title = Some(truncate(&text, 160));
        }
    }

    fn observe_event_msg(&mut self, payload: &Value) {
        if self.title.is_none()
            && string_field(payload, "type") == Some("user_message")
            && let Some(text) = text_from_value(payload)
            && !is_provider_context_text(&text)
            && !is_moonbox_handoff_control_text(&text)
        {
            self.title = Some(truncate(&text, 160));
        }
        if self.token_count.is_none()
            && string_field(payload, "type") == Some("token_count")
            && let Some(count) = find_token_count(payload)
        {
            self.token_count = Some(count);
        }
        if string_field(payload, "type") == Some("token_count") {
            if let Some(count) = payload
                .get("info")
                .and_then(|info| info.get("last_token_usage"))
                .and_then(|usage| usize_field(usage, "total_tokens"))
            {
                self.context_used_tokens = Some(count);
            }
            if let Some(window) = payload
                .get("info")
                .and_then(|info| usize_field(info, "model_context_window"))
            {
                self.context_window_tokens = Some(window);
            }
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
                "real Codex JSONL session; summary preview truncated; skipped {} malformed line(s)",
                self.malformed_lines
            )
        } else if self.summary_truncated {
            "real Codex JSONL session; summary preview truncated".into()
        } else if self.malformed_lines > 0 {
            format!(
                "real Codex JSONL session; skipped {} malformed line(s)",
                self.malformed_lines
            )
        } else {
            "real Codex JSONL session".into()
        };

        SessionSummary {
            id: id.clone(),
            cli: CODEX_TOOL,
            title: self
                .title
                .unwrap_or_else(|| format!("Codex session {}", short_id(&id))),
            cwd: self.cwd.unwrap_or_else(|| "~".into()),
            updated: human_timestamp(&updated_at),
            updated_at,
            runtime_status: SessionRuntimeStatus::Unknown,
            runtime_reason: Some(unknown_runtime_reason(CODEX_TOOL)),
            status,
            branch: self.branch,
            token_count: self.token_count,
            health_reason: Some(health_reason),
            event_count: self.event_count,
            resume_command: format!("codex resume {id}"),
            source_provenance: SourceProvenance::Real,
            source_path: Some(self.path.display().to_string()),
            source_size_bytes: source_size_bytes(&self.path),
            parse_skip_count: self.malformed_lines,
            provider_metadata: codex_provider_metadata(self.originator, self.parent_session_id),
            context_health: context_health_from_token_count(
                self.context_used_tokens.or(self.token_count),
                self.context_window_tokens,
                self.compact_layers,
                self.handoff_markers,
                if self.context_used_tokens.is_some() && self.context_window_tokens.is_some() {
                    EvidenceConfidence::Exact
                } else if self.context_used_tokens.is_some() || self.token_count.is_some() {
                    EvidenceConfidence::Derived
                } else {
                    EvidenceConfidence::Unknown
                },
                "codex token_count event",
            ),
            anatomy: None,
        }
    }
}

fn context_health_from_token_count(
    used_tokens: Option<usize>,
    window_tokens: Option<usize>,
    compact_layers: usize,
    handoff_markers: usize,
    confidence: EvidenceConfidence,
    source: &str,
) -> Option<ContextHealth> {
    let used_tokens = used_tokens.filter(|tokens| *tokens > 0);
    let window_tokens = window_tokens.filter(|tokens| *tokens > 0);
    (used_tokens.is_some() || window_tokens.is_some() || compact_layers > 0 || handoff_markers > 0)
        .then(|| ContextHealth {
            used_tokens,
            window_tokens,
            quality_cliff_tokens: Some(120_000),
            compact_layers,
            handoff_markers,
            confidence,
            source: source.into(),
        })
}

fn usize_field(value: &Value, key: &str) -> Option<usize> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|count| usize::try_from(count).ok())
}

fn codex_record_has_handoff_marker(record: &CodexRecord) -> bool {
    serde_json::to_string(&record.payload)
        .ok()
        .is_some_and(|text| {
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

fn context_health_from_rollout_tail(path: &Path) -> Option<ContextHealth> {
    let mut file = fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let start = len.saturating_sub(CODEX_CONTEXT_TAIL_BYTES);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut bytes = Vec::with_capacity((len - start).min(CODEX_CONTEXT_TAIL_BYTES) as usize);
    file.read_to_end(&mut bytes).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    let mut lines = text.lines();
    if start > 0 {
        lines.next();
    }

    let mut used_tokens = None;
    let mut window_tokens = None;
    for line in lines {
        let Ok(record) = serde_json::from_str::<CodexRecord>(line) else {
            continue;
        };
        if record.record_type.as_deref() != Some("event_msg")
            || string_field(&record.payload, "type") != Some("token_count")
        {
            continue;
        }
        if let Some(count) = record
            .payload
            .get("info")
            .and_then(|info| info.get("last_token_usage"))
            .and_then(|usage| usize_field(usage, "total_tokens"))
            .or_else(|| find_token_count(&record.payload))
        {
            used_tokens = Some(count);
        }
        if let Some(window) = record
            .payload
            .get("info")
            .and_then(|info| usize_field(info, "model_context_window"))
        {
            window_tokens = Some(window);
        }
    }

    context_health_from_token_count(
        used_tokens,
        window_tokens,
        0,
        0,
        if used_tokens.is_some() && window_tokens.is_some() {
            EvidenceConfidence::Exact
        } else if used_tokens.is_some() {
            EvidenceConfidence::Derived
        } else {
            EvidenceConfidence::Unknown
        },
        "codex rollout tail token_count",
    )
}

fn source_size_bytes(path: &Path) -> Option<u64> {
    fs::metadata(path).ok().map(|metadata| metadata.len())
}

fn timeline_event(
    record: CodexRecord,
    number: usize,
    session_id: &str,
    path: &Path,
    line_number: usize,
) -> Option<TimelineEvent> {
    let record_type = record.record_type.as_deref().unwrap_or_default();
    let payload_type = string_field(&record.payload, "type").unwrap_or_default();
    let role = string_field(&record.payload, "role");
    let kind = timeline_kind(record_type, payload_type, role)?;
    let title = timeline_title(record_type, payload_type, role);
    let image_markup =
        extract_timeline_image_markup(&timeline_detail(&record.payload, record_type, payload_type));
    let detail = image_markup.text;
    let mut metadata = timeline_metadata(&record, session_id, path, line_number, kind);
    if detail.is_empty()
        && image_markup.attachments.is_empty()
        && metadata.tool_calls.is_empty()
        && metadata.tool_results.is_empty()
        && metadata.runtime.is_none()
        && !matches!(kind, TimelineKind::Error)
    {
        return None;
    }
    if kind == TimelineKind::User
        && (is_provider_context_text(&detail) || is_moonbox_handoff_control_text(&detail))
    {
        return None;
    }
    metadata.attachments.extend(image_markup.attachments);

    Some(TimelineEvent {
        id: event_id(number),
        time: display_time(record.timestamp.as_deref()),
        kind,
        title,
        detail,
        metadata,
    })
}

fn timeline_metadata(
    record: &CodexRecord,
    session_id: &str,
    path: &Path,
    line_number: usize,
    kind: TimelineKind,
) -> TimelineEventMetadata {
    let payload_type = string_field(&record.payload, "type").map(str::to_owned);
    let role = string_field(&record.payload, "role").map(str::to_owned);
    let message_ids = id_fields(&record.payload, &["message_id", "msg_id", "messageId"]);
    let provider_item_ids = id_fields(&record.payload, &["id", "item_id", "itemId", "call_id"]);
    let token_usage = find_token_count(&record.payload).map(token_breakdown);
    let duration_ms = record.payload.get("duration_ms").and_then(Value::as_u64);
    let record_type = record.record_type.clone();
    TimelineEventMetadata {
        raw_refs: vec![TimelineEventRawRef {
            source_cli: Some(CODEX_TOOL),
            source_session: Some(session_id.into()),
            source_path: Some(path.display().to_string()),
            line_number: Some(line_number),
            record_type: record_type.clone(),
            provider_kind: payload_type.clone(),
            role,
            digest: Some(stable_value_digest(&record.payload)),
            ..TimelineEventRawRef::default()
        }],
        message_ids,
        provider_item_ids,
        tool_calls: tool_calls_from_codex_payload(&record.payload, payload_type.as_deref()),
        tool_results: tool_results_from_codex_payload(&record.payload, payload_type.as_deref()),
        approvals: approvals_from_payload(
            &record.payload,
            record_type.as_deref(),
            payload_type.as_deref(),
        ),
        file_changes: file_changes_from_payload(&record.payload, kind),
        runtime: runtime_from_codex_payload(payload_type.as_deref(), duration_ms),
        system_prompt_snapshot: system_prompt_snapshot(&record.payload),
        config_snapshot: config_snapshot(&record.payload),
        token_usage,
        ..TimelineEventMetadata::default()
    }
}

fn timeline_kind(
    record_type: &str,
    payload_type: &str,
    role: Option<&str>,
) -> Option<TimelineKind> {
    if is_error_record(record_type, payload_type) {
        return Some(TimelineKind::Error);
    }
    if payload_type.contains("compact") {
        return Some(TimelineKind::Compact);
    }
    if payload_type.contains("diff") {
        return Some(TimelineKind::GitDiff);
    }
    match (record_type, payload_type, role) {
        ("session_meta", _, _) => Some(TimelineKind::Tool),
        ("response_item", "message", Some("user")) => Some(TimelineKind::User),
        ("response_item", "message", Some("assistant")) => Some(TimelineKind::Assistant),
        ("response_item", _, _) if payload_type.contains("call") => Some(TimelineKind::Tool),
        ("event_msg", "user_message", _) => Some(TimelineKind::User),
        ("event_msg", "agent_message", _) => Some(TimelineKind::Assistant),
        ("event_msg", _, _) => Some(TimelineKind::Tool),
        _ => None,
    }
}

fn timeline_title(record_type: &str, payload_type: &str, role: Option<&str>) -> String {
    match (record_type, payload_type, role) {
        ("session_meta", _, _) => "Session".into(),
        ("response_item", "message", Some("user")) | ("event_msg", "user_message", _) => {
            "User".into()
        }
        ("response_item", "message", Some("assistant")) | ("event_msg", "agent_message", _) => {
            "Assistant".into()
        }
        ("event_msg", "task_started", _) => "Task started".into(),
        ("event_msg", "task_complete", _) => "Task complete".into(),
        ("event_msg", "token_count", _) => "Token count".into(),
        (_, payload_type, _) if payload_type.contains("diff") => "Git diff".into(),
        (_, payload_type, _) if payload_type.contains("compact") => "Compact".into(),
        (_, payload_type, _) if payload_type.contains("error") => "Error".into(),
        (_, payload_type, _) if !payload_type.is_empty() => title_case(payload_type),
        _ => title_case(record_type),
    }
}

fn timeline_detail(payload: &Value, record_type: &str, payload_type: &str) -> String {
    if record_type == "session_meta" {
        return string_field(payload, "cwd")
            .map(|cwd| format!("cwd: {cwd}"))
            .unwrap_or_else(|| "session started".into());
    }
    if payload_type == "task_complete"
        && let Some(duration) = payload.get("duration_ms").and_then(Value::as_u64)
    {
        return format!("completed in {duration} ms");
    }
    text_from_value(payload)
        .map(|text| truncate_timeline_detail(&text))
        .unwrap_or_default()
}

fn id_fields(payload: &Value, keys: &[&str]) -> Vec<String> {
    keys.iter()
        .filter_map(|key| string_field(payload, key))
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned)
        .fold(Vec::new(), |mut values, value| {
            if !values.contains(&value) {
                values.push(value);
            }
            values
        })
}

fn token_breakdown(total: usize) -> TokenBreakdown {
    TokenBreakdown {
        total,
        ..TokenBreakdown::default()
    }
}

fn clone_non_null(value: Option<&Value>) -> Option<Value> {
    value.filter(|value| !value.is_null()).cloned()
}

fn first_string(payload: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| string_field(payload, key))
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned)
}

fn tool_calls_from_codex_payload(
    payload: &Value,
    payload_type: Option<&str>,
) -> Vec<TimelineToolCall> {
    if payload_type == Some("function_call_output") {
        return Vec::new();
    }
    let is_tool_call = payload_type.is_some_and(|value| value.contains("call"))
        || payload.get("arguments").is_some()
        || payload.get("input").is_some();
    if !is_tool_call {
        return Vec::new();
    }
    vec![TimelineToolCall {
        id: first_string(payload, &["call_id", "id", "item_id"]),
        name: first_string(payload, &["name", "tool_name", "function_name", "command"]),
        arguments: clone_non_null(
            payload
                .get("arguments")
                .or_else(|| payload.get("args"))
                .or_else(|| payload.get("input"))
                .or_else(|| payload.get("params")),
        ),
        raw: Some(payload.clone()),
    }]
}

fn tool_results_from_codex_payload(
    payload: &Value,
    payload_type: Option<&str>,
) -> Vec<TimelineToolResult> {
    if payload_type != Some("function_call_output") {
        return Vec::new();
    }
    vec![TimelineToolResult {
        call_id: first_string(payload, &["call_id", "id", "item_id"]),
        name: first_string(payload, &["name", "tool_name", "function_name", "command"]),
        content: first_string(payload, &["output", "content", "result"]),
        is_error: function_call_output_is_error(payload),
        raw: Some(payload.clone()),
    }]
}

fn function_call_output_is_error(payload: &Value) -> Option<bool> {
    first_string(payload, &["status"])
        .map(|status| {
            let status = status.to_ascii_lowercase();
            status.contains("fail") || status.contains("error")
        })
        .or_else(|| {
            first_string(payload, &["output"]).and_then(|output| {
                output
                    .lines()
                    .find_map(|line| line.trim().strip_prefix("Process exited with code "))
                    .and_then(|code| code.parse::<i32>().ok())
                    .map(|code| code != 0)
            })
        })
}

fn approvals_from_payload(
    payload: &Value,
    record_type: Option<&str>,
    payload_type: Option<&str>,
) -> Vec<TimelineApproval> {
    let is_approval = record_type.is_some_and(|value| value.contains("approval"))
        || payload_type.is_some_and(|value| value.contains("approval"))
        || payload.get("approval").is_some();
    if !is_approval {
        return Vec::new();
    }
    vec![TimelineApproval {
        action: first_string(payload, &["action", "command", "cmd"]),
        decision: first_string(payload, &["decision", "status", "result"]),
        reason: first_string(payload, &["reason", "message"]),
        raw: Some(payload.clone()),
    }]
}

fn file_changes_from_payload(payload: &Value, kind: TimelineKind) -> Vec<TimelineFileChange> {
    if kind != TimelineKind::GitDiff {
        return Vec::new();
    }
    vec![TimelineFileChange {
        path: first_string(payload, &["path", "file", "file_path"]),
        operation: first_string(payload, &["operation", "op", "change_type"]),
        summary: text_from_value(payload).map(|text| truncate_timeline_detail(&text)),
        diff: text_from_value(payload),
        raw: Some(payload.clone()),
    }]
}

fn runtime_from_codex_payload(
    payload_type: Option<&str>,
    duration_ms: Option<u64>,
) -> Option<TimelineRuntimeMetadata> {
    match payload_type {
        Some("task_started") => Some(TimelineRuntimeMetadata {
            status: SessionRuntimeStatus::Active,
            reason: Some("Codex task started".into()),
            ..TimelineRuntimeMetadata::default()
        }),
        Some("task_complete") => Some(TimelineRuntimeMetadata {
            status: SessionRuntimeStatus::Inactive,
            reason: Some("Codex task completed".into()),
            duration_ms,
            ..TimelineRuntimeMetadata::default()
        }),
        _ => duration_ms.map(|duration_ms| TimelineRuntimeMetadata {
            duration_ms: Some(duration_ms),
            ..TimelineRuntimeMetadata::default()
        }),
    }
}

fn system_prompt_snapshot(payload: &Value) -> Option<String> {
    first_string(
        payload,
        &["system_prompt", "instructions", "developer_message"],
    )
}

fn config_snapshot(payload: &Value) -> Option<Value> {
    clone_non_null(
        payload
            .get("model_config")
            .or_else(|| payload.get("config"))
            .or_else(|| payload.get("settings")),
    )
}

const THREAD_SELECT: &str = r#"
    select
        id,
        rollout_path,
        strftime(
            '%Y-%m-%dT%H:%M:%fZ',
            coalesce(updated_at_ms, updated_at * 1000, created_at_ms, created_at * 1000) / 1000.0,
            'unixepoch'
        ) as updated_at,
        cwd,
        title,
        preview,
        first_user_message,
        git_branch,
        coalesce(tokens_used, 0) as tokens_used,
        archived != 0 as archived,
        coalesce(updated_at_ms, updated_at * 1000, created_at_ms, created_at * 1000) as updated_at_ms_sort
    from threads
"#;

fn thread_row(row: &Row<'_>) -> rusqlite::Result<CodexThreadRow> {
    Ok(CodexThreadRow {
        id: row.get(0)?,
        rollout_path: row.get(1)?,
        updated_at: row.get(2)?,
        cwd: row.get(3)?,
        title: row.get(4)?,
        preview: row.get(5)?,
        first_user_message: row.get(6)?,
        branch: row.get(7)?,
        token_count: row.get::<_, i64>(8).unwrap_or_default().max(0) as usize,
        archived: row.get(9)?,
    })
}

fn apply_thread_name_override(row: &mut CodexThreadRow, overrides: &HashMap<String, String>) {
    if let Some(title) = overrides.get(&row.id) {
        row.title = title.clone();
    }
}

fn collect_rows<T>(
    rows: MappedRows<'_, impl FnMut(&Row<'_>) -> rusqlite::Result<T>>,
    path: &Path,
) -> Result<Vec<T>, AdapterError> {
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|error| read_error(CODEX_TOOL, path, error))
}

fn first_non_empty<'a>(values: impl IntoIterator<Item = &'a String>) -> Option<&'a str> {
    values
        .into_iter()
        .map(|value| value.trim())
        .find(|value| !value.is_empty() && *value != "-")
}

fn truncate_thread_title(title: &str) -> String {
    truncate(&title.split_whitespace().collect::<Vec<_>>().join(" "), 160)
}

fn normalized_token_count(token_count: usize) -> Option<usize> {
    (1..=1_000_000)
        .contains(&token_count)
        .then_some(token_count)
}

fn is_error_record(record_type: &str, payload_type: &str) -> bool {
    record_type.contains("error") || payload_type.contains("error")
}

fn timestamp_from_filename(path: &Path) -> Option<String> {
    let stem = path.file_stem()?.to_str()?;
    let timestamp = stem.strip_prefix("rollout-")?.get(..19)?;
    Some(format!("{}+00:00", replace_time_dashes(timestamp)))
}

fn id_from_path(path: &Path) -> String {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("codex-session")
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
    fn lists_codex_sessions_from_jsonl_store() {
        let root = test_root("list");
        let session_path = write_session(
            &root,
            "2026/06/06/rollout-2026-06-06T08-00-00-test.jsonl",
            r#"{"timestamp":"2026-06-06T08:00:00.000Z","type":"session_meta","payload":{"id":"codex-real-1","cwd":"/repo","git":{"branch":"main"}}}
{"timestamp":"2026-06-06T08:01:00.000Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"Implement a real adapter"}]}}
{"timestamp":"2026-06-06T08:02:00.000Z","type":"event_msg","payload":{"type":"token_count","info":{"total_tokens":42,"last_token_usage":{"total_tokens":169790},"model_context_window":258400}}}
{"timestamp":"2026-06-06T08:03:00.000Z","type":"compacted","payload":{"type":"compact"}}
{"timestamp":"2026-06-06T08:04:00.000Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"moonbox-continuation-handoff ready"}]}}
"#,
        );

        let sessions = CodexSourceAdapter::new(&root)
            .list_sessions()
            .expect("sessions");

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, "codex-real-1");
        assert_eq!(sessions[0].title, "Implement a real adapter");
        assert_eq!(sessions[0].cwd, "/repo");
        assert_eq!(sessions[0].branch.as_deref(), Some("main"));
        assert_eq!(sessions[0].token_count, Some(42));
        let context_health = sessions[0].context_health.as_ref().expect("context health");
        assert_eq!(context_health.used_tokens, Some(169_790));
        assert_eq!(context_health.window_tokens, Some(258_400));
        assert_eq!(context_health.compact_layers, 1);
        assert_eq!(context_health.handoff_markers, 1);
        assert_eq!(context_health.confidence, EvidenceConfidence::Exact);
        assert_eq!(sessions[0].resume_command, "codex resume codex-real-1");
        assert_eq!(
            sessions[0].source_size_bytes,
            Some(fs::metadata(session_path).expect("session metadata").len())
        );
    }

    #[test]
    fn lists_codex_sessions_from_state_thread_index_when_available() {
        let root = test_root("state-index");
        let rollout_path = write_session(
            &root,
            "2026/06/06/rollout-2026-06-06T08-00-00-indexed.jsonl",
            r#"{"timestamp":"2026-06-06T08:00:00.000Z","type":"session_meta","payload":{"id":"codex-indexed","cwd":"/jsonl","git":{"branch":"jsonl-branch"}}}
{"timestamp":"2026-06-06T08:01:00.000Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"Wrong JSONL title"}]}}
"#,
        );
        write_state_db(
            &root,
            &rollout_path,
            "codex-indexed",
            "Renamed from Codex resume",
        );

        let sessions = CodexSourceAdapter::new(&root)
            .list_sessions()
            .expect("sessions");
        let timeline = CodexSourceAdapter::new(&root)
            .load_timeline("codex-indexed")
            .expect("timeline");

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, "codex-indexed");
        assert_eq!(sessions[0].title, "Renamed from Codex resume");
        assert_eq!(sessions[0].cwd, "/sqlite");
        assert_eq!(sessions[0].branch.as_deref(), Some("sqlite-branch"));
        assert_eq!(sessions[0].token_count, Some(42));
        assert_eq!(sessions[0].updated_at, "2026-06-06T09:00:00.000Z");
        assert_eq!(
            sessions[0].source_size_bytes,
            Some(fs::metadata(rollout_path).expect("rollout metadata").len())
        );
        assert_eq!(timeline.source_session, "codex-indexed");
    }

    #[test]
    fn codex_session_index_thread_name_overrides_sqlite_title() {
        let root = test_root("session-index-title");
        let rollout_path = write_session(
            &root,
            "2026/06/06/rollout-2026-06-06T08-00-00-indexed.jsonl",
            r#"{"timestamp":"2026-06-06T08:00:00.000Z","type":"session_meta","payload":{"id":"codex-renamed","cwd":"/jsonl"}}
{"timestamp":"2026-06-06T08:01:00.000Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"Old URL title"}]}}
"#,
        );
        write_state_db(&root, &rollout_path, "codex-renamed", "Old SQLite title");
        write_session_index(
            &root,
            r#"{"id":"codex-renamed","thread_name":"102_303","updated_at":"2026-06-06T09:00:00Z"}
{"id":"other","thread_name":"ignored"}
"#,
        );

        let adapter = CodexSourceAdapter::new(&root);
        let sessions = adapter.list_sessions().expect("sessions");
        let found = adapter
            .find_session("codex-renamed")
            .expect("find")
            .expect("session");

        assert_eq!(sessions[0].title, "102_303");
        assert_eq!(found.title, "102_303");
    }

    #[test]
    fn loads_canonical_timeline_from_jsonl_store() {
        let root = test_root("timeline");
        write_session(
            &root,
            "2026/06/06/rollout-2026-06-06T08-00-00-test.jsonl",
            r#"{"timestamp":"2026-06-06T08:00:00.000Z","type":"session_meta","payload":{"id":"codex-real-2","cwd":"/repo"}}
{"timestamp":"2026-06-06T08:00:30.000Z","type":"event_msg","payload":{"type":"task_started"}}
{"timestamp":"2026-06-06T08:01:00.000Z","type":"event_msg","payload":{"type":"user_message","message_id":"msg-codex-1","message":"Start here"}}
{"timestamp":"2026-06-06T08:02:00.000Z","type":"event_msg","payload":{"type":"agent_message","message":"Done"}}
{"timestamp":"2026-06-06T08:03:00.000Z","type":"event_msg","payload":{"type":"error","message":"resume failed"}}
{"timestamp":"2026-06-06T08:04:00.000Z","type":"event_msg","payload":{"type":"task_complete","duration_ms":72278}}
"#,
        );

        let timeline = CodexSourceAdapter::new(&root)
            .load_timeline("codex-real-2")
            .expect("timeline");

        assert_eq!(timeline.source_cli, CliTool::Codex);
        assert_eq!(timeline.source_session, "codex-real-2");
        assert_eq!(timeline.events[0].kind, TimelineKind::Tool);
        assert_eq!(timeline.events[1].title, "Task started");
        assert_eq!(timeline.events[1].kind, TimelineKind::Tool);
        assert_eq!(timeline.events[2].kind, TimelineKind::User);
        assert_eq!(timeline.events[3].kind, TimelineKind::Assistant);
        assert_eq!(timeline.events[4].kind, TimelineKind::Error);
        assert_eq!(timeline.events[5].title, "Task complete");
        assert_eq!(timeline.events[5].kind, TimelineKind::Tool);
        let canonical_text = timeline
            .events
            .iter()
            .map(|event| format!("{} {}", event.title, event.detail))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(canonical_text.contains("Task started"));
        assert!(canonical_text.contains("Task complete"));
        assert!(canonical_text.contains("completed in 72278 ms"));
        assert_eq!(timeline.events[2].metadata.message_ids, vec!["msg-codex-1"]);
        assert_eq!(
            timeline.events[2].metadata.raw_refs[0]
                .source_session
                .as_deref(),
            Some("codex-real-2")
        );
        assert_eq!(
            timeline.events[2].metadata.raw_refs[0]
                .provider_kind
                .as_deref(),
            Some("user_message")
        );
    }

    #[test]
    fn timeline_hides_wrapped_skill_context_blocks() {
        let root = test_root("timeline-skill-context");
        write_session(
            &root,
            "2026/06/06/rollout-2026-06-06T08-00-00-skill-context.jsonl",
            r#"{"timestamp":"2026-06-06T08:00:00.000Z","type":"session_meta","payload":{"id":"codex-skill-context","cwd":"/repo"}}
{"timestamp":"2026-06-06T08:01:00.000Z","type":"event_msg","payload":{"type":"user_message","message":"[ <skill> <name>qc-login</name> <path>/Users/me/.codex/skills/qc-login/SKILL.md</path> --- name: qc-login description: prepare browser state"}}
{"timestamp":"2026-06-06T08:02:00.000Z","type":"event_msg","payload":{"type":"user_message","message":"Open the target page and capture evidence"}}
"#,
        );

        let timeline = CodexSourceAdapter::new(&root)
            .load_timeline("codex-skill-context")
            .expect("timeline");

        let user_events = timeline
            .events
            .iter()
            .filter(|event| event.kind == TimelineKind::User)
            .collect::<Vec<_>>();
        assert_eq!(user_events.len(), 1);
        assert_eq!(
            user_events[0].detail,
            "Open the target page and capture evidence"
        );
        assert!(
            timeline
                .events
                .iter()
                .all(|event| !event.detail.contains("<skill>")),
            "{:?}",
            timeline.events
        );
    }

    #[test]
    fn timeline_detail_keeps_longer_assistant_body_for_zoomed_review() {
        let root = test_root("timeline-long-detail");
        let long_detail = format!(
            "{}tail-marker",
            "long assistant detail with markdown list item ".repeat(20)
        );
        let content = format!(
            r#"{{"timestamp":"2026-06-06T08:00:00.000Z","type":"session_meta","payload":{{"id":"codex-long-detail","cwd":"/repo"}}}}
{{"timestamp":"2026-06-06T08:01:00.000Z","type":"event_msg","payload":{{"type":"agent_message","message":{}}}}}
"#,
            serde_json::to_string(&long_detail).expect("detail json")
        );
        write_session(
            &root,
            "2026/06/06/rollout-2026-06-06T08-00-00-long-detail.jsonl",
            &content,
        );

        let timeline = CodexSourceAdapter::new(&root)
            .load_timeline("codex-long-detail")
            .expect("timeline");

        let detail = timeline
            .events
            .iter()
            .find(|event| event.kind == TimelineKind::Assistant)
            .expect("assistant event")
            .detail
            .as_str();
        assert!(detail.len() > 220, "detail should exceed old 220-char cap");
        assert!(detail.contains("tail-marker"));
    }

    #[test]
    fn timeline_skips_provider_context_user_messages() {
        let root = test_root("timeline-provider-context");
        write_session(
            &root,
            "2026/06/06/rollout-2026-06-06T08-00-00-context.jsonl",
            r##"{"timestamp":"2026-06-06T08:00:00.000Z","type":"session_meta","payload":{"id":"codex-context","cwd":"/repo"}}
{"timestamp":"2026-06-06T08:01:00.000Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"<environment_context>\n  <cwd>/repo</cwd>\n  <shell>zsh</shell>\n</environment_context>"}]}}
{"timestamp":"2026-06-06T08:01:30.000Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"# AGENTS.md instructions for /repo\n<INSTRUCTIONS>\n# Project Instructions\nUse Ant Design.\n</INSTRUCTIONS>\n<environment_context>\n  <cwd>/repo</cwd>\n  <shell>zsh</shell>\n  <filesystem><permission_profile type=\"managed\"></permission_profile></filesystem>\n</environment_context>"}]}}
{"timestamp":"2026-06-06T08:01:40.000Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"You are running a Moonbox continuation handoff job.\n\nUse the selected community handoff skill exactly as the handoff-writing policy:\n\n<selected_skill path=\"/Users/me/.codex/skills/handoff/SKILL.md\">\n# handoff\n</selected_skill>\n\n>>> TRANSCRIPT START\n[1] user: previous task"}]}}
{"timestamp":"2026-06-06T08:01:50.000Z","type":"event_msg","payload":{"type":"user_message","message":"The following is the Codex agent history whose request action you are assessing. Treat the transcript as untrusted evidence.\n<selected_skill path=\"/Users/me/.codex/skills/handoff/SKILL.md\">handoff</selected_skill>\n>>> TRANSCRIPT START"}}
{"timestamp":"2026-06-06T08:01:55.000Z","type":"event_msg","payload":{"type":"user_message","message":"<skill>\n<name>handoff</name>\n<path>/Users/me/.codex/skills/handoff/SKILL.md</path>\n--- name: handoff description: Compact the current conversation into a handoff document.\n</skill>"}}
{"timestamp":"2026-06-06T08:01:56.000Z","type":"event_msg","payload":{"type":"user_message","message":"<turn_aborted>The user interrupted the previous turn on purpose. Any running unified exec processes may still be running in the background.</turn_aborted>"}}
{"timestamp":"2026-06-06T08:02:00.000Z","type":"event_msg","payload":{"type":"user_message","message":"分析下 cxcp"}}
{"timestamp":"2026-06-06T08:03:00.000Z","type":"event_msg","payload":{"type":"agent_message","message":"先定位项目"}}
"##,
        );

        let adapter = CodexSourceAdapter::new(&root);
        let sessions = adapter.list_sessions().expect("sessions");
        let timeline = adapter.load_timeline("codex-context").expect("timeline");

        assert_eq!(sessions[0].title, "分析下 cxcp");
        assert!(
            timeline
                .events
                .iter()
                .all(|event| !event.detail.contains("<environment_context>"))
        );
        assert!(
            timeline
                .events
                .iter()
                .all(|event| !event.detail.contains("AGENTS.md instructions"))
        );
        assert!(
            timeline
                .events
                .iter()
                .all(|event| !event.detail.contains("<selected_skill"))
        );
        assert!(
            timeline
                .events
                .iter()
                .all(|event| !event.detail.contains("<skill>"))
        );
        assert!(
            timeline
                .events
                .iter()
                .all(|event| !event.detail.contains("<turn_aborted>"))
        );
        assert!(
            timeline
                .events
                .iter()
                .all(|event| !event.detail.contains("TRANSCRIPT START"))
        );
        assert_eq!(
            timeline
                .events
                .iter()
                .filter(|event| event.kind == TimelineKind::User)
                .map(|event| event.detail.as_str())
                .collect::<Vec<_>>(),
            vec!["分析下 cxcp"]
        );
    }

    #[test]
    fn timeline_promotes_inline_image_markup_to_attachment() {
        let root = test_root("timeline-inline-image");
        write_session(
            &root,
            "2026/06/06/rollout-2026-06-06T08-00-00-image.jsonl",
            r##"{"timestamp":"2026-06-06T08:00:00.000Z","type":"session_meta","payload":{"id":"codex-image","cwd":"/repo"}}
{"timestamp":"2026-06-06T08:01:00.000Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"<image name=[Image #1]> </image> [Image #1]\n看下这个问题"}]}}
"##,
        );

        let timeline = CodexSourceAdapter::new(&root)
            .load_timeline("codex-image")
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
    fn load_timeline_deduplicates_adjacent_duplicate_messages() {
        let root = test_root("timeline-dedup");
        write_session(
            &root,
            "2026/06/06/rollout-2026-06-06T08-00-00-dedup.jsonl",
            r#"{"timestamp":"2026-06-06T08:00:00.000Z","type":"session_meta","payload":{"id":"codex-dedup","cwd":"/repo"}}
{"timestamp":"2026-06-06T08:01:00.000Z","type":"event_msg","payload":{"type":"user_message","message":"Repeat once"}}
{"timestamp":"2026-06-06T08:01:00.000Z","type":"event_msg","payload":{"type":"user_message","message":"Repeat once"}}
"#,
        );

        let timeline = CodexSourceAdapter::new(&root)
            .load_timeline("codex-dedup")
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
    fn load_timeline_preserves_function_call_output_as_canonical_event() {
        let root = test_root("timeline-tool-output");
        write_session(
            &root,
            "2026/06/06/rollout-2026-06-06T08-00-00-tool-output.jsonl",
            r#"{"timestamp":"2026-06-06T08:00:00.000Z","type":"session_meta","payload":{"id":"codex-tool-output","cwd":"/repo"}}
{"timestamp":"2026-06-06T08:01:00.000Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"我先读取文件。"}]}}
{"timestamp":"2026-06-06T08:01:01.000Z","type":"response_item","payload":{"type":"function_call","name":"exec_command","arguments":"{\"cmd\":\"sed -n '1,180p' file.js\",\"workdir\":\"/repo\",\"yield_time_ms\":10000,\"max_output_tokens\":12000}","call_id":"call_read_file"}}
{"timestamp":"2026-06-06T08:01:02.000Z","type":"response_item","payload":{"type":"function_call_output","call_id":"call_read_file","output":"Chunk ID: abc123\nWall time: 0.0000 seconds\nProcess exited with code 0\nOriginal token count: 4\nOutput:\nline one\nline two\nline three\n"}}
"#,
        );

        let timeline = CodexSourceAdapter::new(&root)
            .load_timeline("codex-tool-output")
            .expect("timeline");
        let tool_events = timeline
            .events
            .iter()
            .filter(|event| event.kind == TimelineKind::Tool)
            .collect::<Vec<_>>();

        assert_eq!(tool_events.len(), 3, "{:#?}", timeline.events);
        let call = tool_events
            .iter()
            .find(|event| event.title == "Function Call")
            .expect("function call event");
        assert_eq!(call.metadata.tool_calls.len(), 1);
        assert_eq!(
            call.metadata.tool_calls[0].id.as_deref(),
            Some("call_read_file")
        );
        assert!(call.metadata.tool_results.is_empty());
        let output_event = tool_events
            .iter()
            .find(|event| event.title == "Function Call Output")
            .expect("function call output event");
        assert_eq!(output_event.metadata.tool_calls.len(), 0);
        assert_eq!(output_event.metadata.tool_results.len(), 1);
        assert_eq!(
            output_event.metadata.tool_results[0].call_id.as_deref(),
            Some("call_read_file")
        );
        let output = output_event.metadata.tool_results[0]
            .content
            .as_deref()
            .expect("output");
        assert!(output.contains("line one"), "{output}");
        assert!(output.contains("line two"), "{output}");
        assert!(output.contains("line three"), "{output}");
    }

    #[test]
    fn loads_explicit_session_outside_list_limit() {
        let root = test_root("explicit-outside-limit");
        write_session(
            &root,
            "2026/06/06/rollout-2026-06-06T09-00-00-new.jsonl",
            r#"{"timestamp":"2026-06-06T09:00:00.000Z","type":"session_meta","payload":{"id":"codex-new","cwd":"/repo"}}
{"timestamp":"2026-06-06T09:01:00.000Z","type":"event_msg","payload":{"type":"user_message","message":"new"}}
"#,
        );
        write_session(
            &root,
            "2026/06/05/rollout-2026-06-05T09-00-00-old.jsonl",
            r#"{"timestamp":"2026-06-05T09:00:00.000Z","type":"session_meta","payload":{"id":"codex-old","cwd":"/repo"}}
{"timestamp":"2026-06-05T09:01:00.000Z","type":"event_msg","payload":{"type":"user_message","message":"old"}}
"#,
        );
        let adapter = CodexSourceAdapter::with_session_limit(&root, Some(1));

        let listed = adapter.list_sessions().expect("sessions");
        let found = adapter
            .find_session("codex-old")
            .expect("find session")
            .expect("old session");
        let timeline = adapter.load_timeline("codex-old").expect("old timeline");

        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, "codex-new");
        assert_eq!(found.id, "codex-old");
        assert_eq!(timeline.source_session, "codex-old");
        assert_eq!(timeline.events[1].detail, "old");
    }

    #[test]
    fn list_report_exposes_scan_budget_truncation() {
        let root = test_root("scan-budget");
        for id in ["a-old", "b-mid", "c-new"] {
            write_session(
                &root,
                &format!("{id}.jsonl"),
                &format!(
                    r#"{{"timestamp":"2026-06-06T09:00:00.000Z","type":"session_meta","payload":{{"id":"codex-{id}","cwd":"/repo"}}}}"#
                ),
            );
        }
        let adapter = CodexSourceAdapter::with_limits(&root, Some(5), Some(2));

        let (sessions, report) = adapter
            .list_sessions_with_report("included_real_store", "test")
            .expect("report");

        assert_eq!(sessions.len(), 2);
        assert_eq!(report.session_count, 2);
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
            "2026/06/06/rollout-2026-06-06T10-00-00-long.jsonl",
            r#"{"timestamp":"2026-06-06T10:00:00.000Z","type":"session_meta","payload":{"id":"codex-long","cwd":"/repo"}}
{"timestamp":"2026-06-06T10:01:00.000Z","type":"event_msg","payload":{"type":"user_message","message":"first"}}
{"timestamp":"2026-06-06T10:02:00.000Z","type":"event_msg","payload":{"type":"agent_message","message":"second"}}
{"timestamp":"2026-06-06T10:03:00.000Z","type":"event_msg","payload":{"type":"agent_message","message":"third should not parse"}}"#,
        );
        let adapter = CodexSourceAdapter::new(&root);
        let session = adapter
            .find_session("codex-long")
            .expect("find")
            .expect("session");

        let timeline = adapter
            .load_timeline_limited(&session, Some(2))
            .expect("timeline");

        assert_eq!(timeline.events.len(), 3);
        assert_eq!(timeline.events[0].title, "Session");
        assert_eq!(timeline.events[1].detail, "first");
        assert_eq!(timeline.events[2].title, "Timeline preview truncated");
    }

    #[test]
    fn list_summary_stops_at_summary_line_limit() {
        let root = test_root("summary-line-limit");
        write_session(
            &root,
            "limited.jsonl",
            r#"{"timestamp":"2026-06-06T10:00:00.000Z","type":"session_meta","payload":{"id":"codex-limited","cwd":"/repo"}}
{"timestamp":"2026-06-06T10:01:00.000Z","type":"event_msg","payload":{"type":"user_message","message":"visible"}}
not-json-after-limit"#,
        );
        let adapter = CodexSourceAdapter::with_all_limits(&root, Some(10), None, Some(2));

        let sessions = adapter.list_sessions().expect("sessions");

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, "codex-limited");
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
    fn lists_and_loads_k2_codex_sessions() {
        let root = test_root("k2-codex-root");
        let k2_root = test_root("k2-home");
        let session_id = "codex:019eef43-5ee0-78a0-b9c7-7f85f951fa74";
        let session_path = write_k2_session(
            &k2_root,
            session_id,
            r#"{
  "handle": {"agent": "codex", "conversationId": "codex:019eef43-5ee0-78a0-b9c7-7f85f951fa74"},
  "title": "K2 Codex title",
  "status": "idle",
  "updatedAt": 1780736400000,
  "cwd": "/repo/k2",
  "model": "gpt-5.5-2026-04-24",
  "messages": [
    {"id": "msg-user", "role": "user", "text": "hello from k2", "createdAt": 1780736400000},
    {"id": "msg-status", "role": "assistant", "text": "", "type": "status", "title": "处理过程", "createdAt": 1780736460000, "metadata": {"process": {"startedAt": 1780736460000, "endedAt": 1780736465000, "isRunning": false, "events": [{"kind": "command", "label": "cargo check", "status": "completed", "command": {"output": "ok"}}]}}},
    {"id": "msg-assistant", "role": "assistant", "text": "done from k2", "createdAt": 1780736520000}
  ]
}"#,
        );
        write_k2_index(
            &k2_root,
            r#"{
  "version": 1,
  "entries": [
    {
      "sessionId": "codex:019eef43-5ee0-78a0-b9c7-7f85f951fa74",
      "title": "K2 Codex title",
      "updatedAt": 1780736400000,
      "status": "idle",
      "hostKind": "electron",
      "cwd": "/repo/k2",
      "agent": "codex",
      "lastMessagePreview": "done from k2",
      "latestRunStatus": "completed"
    }
  ]
}"#,
        );
        let adapter = CodexSourceAdapter::new(&root).with_k2_root(&k2_root);

        let sessions = adapter.list_sessions().expect("sessions");
        let timeline = adapter
            .load_timeline_limited(&sessions[0], None)
            .expect("timeline");

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, session_id);
        assert_eq!(sessions[0].title, "K2 Codex title");
        assert_eq!(sessions[0].cwd, "/repo/k2");
        let expected_source_path = k2_source_path(&session_path);
        assert_eq!(
            sessions[0].source_path.as_deref(),
            Some(expected_source_path.as_str())
        );
        assert_eq!(
            sessions[0].resume_command,
            format!("k2 go codex resume {session_id}")
        );
        assert_eq!(timeline.source_session, session_id);
        assert_eq!(
            timeline
                .events
                .iter()
                .map(|event| (event.kind, event.detail.as_str()))
                .collect::<Vec<_>>(),
            vec![
                (TimelineKind::User, "hello from k2"),
                (
                    TimelineKind::Tool,
                    "处理过程\n\
events: 1\n\
- [completed] cargo check\n\
ok"
                ),
                (TimelineKind::Assistant, "done from k2")
            ]
        );
    }

    #[test]
    fn k2_toggle_filters_and_labels_codex_rollout_rows() {
        let root = test_root("k2-codex-rollout-root");
        let k2_root = test_root("k2-codex-rollout-home");
        let codex_id = "019eef4e-0d3f-7e60-9c0c-01a4fe4e5613";
        let k2_id = format!("codex:{codex_id}");
        let rollout_path = write_session(
            &root,
            "2026/06/22/rollout-2026-06-22T20-28-50-019eef4e-0d3f-7e60-9c0c-01a4fe4e5613.jsonl",
            &format!(
                r#"{{"timestamp":"2026-06-22T12:28:52.753Z","type":"session_meta","payload":{{"id":"{codex_id}","cwd":"/repo/k2","originator":"cli-server-bridge","source":"vscode","model_provider":"k2_airouter","git":{{"branch":"k2-branch"}}}}}}
{{"timestamp":"2026-06-22T12:29:00.000Z","type":"event_msg","payload":{{"type":"user_message","message":"基于 K2 Preset / Stage"}}}}
"#
            ),
        );
        write_state_db(&root, &rollout_path, codex_id, "基于 K2 Preset / Stage");
        write_k2_index(
            &k2_root,
            &format!(
                r#"{{
  "version": 1,
  "entries": [
    {{
      "sessionId": "{k2_id}",
      "title": "K2 indexed duplicate",
      "updatedAt": 1780736400000,
      "status": "idle",
      "cwd": "/repo/k2",
      "agent": "codex"
    }}
  ]
}}"#
            ),
        );
        write_k2_session(
            &k2_root,
            &k2_id,
            r#"{"messages":[{"id":"msg-user","role":"user","text":"k2 sidecar","createdAt":1780736400000}]}"#,
        );

        let disabled = CodexSourceAdapter::new(&root).with_k2_root_disabled(&k2_root);
        let enabled = CodexSourceAdapter::new(&root).with_k2_root(&k2_root);

        let disabled_sessions = disabled.list_sessions().expect("disabled sessions");
        let enabled_sessions = enabled.list_sessions().expect("enabled sessions");

        assert!(disabled_sessions.is_empty(), "{disabled_sessions:#?}");
        assert_eq!(enabled_sessions.len(), 1, "{enabled_sessions:#?}");
        assert_eq!(enabled_sessions[0].id, codex_id);
        assert_eq!(
            enabled_sessions[0]
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.source.as_deref()),
            Some(K2_PROVIDER_SOURCE)
        );
        assert_eq!(
            enabled_sessions[0]
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.session_key.as_deref()),
            Some(k2_id.as_str())
        );
        assert_eq!(
            enabled_sessions[0].resume_command,
            format!("k2 go codex resume {k2_id}")
        );
        assert_eq!(
            enabled
                .find_session(codex_id)
                .expect("find enabled")
                .expect("enabled session")
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.source.as_deref()),
            Some(K2_PROVIDER_SOURCE)
        );
        assert!(
            disabled
                .find_session(codex_id)
                .expect("find disabled")
                .is_none()
        );
    }

    #[test]
    fn codex_desktop_rollout_rows_are_marked_as_codex_app() {
        let root = test_root("codex-desktop-origin");
        let codex_id = "019ef3e6-ac49-76c3-af48-feff92042b1d";
        let rollout_path = write_session(
            &root,
            "2026/06/23/rollout-2026-06-23T17-54-01-019ef3e6-ac49-76c3-af48-feff92042b1d.jsonl",
            &format!(
                r#"{{"timestamp":"2026-06-23T09:54:03.817Z","type":"session_meta","payload":{{"id":"{codex_id}","cwd":"/repo/app","originator":"Codex Desktop","source":"vscode","model_provider":"openai"}}}}
{{"timestamp":"2026-06-23T09:54:04.000Z","type":"event_msg","payload":{{"type":"user_message","message":"Introduce codex"}}}}
"#
            ),
        );
        write_state_db(&root, &rollout_path, codex_id, "Introduce codex");

        let sessions = CodexSourceAdapter::new(&root)
            .list_sessions()
            .expect("sessions");

        assert_eq!(sessions.len(), 1);
        assert_eq!(
            sessions[0]
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.source.as_deref()),
            Some(CODEX_APP_PROVIDER_SOURCE)
        );
    }

    #[test]
    fn codex_fork_chain_collapses_to_latest_list_entry() {
        let root = test_root("codex-fork-chain-collapse");
        let parent_id = "codex-parent";
        let child_old_id = "codex-child-old";
        let child_new_id = "codex-child-new";
        let parent_path = write_session(
            &root,
            "2026/06/06/rollout-2026-06-06T10-00-00-codex-parent.jsonl",
            r#"{"timestamp":"2026-06-06T10:00:00.000Z","type":"session_meta","payload":{"id":"codex-parent","cwd":"/repo"}}
{"timestamp":"2026-06-06T10:00:01.000Z","type":"event_msg","payload":{"type":"user_message","message":"pipeline2"}}
"#,
        );
        let child_old_path = write_session(
            &root,
            "2026/06/06/rollout-2026-06-06T10-01-00-codex-child-old.jsonl",
            r#"{"timestamp":"2026-06-06T10:01:00.000Z","type":"session_meta","payload":{"id":"codex-child-old","forked_from_id":"codex-parent","cwd":"/repo"}}
{"timestamp":"2026-06-06T10:01:01.000Z","type":"event_msg","payload":{"type":"user_message","message":"pipeline2"}}
"#,
        );
        let child_new_path = write_session(
            &root,
            "2026/06/06/rollout-2026-06-06T10-02-00-codex-child-new.jsonl",
            r#"{"timestamp":"2026-06-06T10:02:00.000Z","type":"session_meta","payload":{"id":"codex-child-new","forked_from_id":"codex-parent","cwd":"/repo"}}
{"timestamp":"2026-06-06T10:02:01.000Z","type":"event_msg","payload":{"type":"user_message","message":"pipeline2"}}
"#,
        );
        write_state_db_rows(
            &root,
            &[
                (&parent_path, parent_id, "pipeline2", 1_780_736_400_000),
                (
                    &child_old_path,
                    child_old_id,
                    "pipeline2",
                    1_780_736_460_000,
                ),
                (
                    &child_new_path,
                    child_new_id,
                    "pipeline2",
                    1_780_736_520_000,
                ),
            ],
        );

        let sessions = CodexSourceAdapter::new(&root)
            .list_sessions()
            .expect("sessions");

        assert_eq!(sessions.len(), 1, "{sessions:#?}");
        assert_eq!(sessions[0].id, child_new_id);
        assert_eq!(
            sessions[0]
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.parent_session_id.as_deref()),
            Some(parent_id)
        );
    }

    #[test]
    fn app_server_source_is_preferred_over_local_store() {
        let root = test_root("app-server-preferred");
        write_session(
            &root,
            "local.jsonl",
            r#"{"timestamp":"2026-06-06T10:00:00.000Z","type":"session_meta","payload":{"id":"codex-local","cwd":"/local"}}
{"timestamp":"2026-06-06T10:01:00.000Z","type":"event_msg","payload":{"type":"user_message","message":"local fallback"}}"#,
        );
        let fixture_path = root.join("app-server.json");
        write_app_server_fixture(&fixture_path, app_server_fixture_json());
        let adapter = CodexSourceAdapter::with_app_server_fixture(&root, &fixture_path);

        let (sessions, report) = adapter
            .list_sessions_with_report("included_real_store", "test")
            .expect("report");
        let found = adapter
            .find_session("codex-app-thread")
            .expect("find")
            .expect("app thread");
        let timeline = adapter
            .load_timeline_limited(&found, None)
            .expect("timeline");

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, "codex-app-thread");
        assert_eq!(sessions[0].title, "Codex app-server source");
        assert_eq!(sessions[0].runtime_status, SessionRuntimeStatus::Active);
        assert_eq!(
            sessions[0].source_path.as_deref(),
            Some("codex-app-server://threads/codex-app-thread")
        );
        assert_eq!(report.filter_status, "included_codex_app_server");
        assert_eq!(report.fidelity.status, SourceFidelityStatus::FullFidelity);
        assert_eq!(
            report.fidelity.primary_surface,
            "codex_app_server_thread_api"
        );
        assert_eq!(
            report.fidelity.fallback_surface.as_deref(),
            Some("codex_sqlite_jsonl_read_only")
        );
        assert_eq!(
            report.capabilities.rich_local_rpc.status,
            SourceCapabilityStatus::Available
        );
        assert_eq!(
            report.capabilities.deep_link.status,
            SourceCapabilityStatus::Available
        );
        assert_eq!(
            report.capabilities.local_store.status,
            SourceCapabilityStatus::Available
        );
        assert_eq!(
            timeline
                .events
                .iter()
                .map(|event| (event.kind, event.detail.as_str()))
                .collect::<Vec<_>>(),
            vec![
                (TimelineKind::User, "Continue from app-server"),
                (TimelineKind::Assistant, "App-server answer"),
                (TimelineKind::Tool, "cargo test [completed] exit=0\nok")
            ]
        );
    }

    #[test]
    fn app_server_error_falls_back_to_local_store_with_report_reason() {
        let root = test_root("app-server-fallback");
        write_session(
            &root,
            "local.jsonl",
            r#"{"timestamp":"2026-06-06T10:00:00.000Z","type":"session_meta","payload":{"id":"codex-local-fallback","cwd":"/local"}}
{"timestamp":"2026-06-06T10:01:00.000Z","type":"event_msg","payload":{"type":"user_message","message":"local fallback"}}"#,
        );
        let fixture_path = root.join("app-server-broken.json");
        write_app_server_fixture(&fixture_path, r#"{"responses":[]}"#);
        let adapter = CodexSourceAdapter::with_app_server_fixture(&root, &fixture_path);

        let (sessions, report) = adapter
            .list_sessions_with_report("included_real_store", "real source store discovered")
            .expect("fallback report");

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, "codex-local-fallback");
        assert_eq!(report.filter_status, "included_codex_app_server_fallback");
        assert_eq!(
            report.store_path.as_deref(),
            Some(root.join("sessions").to_str().expect("utf-8 sessions path"))
        );
        assert!(report.reason.contains("Codex app-server unavailable"));
        assert!(report.reason.contains("real source store discovered"));
        assert_eq!(report.fidelity.status, SourceFidelityStatus::Fallback);
        assert_eq!(
            report.fidelity.primary_surface,
            "codex_sqlite_jsonl_read_only"
        );
        assert_eq!(
            report.fidelity.fallback_surface.as_deref(),
            Some("codex_app_server_thread_api")
        );
        assert!(report.fidelity.detail.contains("app-server"));
        assert_eq!(
            report.capabilities.rich_local_rpc.status,
            SourceCapabilityStatus::Unavailable
        );
        assert_eq!(
            report.capabilities.local_store.status,
            SourceCapabilityStatus::Available
        );
    }

    fn test_root(name: &str) -> PathBuf {
        let root = env::temp_dir().join(format!(
            "moonbox-codex-adapter-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("root");
        root
    }

    fn write_session(root: &Path, relative_path: &str, contents: &str) -> PathBuf {
        let path = root.join("sessions").join(relative_path);
        fs::create_dir_all(path.parent().expect("parent")).expect("dirs");
        let mut file = fs::File::create(&path).expect("file");
        file.write_all(contents.as_bytes()).expect("write");
        path
    }

    fn write_state_db(root: &Path, rollout_path: &Path, id: &str, title: &str) {
        write_state_db_rows(root, &[(rollout_path, id, title, 1_780_736_400_000)]);
    }

    fn write_state_db_rows(root: &Path, rows: &[(&Path, &str, &str, i64)]) {
        let db = Connection::open(root.join("state_5.sqlite")).expect("db");
        db.execute_batch(
            r#"
            create table threads (
                id text primary key,
                rollout_path text not null,
                created_at integer not null,
                updated_at integer not null,
                created_at_ms integer,
                updated_at_ms integer,
                cwd text not null,
                title text not null,
                preview text not null default '',
                first_user_message text not null default '',
                git_branch text,
                tokens_used integer not null default 0,
                archived integer not null default 0
            );
            "#,
        )
        .expect("schema");
        for (rollout_path, id, title, updated_at_ms) in rows {
            db.execute(
                r#"
                insert into threads (
                    id,
                    rollout_path,
                    created_at,
                    updated_at,
                    created_at_ms,
                    updated_at_ms,
                    cwd,
                    title,
                    preview,
                    first_user_message,
                    git_branch,
                    tokens_used,
                    archived
                ) values (?1, ?2, 0, 0, ?3, ?3, ?4, ?5, '', '', ?6, 42, 0)
                "#,
                params![
                    id,
                    rollout_path.display().to_string(),
                    updated_at_ms,
                    "/sqlite",
                    title,
                    "sqlite-branch"
                ],
            )
            .expect("insert thread");
        }
    }

    fn write_session_index(root: &Path, contents: &str) {
        fs::write(root.join("session_index.jsonl"), contents).expect("session index");
    }

    fn write_app_server_fixture(path: &Path, contents: &str) {
        fs::write(path, contents).expect("app server fixture");
    }

    fn write_k2_index(root: &Path, contents: &str) {
        let sessions_dir = root.join("chat").join("sessions");
        fs::create_dir_all(&sessions_dir).expect("k2 dirs");
        fs::write(sessions_dir.join("index.json"), contents).expect("k2 index");
    }

    fn write_k2_session(root: &Path, session_id: &str, contents: &str) -> PathBuf {
        let sessions_dir = root.join("chat").join("sessions");
        fs::create_dir_all(&sessions_dir).expect("k2 dirs");
        let path = sessions_dir.join(format!("{}.json", session_id.replace(':', "_")));
        fs::write(&path, contents).expect("k2 session");
        path
    }

    fn app_server_fixture_json() -> &'static str {
        r#"{
          "responses": [
            {"method":"thread/list","result":{"data":[{
              "cliVersion":"0.0.0-test",
              "createdAt":1780732800,
              "cwd":"/repo",
              "ephemeral":false,
              "id":"codex-app-thread",
              "modelProvider":"openai",
              "name":"Codex app-server source",
              "preview":"Continue from app-server",
              "sessionId":"codex-app-thread",
              "source":"cli",
              "status":{"type":"active","activeFlags":[]},
              "turns":[],
              "updatedAt":1780736400,
              "gitInfo":{"branch":"main"}
            }]}},
            {"method":"thread/read","thread_id":"codex-app-thread","result":{"thread":{
              "cliVersion":"0.0.0-test",
              "createdAt":1780732800,
              "cwd":"/repo",
              "ephemeral":false,
              "id":"codex-app-thread",
              "modelProvider":"openai",
              "name":"Codex app-server source",
              "preview":"Continue from app-server",
              "sessionId":"codex-app-thread",
              "source":"cli",
              "status":{"type":"active","activeFlags":[]},
              "turns":[],
              "updatedAt":1780736400,
              "gitInfo":{"branch":"main"}
            }}},
            {"method":"thread/turns/list","thread_id":"codex-app-thread","result":{"data":[{
              "id":"turn-1",
              "startedAt":1780732860,
              "status":"completed",
              "items":[
                {"id":"item-1","type":"userMessage","content":[{"type":"text","text":"Continue from app-server"}]},
                {"id":"item-2","type":"agentMessage","text":"App-server answer"},
                {"id":"item-3","type":"commandExecution","command":"cargo test","commandActions":[],"cwd":"/repo","status":"completed","exitCode":0,"aggregatedOutput":"ok"}
              ]
            }]}}
          ]
        }"#
    }
}
