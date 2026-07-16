use std::{
    collections::{HashMap, HashSet},
    env, fs,
    path::{Path, PathBuf},
};

use rusqlite::{Connection, MappedRows, OpenFlags, OptionalExtension, Row, named_params, params};
use serde::Deserialize;
use serde_json::Value;

use super::{
    adapter::{
        AdapterError, SourceAdapter, SourceReportMeta, SourceScanStats,
        report_from_sessions_with_scan,
    },
    local_jsonl::{
        configured_session_limit, display_time, event_id, human_timestamp,
        is_moonbox_handoff_control_text, is_provider_context_text, push_timeline_event, read_error,
        stable_text_digest, text_from_value, timeline_preview_truncated_event, title_case,
        truncate, truncate_timeline_detail,
    },
    model::{
        CanonicalTimeline, CliTool, ContextHealth, EvidenceConfidence, ProviderContinuationPoint,
        ProviderHandoffMetadata, ProviderScrollContext, ProviderSearchMetadata,
        ProviderSessionMetadata, SessionRuntimeStatus, SessionStatus, SessionSummary,
        SourceFidelity, SourceFidelityStatus, SourceProvenance, TimelineEvent,
        TimelineEventMetadata, TimelineEventRawRef, TimelineKind, TimelineToolCall, TokenBreakdown,
        unknown_runtime_reason,
    },
    model_context::resolve_model_context_window,
};

const HERMES_TOOL: CliTool = CliTool::Hermes;

#[derive(Debug, Clone)]
pub struct HermesSourceAdapter {
    root: PathBuf,
    list_limit: Option<usize>,
}

#[derive(Debug, Clone)]
struct HermesSessionRow {
    id: String,
    source: String,
    user_id: Option<String>,
    model: Option<String>,
    model_config: Option<String>,
    system_prompt: Option<String>,
    parent_session_id: Option<String>,
    updated_at: String,
    end_reason: Option<String>,
    message_count: usize,
    tool_call_count: usize,
    input_tokens: usize,
    output_tokens: usize,
    cache_read_tokens: usize,
    cache_write_tokens: usize,
    reasoning_tokens: usize,
    cwd: Option<String>,
    title: Option<String>,
    handoff_state: Option<String>,
    handoff_platform: Option<String>,
    handoff_error: Option<String>,
    rewind_count: usize,
    compact_count: usize,
    archived: bool,
    active_message_count: usize,
    preview: String,
}

#[derive(Debug, Clone)]
struct HermesMessageRow {
    message_id: String,
    platform_message_id: Option<String>,
    role: String,
    content: Option<String>,
    tool_calls: Option<String>,
    tool_name: Option<String>,
    timestamp: String,
    token_count: Option<usize>,
    finish_reason: Option<String>,
    reasoning: Option<String>,
    reasoning_content: Option<String>,
    reasoning_details: Option<String>,
}

#[derive(Debug, Clone)]
struct HermesContinuationRow {
    message_id: String,
    role: String,
    timestamp: String,
    detail: String,
    message_index: usize,
    total_messages: usize,
    before_message_id: Option<String>,
    bookend_before: Option<String>,
    after_message_id: Option<String>,
    bookend_after: Option<String>,
}

#[derive(Debug, Clone)]
struct HermesContinuationExport {
    search: ProviderSearchMetadata,
    points: Vec<ProviderContinuationPoint>,
}

#[derive(Debug, Clone, Deserialize)]
struct SessionRegistryEntry {
    session_id: String,
    session_key: Option<String>,
    display_name: Option<String>,
    platform: Option<String>,
    chat_type: Option<String>,
    total_tokens: Option<usize>,
    suspended: Option<bool>,
    resume_pending: Option<bool>,
    expiry_finalized: Option<bool>,
    #[serde(default)]
    origin: Value,
}

impl HermesSourceAdapter {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            list_limit: configured_session_limit(),
        }
    }

    #[cfg(test)]
    fn with_session_limit(root: impl Into<PathBuf>, list_limit: Option<usize>) -> Self {
        Self {
            root: root.into(),
            list_limit,
        }
    }

    #[cfg(not(test))]
    pub fn from_default_home() -> Option<Self> {
        if let Some(path) = env::var_os("MOONBOX_HERMES_HOME") {
            return Some(Self::new(path));
        }
        if let Some(path) = env::var_os("HERMES_HOME") {
            return Some(Self::new(path));
        }
        env::var_os("HOME").map(|home| Self::new(PathBuf::from(home).join(".hermes")))
    }

    #[cfg(not(test))]
    pub fn has_session_store(&self) -> bool {
        self.state_db_path().is_file()
    }

    #[cfg(not(test))]
    pub(crate) fn session_store_path(&self) -> PathBuf {
        self.state_db_path()
    }

    fn state_db_path(&self) -> PathBuf {
        self.root.join("state.db")
    }

    fn sessions_json_path(&self) -> PathBuf {
        self.root.join("sessions").join("sessions.json")
    }

    fn open_connection(&self) -> Result<Connection, AdapterError> {
        Connection::open_with_flags(
            self.state_db_path(),
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|error| read_error(HERMES_TOOL, &self.state_db_path(), error))
    }

    fn registry(&self) -> Result<HashMap<String, SessionRegistryEntry>, AdapterError> {
        let path = self.sessions_json_path();
        if !path.exists() {
            return Ok(HashMap::new());
        }
        let json =
            fs::read_to_string(&path).map_err(|error| read_error(HERMES_TOOL, &path, error))?;
        let registry = serde_json::from_str::<HashMap<String, SessionRegistryEntry>>(&json)
            .map_err(|error| read_error(HERMES_TOOL, &path, error))?;
        Ok(registry
            .into_values()
            .map(|entry| (entry.session_id.clone(), entry))
            .collect())
    }

    fn list_session_rows(
        &self,
        limit: Option<usize>,
    ) -> Result<Vec<HermesSessionRow>, AdapterError> {
        let db = self.open_connection()?;
        let columns = session_columns(&db, &self.state_db_path())?;
        let message_columns = message_columns(&db, &self.state_db_path())?;
        let select = session_select_sql(&columns, &message_columns);
        let unarchived = unarchived_clause(&columns);
        let query = format!(
            "{select} where {unarchived} order by s.started_at desc{}",
            match limit {
                Some(_) => " limit :limit",
                None => "",
            }
        );
        let mut statement = db
            .prepare(&query)
            .map_err(|error| read_error(HERMES_TOOL, &self.state_db_path(), error))?;
        let rows = if let Some(limit) = limit {
            let limit = i64::try_from(limit).unwrap_or(i64::MAX);
            statement
                .query_map(named_params! {":limit": limit}, session_row)
                .map_err(|error| read_error(HERMES_TOOL, &self.state_db_path(), error))?
        } else {
            statement
                .query_map([], session_row)
                .map_err(|error| read_error(HERMES_TOOL, &self.state_db_path(), error))?
        };
        collect_rows(rows, &self.state_db_path())
    }

    fn find_session_row(&self, session_id: &str) -> Result<Option<HermesSessionRow>, AdapterError> {
        let db = self.open_connection()?;
        let columns = session_columns(&db, &self.state_db_path())?;
        let message_columns = message_columns(&db, &self.state_db_path())?;
        let select = session_select_sql(&columns, &message_columns);
        let mut statement = db
            .prepare(&format!("{select} where s.id = ?1"))
            .map_err(|error| read_error(HERMES_TOOL, &self.state_db_path(), error))?;
        statement
            .query_row(params![session_id], session_row)
            .optional()
            .map_err(|error| read_error(HERMES_TOOL, &self.state_db_path(), error))
    }

    fn load_messages(
        &self,
        session_id: &str,
        event_limit: Option<usize>,
    ) -> Result<Vec<HermesMessageRow>, AdapterError> {
        let db = self.open_connection()?;
        let columns = message_columns(&db, &self.state_db_path())?;
        let platform_message_id = message_column(&columns, "platform_message_id", "NULL");
        let role = message_column(&columns, "role", "''");
        let content = message_column(&columns, "content", "NULL");
        let tool_calls = message_column(&columns, "tool_calls", "NULL");
        let tool_name = message_column(&columns, "tool_name", "NULL");
        let timestamp = message_column(&columns, "timestamp", "0");
        let token_count = message_column(&columns, "token_count", "NULL");
        let finish_reason = message_column(&columns, "finish_reason", "NULL");
        let reasoning = message_column(&columns, "reasoning", "NULL");
        let reasoning_content = message_column(&columns, "reasoning_content", "NULL");
        let reasoning_details = message_column(&columns, "reasoning_details", "NULL");
        let active = active_message_clause(&columns, "m");
        let limit_clause = if event_limit.is_some() {
            "limit ?2"
        } else {
            ""
        };
        let mut statement = db
            .prepare(&format!(
                r#"
                select
                    id,
                    {platform_message_id},
                    {role},
                    {content},
                    {tool_calls},
                    {tool_name},
                    strftime('%Y-%m-%dT%H:%M:%SZ', {timestamp}, 'unixepoch') as timestamp,
                    {token_count},
                    {finish_reason},
                    {reasoning},
                    {reasoning_content},
                    {reasoning_details}
                from messages m
                where m.session_id = ?1 and {active}
                order by timestamp asc, id asc
                {limit_clause}
                "#,
            ))
            .map_err(|error| read_error(HERMES_TOOL, &self.state_db_path(), error))?;
        if let Some(limit) = event_limit {
            let limit = i64::try_from(limit.saturating_add(1)).unwrap_or(i64::MAX);
            let rows = statement
                .query_map(params![session_id, limit], message_row)
                .map_err(|error| read_error(HERMES_TOOL, &self.state_db_path(), error))?;
            collect_rows(rows, &self.state_db_path())
        } else {
            let rows = statement
                .query_map(params![session_id], message_row)
                .map_err(|error| read_error(HERMES_TOOL, &self.state_db_path(), error))?;
            collect_rows(rows, &self.state_db_path())
        }
    }

    fn list_session_rows_matching(
        &self,
        query: &str,
        limit: Option<usize>,
    ) -> Result<Vec<HermesSessionRow>, AdapterError> {
        let db = self.open_connection()?;
        let session_columns = session_columns(&db, &self.state_db_path())?;
        let message_columns = message_columns(&db, &self.state_db_path())?;
        let select = session_select_sql(&session_columns, &message_columns);
        let unarchived = unarchived_clause(&session_columns);
        let active = active_message_clause(&message_columns, "m");
        let search_text = message_search_text_sql(&message_columns, "m");
        let query_sql = format!(
            r#"
            {select}
            where {unarchived}
              and exists (
                select 1
                from messages m
                where m.session_id = s.id
                  and {active}
                  and lower({search_text}) like :pattern escape '\'
              )
            order by s.started_at desc{}
            "#,
            match limit {
                Some(_) => " limit :limit",
                None => "",
            }
        );
        let pattern = like_pattern(query);
        let mut statement = db
            .prepare(&query_sql)
            .map_err(|error| read_error(HERMES_TOOL, &self.state_db_path(), error))?;
        let rows = if let Some(limit) = limit {
            let limit = i64::try_from(limit).unwrap_or(i64::MAX);
            statement
                .query_map(
                    named_params! {":pattern": pattern, ":limit": limit},
                    session_row,
                )
                .map_err(|error| read_error(HERMES_TOOL, &self.state_db_path(), error))?
        } else {
            statement
                .query_map(named_params! {":pattern": pattern}, session_row)
                .map_err(|error| read_error(HERMES_TOOL, &self.state_db_path(), error))?
        };
        collect_rows(rows, &self.state_db_path())
    }

    fn summary_for_row(
        &self,
        row: HermesSessionRow,
        registry: &HashMap<String, SessionRegistryEntry>,
    ) -> SessionSummary {
        let supplement = registry.get(&row.id);
        let token_count = supplement
            .and_then(|entry| entry.total_tokens)
            .or_else(|| total_tokens(&row));
        let provider_metadata = provider_metadata(&row, supplement, token_count, None, Vec::new());
        self.summary_for_row_with_metadata(row, registry, token_count, provider_metadata)
    }

    fn summary_for_row_with_continuation(
        &self,
        row: HermesSessionRow,
        registry: &HashMap<String, SessionRegistryEntry>,
        export: HermesContinuationExport,
    ) -> SessionSummary {
        let supplement = registry.get(&row.id);
        let token_count = supplement
            .and_then(|entry| entry.total_tokens)
            .or_else(|| total_tokens(&row));
        let provider_metadata = provider_metadata(
            &row,
            supplement,
            token_count,
            Some(export.search),
            export.points,
        );
        self.summary_for_row_with_metadata(row, registry, token_count, provider_metadata)
    }

    fn summary_for_row_with_metadata(
        &self,
        row: HermesSessionRow,
        registry: &HashMap<String, SessionRegistryEntry>,
        token_count: Option<usize>,
        provider_metadata: ProviderSessionMetadata,
    ) -> SessionSummary {
        let supplement = registry.get(&row.id);
        let status = session_status(&row, supplement);
        let health_reason = health_reason(&row, supplement);
        let title = row
            .title
            .clone()
            .or_else(|| supplement.and_then(display_name))
            .or_else(|| {
                let preview = row.preview.trim();
                (!preview.is_empty()
                    && !is_provider_context_text(preview)
                    && !is_moonbox_handoff_control_text(preview))
                .then(|| truncate(preview, 160))
            })
            .unwrap_or_else(|| format!("Hermes {} session {}", row.source, short_id(&row.id)));

        SessionSummary {
            id: row.id.clone(),
            cli: HERMES_TOOL,
            title,
            cwd: row
                .cwd
                .clone()
                .or_else(|| supplement.and_then(context_from_supplement))
                .unwrap_or_else(|| "~".into()),
            updated: human_timestamp(&row.updated_at),
            updated_at: row.updated_at,
            runtime_status: SessionRuntimeStatus::Unknown,
            runtime_reason: Some(unknown_runtime_reason(HERMES_TOOL)),
            status,
            branch: None,
            token_count,
            health_reason: Some(health_reason),
            event_count: row.active_message_count.max(row.message_count),
            resume_command: format!("hermes --resume {}", row.id),
            source_provenance: SourceProvenance::Real,
            source_path: Some(self.state_db_path().display().to_string()),
            source_size_bytes: None,
            parse_skip_count: 0,
            provider_metadata: Some(provider_metadata),
            context_health: hermes_context_health(
                token_count,
                row.model.as_deref(),
                row.compact_count,
            ),
            anatomy: None,
        }
    }

    pub(crate) fn search_sessions(
        &self,
        query: &str,
        point_limit: usize,
    ) -> Result<Vec<SessionSummary>, AdapterError> {
        let query = query.trim();
        if query.is_empty() {
            return Ok(Vec::new());
        }
        let registry = self.registry()?;
        let rows = self.list_session_rows_matching(query, self.list_limit)?;
        let db = self.open_connection()?;
        rows.into_iter()
            .map(|row| {
                let export =
                    self.continuation_export_for_session(&db, &row.id, query, point_limit)?;
                Ok(self.summary_for_row_with_continuation(row, &registry, export))
            })
            .collect()
    }

    fn continuation_export_for_session(
        &self,
        db: &Connection,
        session_id: &str,
        query: &str,
        point_limit: usize,
    ) -> Result<HermesContinuationExport, AdapterError> {
        let point_limit = point_limit.max(1);
        let message_columns = message_columns(db, &self.state_db_path())?;
        let rows = continuation_rows(
            db,
            &self.state_db_path(),
            &message_columns,
            session_id,
            query,
            point_limit,
        )?;
        let matched_message_count = continuation_match_count(
            db,
            &self.state_db_path(),
            &message_columns,
            session_id,
            query,
        )?;
        let points = rows
            .into_iter()
            .enumerate()
            .map(|(index, row)| continuation_point(row, index + 1))
            .collect::<Vec<_>>();
        let search = ProviderSearchMetadata {
            backend: "local_sqlite_like".into(),
            query: Some(query.to_owned()),
            matched_message_count,
            continuation_point_count: points.len(),
            truncated: matched_message_count > points.len(),
        };
        Ok(HermesContinuationExport { search, points })
    }
}

impl SourceAdapter for HermesSourceAdapter {
    fn tool(&self) -> CliTool {
        HERMES_TOOL
    }

    fn provenance(&self) -> SourceProvenance {
        SourceProvenance::Real
    }

    fn store_path(&self) -> Option<String> {
        Some(self.state_db_path().display().to_string())
    }

    fn list_sessions(&self) -> Result<Vec<SessionSummary>, AdapterError> {
        let registry = self.registry()?;
        Ok(self
            .list_session_rows(self.list_limit)?
            .into_iter()
            .map(|row| self.summary_for_row(row, &registry))
            .collect())
    }

    fn list_sessions_with_report(
        &self,
        filter_status: &str,
        reason: &str,
    ) -> Result<(Vec<SessionSummary>, super::model::SourceAdapterReport), AdapterError> {
        let sessions = self.list_sessions()?;
        let report = report_from_sessions_with_scan(
            SourceReportMeta {
                cli: self.tool(),
                provenance: self.provenance(),
                active: true,
                store_path: self.store_path(),
                filter_status: filter_status.into(),
                reason: reason.into(),
                fidelity: Some(hermes_local_fidelity()),
                capabilities: None,
            },
            &sessions,
            SourceScanStats {
                list_limit: self.list_limit,
                scan_entry_count: sessions.len(),
                ..SourceScanStats::default()
            },
        );
        Ok((sessions, report))
    }

    fn find_session(&self, session_id: &str) -> Result<Option<SessionSummary>, AdapterError> {
        let Some(row) = self.find_session_row(session_id)? else {
            return Ok(None);
        };
        let registry = self.registry()?;
        Ok(Some(self.summary_for_row(row, &registry)))
    }

    fn load_timeline(&self, session_id: &str) -> Result<CanonicalTimeline, AdapterError> {
        self.load_timeline_limited_for_id(session_id, None)
    }

    fn load_timeline_limited(
        &self,
        session: &SessionSummary,
        event_limit: Option<usize>,
    ) -> Result<CanonicalTimeline, AdapterError> {
        self.load_timeline_limited_for_id(&session.id, event_limit)
    }
}

impl HermesSourceAdapter {
    fn load_timeline_limited_for_id(
        &self,
        session_id: &str,
        event_limit: Option<usize>,
    ) -> Result<CanonicalTimeline, AdapterError> {
        if self.find_session_row(session_id)?.is_none() {
            return Err(AdapterError::SessionNotFound {
                tool: HERMES_TOOL,
                session_id: session_id.into(),
            });
        }
        let messages = self.load_messages(session_id, event_limit)?;
        let mut events = Vec::new();
        for row in messages {
            if let Some(event) = timeline_event(row, events.len() + 1, session_id) {
                push_timeline_event(&mut events, event, None);
            }
        }
        if let Some(limit) = event_limit
            && events.len() > limit
        {
            events.truncate(limit);
            events.push(timeline_preview_truncated_event(events.len() + 1, limit));
        }

        Ok(CanonicalTimeline {
            version: 1,
            source_cli: HERMES_TOOL,
            source_session: session_id.into(),
            events,
        })
    }
}

fn session_columns(db: &Connection, path: &Path) -> Result<HashSet<String>, AdapterError> {
    let mut statement = db
        .prepare("pragma table_info(sessions)")
        .map_err(|error| read_error(HERMES_TOOL, path, error))?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|error| read_error(HERMES_TOOL, path, error))?;
    collect_rows(rows, path).map(|columns| columns.into_iter().collect())
}

fn message_columns(db: &Connection, path: &Path) -> Result<HashSet<String>, AdapterError> {
    let mut statement = db
        .prepare("pragma table_info(messages)")
        .map_err(|error| read_error(HERMES_TOOL, path, error))?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|error| read_error(HERMES_TOOL, path, error))?;
    collect_rows(rows, path).map(|columns| columns.into_iter().collect())
}

fn session_select_sql(
    session_columns: &HashSet<String>,
    message_columns: &HashSet<String>,
) -> String {
    let user_id = session_column(session_columns, "user_id", "NULL");
    let model = session_column(session_columns, "model", "NULL");
    let model_config = session_column(session_columns, "model_config", "NULL");
    let system_prompt = session_column(session_columns, "system_prompt", "NULL");
    let parent_session_id = session_column(session_columns, "parent_session_id", "NULL");
    let started_at = session_column(session_columns, "started_at", "0");
    let ended_at = session_column(session_columns, "ended_at", "NULL");
    let end_reason = session_column(session_columns, "end_reason", "NULL");
    let message_count = session_integer_column(session_columns, "message_count");
    let tool_call_count = session_integer_column(session_columns, "tool_call_count");
    let input_tokens = session_integer_column(session_columns, "input_tokens");
    let output_tokens = session_integer_column(session_columns, "output_tokens");
    let cache_read_tokens = session_integer_column(session_columns, "cache_read_tokens");
    let cache_write_tokens = session_integer_column(session_columns, "cache_write_tokens");
    let reasoning_tokens = session_integer_column(session_columns, "reasoning_tokens");
    let cwd = session_column(session_columns, "cwd", "NULL");
    let title = session_column(session_columns, "title", "NULL");
    let handoff_state = session_column(session_columns, "handoff_state", "NULL");
    let handoff_platform = session_column(session_columns, "handoff_platform", "NULL");
    let handoff_error = session_column(session_columns, "handoff_error", "NULL");
    let rewind_count = session_integer_column(session_columns, "rewind_count");
    let archived = if session_columns.contains("archived") {
        "s.archived != 0"
    } else {
        "0"
    };
    let message_active = active_message_clause(message_columns, "m");
    let message_timestamp = message_column(message_columns, "timestamp", "0");
    let message_role = message_column(message_columns, "role", "''");
    let message_content = message_column(message_columns, "content", "NULL");

    format!(
        r#"
    select
        s.id,
        s.source,
        {user_id},
        {model},
        {model_config},
        {system_prompt},
        {parent_session_id},
        strftime(
            '%Y-%m-%dT%H:%M:%SZ',
            coalesce(
                (select max({message_timestamp}) from messages m where m.session_id = s.id and {message_active}),
                {ended_at},
                {started_at}
            ),
            'unixepoch'
        ) as updated_at,
        {end_reason},
        {message_count} as message_count,
        {tool_call_count} as tool_call_count,
        {input_tokens} as input_tokens,
        {output_tokens} as output_tokens,
        {cache_read_tokens} as cache_read_tokens,
        {cache_write_tokens} as cache_write_tokens,
        {reasoning_tokens} as reasoning_tokens,
        {cwd},
        {title},
        {handoff_state},
        {handoff_platform},
        {handoff_error},
        {rewind_count} as rewind_count,
        coalesce((select count(*) from messages m where m.session_id = s.id and {message_active} and {message_role} in ('summary', 'compact')), 0) as compact_count,
        {archived} as archived,
        coalesce((select count(*) from messages m where m.session_id = s.id and {message_active}), 0) as active_message_count,
        coalesce(
            (
                select substr(replace(replace({message_content}, char(10), ' '), char(13), ' '), 1, 63)
                from messages m
                where m.session_id = s.id and {message_active} and {message_role} = 'user' and {message_content} is not null
                order by {message_timestamp} asc, m.id asc
                limit 1
            ),
            ''
        ) as preview
    from sessions s
"#
    )
}

fn session_column(columns: &HashSet<String>, column: &str, fallback: &str) -> String {
    if columns.contains(column) {
        format!("s.{column}")
    } else {
        fallback.to_owned()
    }
}

fn session_integer_column(columns: &HashSet<String>, column: &str) -> String {
    if columns.contains(column) {
        format!("coalesce(s.{column}, 0)")
    } else {
        "0".to_owned()
    }
}

fn unarchived_clause(columns: &HashSet<String>) -> &'static str {
    if columns.contains("archived") {
        "s.archived = 0"
    } else {
        "1 = 1"
    }
}

fn session_row(row: &Row<'_>) -> rusqlite::Result<HermesSessionRow> {
    Ok(HermesSessionRow {
        id: row.get(0)?,
        source: row.get(1)?,
        user_id: row.get(2)?,
        model: row.get(3)?,
        model_config: row.get(4)?,
        system_prompt: row.get(5)?,
        parent_session_id: row.get(6)?,
        updated_at: row.get(7)?,
        end_reason: row.get(8)?,
        message_count: integer(row, 9)?,
        tool_call_count: integer(row, 10)?,
        input_tokens: integer(row, 11)?,
        output_tokens: integer(row, 12)?,
        cache_read_tokens: integer(row, 13)?,
        cache_write_tokens: integer(row, 14)?,
        reasoning_tokens: integer(row, 15)?,
        cwd: row.get(16)?,
        title: row.get(17)?,
        handoff_state: row.get(18)?,
        handoff_platform: row.get(19)?,
        handoff_error: row.get(20)?,
        rewind_count: integer(row, 21)?,
        compact_count: integer(row, 22)?,
        archived: row.get(23)?,
        active_message_count: integer(row, 24)?,
        preview: row.get(25)?,
    })
}

fn message_row(row: &Row<'_>) -> rusqlite::Result<HermesMessageRow> {
    Ok(HermesMessageRow {
        message_id: row.get::<_, i64>(0)?.to_string(),
        platform_message_id: row.get(1)?,
        role: row.get(2)?,
        content: row.get(3)?,
        tool_calls: row.get(4)?,
        tool_name: row.get(5)?,
        timestamp: row.get(6)?,
        token_count: optional_integer(row, 7)?,
        finish_reason: row.get(8)?,
        reasoning: row.get(9)?,
        reasoning_content: row.get(10)?,
        reasoning_details: row.get(11)?,
    })
}

fn continuation_row(row: &Row<'_>) -> rusqlite::Result<HermesContinuationRow> {
    Ok(HermesContinuationRow {
        message_id: row.get::<_, i64>(0)?.to_string(),
        role: row.get(1)?,
        timestamp: row.get(2)?,
        detail: row.get(3)?,
        message_index: integer(row, 4)?,
        total_messages: integer(row, 5)?,
        before_message_id: row.get::<_, Option<i64>>(6)?.map(|id| id.to_string()),
        bookend_before: row.get(7)?,
        after_message_id: row.get::<_, Option<i64>>(8)?.map(|id| id.to_string()),
        bookend_after: row.get(9)?,
    })
}

fn collect_rows<T>(
    rows: MappedRows<'_, impl FnMut(&Row<'_>) -> rusqlite::Result<T>>,
    path: &Path,
) -> Result<Vec<T>, AdapterError> {
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|error| read_error(HERMES_TOOL, path, error))
}

fn integer(row: &Row<'_>, index: usize) -> rusqlite::Result<usize> {
    let value = row.get::<_, i64>(index)?;
    Ok(usize::try_from(value).unwrap_or(0))
}

fn optional_integer(row: &Row<'_>, index: usize) -> rusqlite::Result<Option<usize>> {
    let value = row.get::<_, Option<i64>>(index)?;
    Ok(value.and_then(|value| usize::try_from(value).ok()))
}

fn continuation_rows(
    db: &Connection,
    path: &Path,
    columns: &HashSet<String>,
    session_id: &str,
    query: &str,
    limit: usize,
) -> Result<Vec<HermesContinuationRow>, AdapterError> {
    let active = active_message_clause(columns, "m");
    let detail = message_detail_sql(columns, "m");
    let search_text = message_search_text_sql(columns, "m");
    let sql = format!(
        r#"
        with base as (
            select
                m.id as id,
                {role} as role,
                strftime('%Y-%m-%dT%H:%M:%SZ', {timestamp}, 'unixepoch') as timestamp,
                {detail} as detail,
                {search_text} as search_text,
                {timestamp} as raw_timestamp
            from messages m
            where m.session_id = :session_id
              and {active}
        ),
        ordered as (
            select
                id,
                role,
                timestamp,
                detail,
                search_text,
                row_number() over (order by raw_timestamp asc, id asc) as message_index,
                count(*) over () as total_messages,
                lag(id) over (order by raw_timestamp asc, id asc) as before_message_id,
                lag(detail) over (order by raw_timestamp asc, id asc) as bookend_before,
                lead(id) over (order by raw_timestamp asc, id asc) as after_message_id,
                lead(detail) over (order by raw_timestamp asc, id asc) as bookend_after
            from base
        )
        select
            id,
            role,
            timestamp,
            detail,
            message_index,
            total_messages,
            before_message_id,
            bookend_before,
            after_message_id,
            bookend_after
        from ordered
        where lower(search_text) like :pattern escape '\'
        order by message_index desc
        limit :limit
        "#,
        role = message_column(columns, "role", "''"),
        timestamp = message_column(columns, "timestamp", "0"),
    );
    let limit = i64::try_from(limit).unwrap_or(i64::MAX);
    let pattern = like_pattern(query);
    let mut statement = db
        .prepare(&sql)
        .map_err(|error| read_error(HERMES_TOOL, path, error))?;
    let rows = statement
        .query_map(
            named_params! {
                ":session_id": session_id,
                ":pattern": pattern,
                ":limit": limit,
            },
            continuation_row,
        )
        .map_err(|error| read_error(HERMES_TOOL, path, error))?;
    collect_rows(rows, path)
}

fn continuation_match_count(
    db: &Connection,
    path: &Path,
    columns: &HashSet<String>,
    session_id: &str,
    query: &str,
) -> Result<usize, AdapterError> {
    let active = active_message_clause(columns, "m");
    let search_text = message_search_text_sql(columns, "m");
    let sql = format!(
        r#"
        select count(*)
        from messages m
        where m.session_id = :session_id
          and {active}
          and lower({search_text}) like :pattern escape '\'
        "#
    );
    let pattern = like_pattern(query);
    db.query_row(
        &sql,
        named_params! {":session_id": session_id, ":pattern": pattern},
        |row| integer(row, 0),
    )
    .map_err(|error| read_error(HERMES_TOOL, path, error))
}

fn continuation_point(row: HermesContinuationRow, score: usize) -> ProviderContinuationPoint {
    let snippet = truncate_timeline_detail(&row.detail);
    ProviderContinuationPoint {
        message_id: row.message_id,
        event_id: Some(event_id(row.message_index)),
        role: row.role,
        timestamp: row.timestamp,
        snippet,
        bookend_before: row
            .bookend_before
            .filter(|value| !value.trim().is_empty())
            .map(|value| truncate(&value, 280)),
        bookend_after: row
            .bookend_after
            .filter(|value| !value.trim().is_empty())
            .map(|value| truncate(&value, 280)),
        scroll_context: ProviderScrollContext {
            message_index: row.message_index,
            total_messages: row.total_messages,
            before_message_id: row.before_message_id,
            after_message_id: row.after_message_id,
        },
        score,
    }
}

fn active_message_clause(columns: &HashSet<String>, alias: &str) -> String {
    if columns.contains("active") {
        format!("coalesce({alias}.active, 1) = 1")
    } else {
        "1 = 1".to_owned()
    }
}

fn message_column(columns: &HashSet<String>, column: &str, fallback: &str) -> String {
    if columns.contains(column) {
        format!("m.{column}")
    } else {
        fallback.to_owned()
    }
}

fn message_detail_sql(columns: &HashSet<String>, alias: &str) -> String {
    let values = [
        "content",
        "reasoning",
        "reasoning_content",
        "reasoning_details",
        "tool_calls",
        "tool_name",
        "finish_reason",
    ]
    .into_iter()
    .filter(|column| columns.contains(*column))
    .map(|column| format!("nullif(trim(coalesce({alias}.{column}, '')), '')"))
    .collect::<Vec<_>>();
    if values.is_empty() {
        "''".to_owned()
    } else {
        format!("coalesce({}, '')", values.join(", "))
    }
}

fn message_search_text_sql(columns: &HashSet<String>, alias: &str) -> String {
    let values = [
        "content",
        "reasoning",
        "reasoning_content",
        "reasoning_details",
        "tool_calls",
        "tool_name",
        "finish_reason",
    ]
    .into_iter()
    .filter(|column| columns.contains(*column))
    .map(|column| format!("coalesce({alias}.{column}, '')"))
    .collect::<Vec<_>>();
    if values.is_empty() {
        "''".to_owned()
    } else {
        values.join(" || ' ' || ")
    }
}

fn like_pattern(query: &str) -> String {
    let escaped = query
        .trim()
        .to_ascii_lowercase()
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    format!("%{escaped}%")
}

fn total_tokens(row: &HermesSessionRow) -> Option<usize> {
    row.input_tokens
        .checked_add(row.output_tokens)?
        .checked_add(row.cache_read_tokens)?
        .checked_add(row.cache_write_tokens)?
        .checked_add(row.reasoning_tokens)
        .filter(|tokens| *tokens > 0)
}

fn hermes_context_health(
    token_count: Option<usize>,
    model: Option<&str>,
    compact_count: usize,
) -> Option<ContextHealth> {
    let used_tokens = token_count.filter(|tokens| *tokens > 0);
    let window_resolution =
        model.and_then(|model| resolve_model_context_window(HERMES_TOOL, model));
    let window_tokens = window_resolution
        .as_ref()
        .map(|resolution| resolution.window_tokens);
    let quality_cliff_tokens = window_resolution
        .as_ref()
        .and_then(|resolution| resolution.quality_cliff_tokens);
    (used_tokens.is_some() || window_tokens.is_some() || compact_count > 0).then(|| {
        let source = match (
            model.filter(|model| !model.trim().is_empty()),
            &window_resolution,
        ) {
            (Some(model), Some(resolution)) => {
                format!(
                    "hermes sqlite session · model {model} · {}",
                    resolution.source
                )
            }
            (Some(model), None) => format!("hermes sqlite session · model {model}"),
            (None, _) => "hermes sqlite session".into(),
        };

        ContextHealth {
            used_tokens,
            window_tokens,
            quality_cliff_tokens,
            compact_layers: compact_count,
            handoff_markers: 0,
            confidence: EvidenceConfidence::Derived,
            source,
        }
    })
}

fn session_status(
    row: &HermesSessionRow,
    supplement: Option<&SessionRegistryEntry>,
) -> SessionStatus {
    if row
        .handoff_error
        .as_ref()
        .is_some_and(|value| !value.is_empty())
        || row
            .end_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("error") || reason.contains("fail"))
    {
        return SessionStatus::Failed;
    }
    if row.archived
        || row.rewind_count > 0
        || supplement
            .and_then(|entry| entry.suspended)
            .unwrap_or(false)
        || supplement
            .and_then(|entry| entry.resume_pending)
            .unwrap_or(false)
    {
        return SessionStatus::Warning;
    }
    SessionStatus::Healthy
}

fn health_reason(row: &HermesSessionRow, supplement: Option<&SessionRegistryEntry>) -> String {
    let mut parts = vec![format!(
        "real Hermes SQLite session, source: {}",
        row.source
    )];
    if let Some(platform) = supplement
        .and_then(|entry| entry.platform.as_deref())
        .filter(|platform| !platform.is_empty() && *platform != row.source)
    {
        parts.push(format!("platform: {platform}"));
    }
    if let Some(user_id) = row.user_id.as_deref().filter(|value| !value.is_empty()) {
        parts.push(format!("user: {user_id}"));
    }
    if let Some(session_key) = supplement
        .and_then(|entry| entry.session_key.as_deref())
        .filter(|value| !value.is_empty())
    {
        parts.push(format!("session_key: {session_key}"));
    }
    if let Some(model) = row.model.as_deref().filter(|model| !model.is_empty()) {
        parts.push(format!("model: {model}"));
    }
    if let Some(total) = total_tokens(row) {
        parts.push(format!("tokens: {total}"));
    }
    if row.tool_call_count > 0 {
        parts.push(format!("{} tool call(s)", row.tool_call_count));
    }
    if let Some(platform) = row
        .handoff_platform
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        parts.push(format!("handoff: {platform}"));
    }
    if let Some(state) = row
        .handoff_state
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        parts.push(format!("state: {state}"));
    }
    if let Some(error) = row
        .handoff_error
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        parts.push(format!("error: {error}"));
    }
    if supplement
        .and_then(|entry| entry.expiry_finalized)
        .unwrap_or(false)
    {
        parts.push("expiry finalized".into());
    }
    parts.join("; ")
}

fn provider_metadata(
    row: &HermesSessionRow,
    supplement: Option<&SessionRegistryEntry>,
    token_count: Option<usize>,
    search: Option<ProviderSearchMetadata>,
    continuation_points: Vec<ProviderContinuationPoint>,
) -> ProviderSessionMetadata {
    ProviderSessionMetadata {
        source: non_empty_string(&row.source),
        thread_source: None,
        platform: supplement
            .and_then(|entry| entry.platform.as_deref())
            .and_then(non_empty_string)
            .or_else(|| non_empty_string(&row.source)),
        user_id: row.user_id.as_deref().and_then(non_empty_string),
        session_key: supplement
            .and_then(|entry| entry.session_key.as_deref())
            .and_then(non_empty_string),
        parent_session_id: row.parent_session_id.as_deref().and_then(non_empty_string),
        model: row.model.as_deref().and_then(non_empty_string),
        model_config: row.model_config.as_deref().and_then(json_value_from_text),
        system_prompt_snapshot: row
            .system_prompt
            .as_deref()
            .and_then(non_empty_string)
            .map(|prompt| truncate(&prompt, 4000)),
        origin: supplement.and_then(origin_metadata),
        handoff: handoff_metadata(row),
        token_breakdown: token_breakdown(row, token_count),
        archived: Some(row.archived),
        search,
        continuation_points,
    }
}

fn non_empty_string(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

fn json_value_from_text(value: &str) -> Option<Value> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    Some(serde_json::from_str::<Value>(value).unwrap_or_else(|_| Value::String(value.to_owned())))
}

fn origin_metadata(entry: &SessionRegistryEntry) -> Option<Value> {
    if entry.origin.is_null() {
        None
    } else {
        Some(entry.origin.clone())
    }
}

fn handoff_metadata(row: &HermesSessionRow) -> Option<ProviderHandoffMetadata> {
    let handoff = ProviderHandoffMetadata {
        state: row.handoff_state.as_deref().and_then(non_empty_string),
        platform: row.handoff_platform.as_deref().and_then(non_empty_string),
        error: row.handoff_error.as_deref().and_then(non_empty_string),
    };
    (handoff.state.is_some() || handoff.platform.is_some() || handoff.error.is_some())
        .then_some(handoff)
}

fn token_breakdown(
    row: &HermesSessionRow,
    total_override: Option<usize>,
) -> Option<TokenBreakdown> {
    let total = total_override.or_else(|| total_tokens(row)).unwrap_or(0);
    if total == 0
        && row.input_tokens == 0
        && row.output_tokens == 0
        && row.cache_read_tokens == 0
        && row.cache_write_tokens == 0
        && row.reasoning_tokens == 0
    {
        return None;
    }
    Some(TokenBreakdown {
        input: row.input_tokens,
        output: row.output_tokens,
        cache_read: row.cache_read_tokens,
        cache_write: row.cache_write_tokens,
        reasoning: row.reasoning_tokens,
        total,
    })
}

fn display_name(entry: &SessionRegistryEntry) -> Option<String> {
    entry
        .origin
        .get("chat_name")
        .and_then(Value::as_str)
        .or(entry.display_name.as_deref())
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn context_from_supplement(entry: &SessionRegistryEntry) -> Option<String> {
    let platform = entry.platform.as_deref()?;
    let mut context = platform.to_owned();
    if let Some(chat_type) = entry.chat_type.as_deref().filter(|value| !value.is_empty()) {
        context.push('/');
        context.push_str(chat_type);
    }
    if let Some(session_key) = entry
        .session_key
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        context.push(' ');
        context.push_str(session_key);
    }
    Some(context)
}

fn timeline_event(row: HermesMessageRow, number: usize, session_id: &str) -> Option<TimelineEvent> {
    let kind = timeline_kind(&row)?;
    let detail = timeline_detail(&row);
    if detail.is_empty() && !matches!(kind, TimelineKind::Error) {
        return None;
    }
    if kind == TimelineKind::User
        && (is_provider_context_text(&detail) || is_moonbox_handoff_control_text(&detail))
    {
        return None;
    }

    Some(TimelineEvent {
        id: event_id(number),
        time: display_time(Some(&row.timestamp)),
        kind,
        title: timeline_title(&row),
        detail,
        metadata: timeline_metadata(&row, kind, session_id),
    })
}

fn timeline_metadata(
    row: &HermesMessageRow,
    kind: TimelineKind,
    session_id: &str,
) -> TimelineEventMetadata {
    let mut message_ids = vec![row.message_id.clone()];
    if let Some(platform_message_id) = row
        .platform_message_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        message_ids.push(platform_message_id.to_owned());
    }
    TimelineEventMetadata {
        raw_refs: vec![TimelineEventRawRef {
            source_cli: Some(HERMES_TOOL),
            source_session: Some(session_id.into()),
            row_id: Some(row.message_id.clone()),
            provider_kind: Some(row.role.clone()),
            role: Some(row.role.clone()),
            digest: Some(stable_text_digest(&format!(
                "{}\n{}\n{}\n{}\n{}",
                row.message_id,
                row.role,
                row.content.as_deref().unwrap_or_default(),
                row.tool_calls.as_deref().unwrap_or_default(),
                row.timestamp
            ))),
            ..TimelineEventRawRef::default()
        }],
        message_ids,
        provider_item_ids: row
            .platform_message_id
            .clone()
            .into_iter()
            .filter(|value| !value.trim().is_empty())
            .collect(),
        tool_calls: row
            .tool_calls
            .as_deref()
            .and_then(tool_call_metadata)
            .into_iter()
            .collect(),
        file_changes: (kind == TimelineKind::GitDiff)
            .then_some(super::model::TimelineFileChange {
                summary: Some(timeline_detail(row)),
                diff: row.content.clone(),
                ..super::model::TimelineFileChange::default()
            })
            .into_iter()
            .collect(),
        token_usage: row.token_count.map(|total| TokenBreakdown {
            total,
            ..TokenBreakdown::default()
        }),
        ..TimelineEventMetadata::default()
    }
}

fn timeline_kind(row: &HermesMessageRow) -> Option<TimelineKind> {
    if row
        .finish_reason
        .as_deref()
        .is_some_and(|reason| reason.contains("error") || reason.contains("fail"))
    {
        return Some(TimelineKind::Error);
    }
    match row.role.as_str() {
        "user" => Some(TimelineKind::User),
        "assistant" if has_tool_call(row) => Some(TimelineKind::Tool),
        "assistant" => Some(TimelineKind::Assistant),
        "tool" | "session_meta" => Some(TimelineKind::Tool),
        "summary" | "compact" => Some(TimelineKind::Compact),
        role if !role.is_empty() => Some(TimelineKind::Tool),
        _ => None,
    }
}

fn timeline_title(row: &HermesMessageRow) -> String {
    match row.role.as_str() {
        "user" => "User".into(),
        "assistant" if has_tool_call(row) => "Tool call".into(),
        "assistant" => "Assistant".into(),
        "tool" => row
            .tool_name
            .as_deref()
            .map(|name| format!("Tool: {name}"))
            .unwrap_or_else(|| "Tool".into()),
        "session_meta" => "Session".into(),
        "summary" | "compact" => "Compact".into(),
        role => title_case(role),
    }
}

fn timeline_detail(row: &HermesMessageRow) -> String {
    let content = row
        .content
        .as_deref()
        .and_then(|content| text_from_value(&Value::String(content.into())));
    let reasoning = row
        .reasoning
        .as_deref()
        .or(row.reasoning_content.as_deref())
        .or(row.reasoning_details.as_deref())
        .and_then(|content| text_from_value(&Value::String(content.into())));
    let tool_call = row.tool_calls.as_deref().and_then(tool_call_detail);
    let token_count = row.token_count.map(|tokens| format!("{tokens} token(s)"));

    content
        .or(reasoning)
        .or(tool_call)
        .or(token_count)
        .map(|detail| truncate_timeline_detail(&detail))
        .unwrap_or_default()
}

fn has_tool_call(row: &HermesMessageRow) -> bool {
    row.tool_calls
        .as_deref()
        .is_some_and(|calls| !calls.trim().is_empty() && calls.trim() != "[]")
}

fn tool_call_detail(tool_calls: &str) -> Option<String> {
    let value = serde_json::from_str::<Value>(tool_calls).ok()?;
    text_from_value(&value).or_else(|| Some("tool call".into()))
}

fn tool_call_metadata(tool_calls: &str) -> Option<TimelineToolCall> {
    let value = serde_json::from_str::<Value>(tool_calls).ok()?;
    let item = value
        .as_array()
        .and_then(|items| items.first())
        .unwrap_or(&value);
    Some(TimelineToolCall {
        id: item
            .get("id")
            .or_else(|| item.get("call_id"))
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_owned),
        name: item
            .get("name")
            .or_else(|| item.get("tool_name"))
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_owned),
        arguments: item
            .get("arguments")
            .or_else(|| item.get("args"))
            .or_else(|| item.get("input"))
            .filter(|value| !value.is_null())
            .cloned(),
        raw: Some(value),
    })
}

fn short_id(id: &str) -> String {
    id.chars().take(8).collect()
}

fn hermes_local_fidelity() -> SourceFidelity {
    SourceFidelity {
        status: SourceFidelityStatus::Fallback,
        primary_surface: "hermes_local_sqlite".into(),
        fallback_surface: Some("hermes_gateway_export_search".into()),
        detail: "read-only local SQLite/registry fallback; Hermes gateway/export/search commands are not invoked".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn lists_hermes_sessions_from_sqlite_store() {
        let root = test_root("list");
        write_state_db(&root);
        Connection::open(root.join("state.db"))
            .expect("db")
            .execute(
                "insert into messages (session_id, role, content, timestamp, active) values (?1, ?2, ?3, ?4, 1)",
                params![
                    "hermes-feishu",
                    "compact",
                    "Conversation summary",
                    1780641496.5
                ],
            )
            .expect("compact message");
        write_sessions_json(
            &root,
            r#"{
  "agent:main:feishu:dm:chat": {
    "session_id": "hermes-feishu",
    "session_key": "agent:main:feishu:dm:chat",
    "display_name": "Feishu DM",
    "platform": "feishu",
    "chat_type": "dm",
    "total_tokens": 88,
    "suspended": false,
    "resume_pending": false,
    "expiry_finalized": true,
    "origin": {"chat_name": "Ops Room"}
  }
}"#,
        );

        let sessions = HermesSourceAdapter::new(&root)
            .list_sessions()
            .expect("sessions");

        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].id, "hermes-feishu");
        assert_eq!(sessions[0].title, "Ops Room");
        assert_eq!(sessions[0].token_count, Some(88));
        assert_eq!(sessions[0].resume_command, "hermes --resume hermes-feishu");
        let metadata = sessions[0].provider_metadata.as_ref().expect("metadata");
        assert_eq!(metadata.source.as_deref(), Some("feishu"));
        assert_eq!(metadata.platform.as_deref(), Some("feishu"));
        assert_eq!(metadata.user_id.as_deref(), Some("ou_feishu"));
        assert_eq!(
            metadata.session_key.as_deref(),
            Some("agent:main:feishu:dm:chat")
        );
        assert_eq!(metadata.model.as_deref(), Some("claude-sonnet-4-6"));
        let context_health = sessions[0].context_health.as_ref().expect("context health");
        assert_eq!(context_health.used_tokens, Some(88));
        assert_eq!(context_health.window_tokens, Some(1_000_000));
        assert_eq!(context_health.compact_layers, 1);
        assert_eq!(metadata.parent_session_id.as_deref(), Some("parent-feishu"));
        assert_eq!(metadata.archived, Some(false));
        assert_eq!(
            metadata
                .token_breakdown
                .as_ref()
                .expect("token breakdown")
                .total,
            88
        );
        assert_eq!(
            metadata
                .handoff
                .as_ref()
                .expect("handoff")
                .platform
                .as_deref(),
            Some("feishu")
        );
        assert!(metadata.model_config.is_some());
        assert!(
            metadata
                .system_prompt_snapshot
                .as_deref()
                .expect("system prompt")
                .contains("Feishu system")
        );
        assert_eq!(sessions[1].id, "hermes-cli");
        assert_eq!(sessions[1].title, "CLI bugfix");
        assert_eq!(sessions[1].token_count, Some(15));
        assert_eq!(sessions[1].resume_command, "hermes --resume hermes-cli");
        assert!(
            sessions
                .iter()
                .all(|session| session.id != "hermes-discord-archived")
        );
    }

    #[test]
    fn lists_legacy_hermes_schema_without_provider_columns() {
        let root = test_root("legacy-schema");
        write_legacy_state_db(&root);
        let adapter = HermesSourceAdapter::new(&root);

        let sessions = adapter.list_sessions().expect("sessions");

        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].id, "legacy-feishu");
        assert_eq!(sessions[0].token_count, Some(5));
        assert_eq!(sessions[0].event_count, 1);
        let metadata = sessions[0].provider_metadata.as_ref().expect("metadata");
        assert_eq!(metadata.source.as_deref(), Some("feishu"));
        assert_eq!(metadata.platform.as_deref(), Some("feishu"));
        assert_eq!(metadata.user_id, None);
        assert_eq!(metadata.model_config, None);
        assert_eq!(metadata.system_prompt_snapshot, None);
        assert_eq!(metadata.archived, Some(false));
        assert_eq!(sessions[1].id, "legacy-cli");

        let timeline = adapter.load_timeline("legacy-feishu").expect("timeline");
        assert_eq!(timeline.events.len(), 1);
        assert_eq!(timeline.events[0].detail, "Fix legacy Feishu");

        let matches = adapter
            .search_sessions("legacy Feishu", 1)
            .expect("search sessions");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].id, "legacy-feishu");
    }

    #[test]
    fn loads_canonical_timeline_from_hermes_messages() {
        let root = test_root("timeline");
        write_state_db(&root);

        let timeline = HermesSourceAdapter::new(&root)
            .load_timeline("hermes-feishu")
            .expect("timeline");

        assert_eq!(timeline.source_cli, CliTool::Hermes);
        assert_eq!(timeline.source_session, "hermes-feishu");
        assert_eq!(timeline.events[0].kind, TimelineKind::Tool);
        assert_eq!(timeline.events[1].kind, TimelineKind::User);
        assert_eq!(timeline.events[2].kind, TimelineKind::Tool);
        assert_eq!(timeline.events[3].kind, TimelineKind::Tool);
        assert_eq!(timeline.events[4].kind, TimelineKind::Assistant);
        assert_eq!(timeline.events[3].title, "Tool: skill_view");
        assert_eq!(
            timeline.events[1].metadata.message_ids,
            vec!["4", "feishu-msg-4"]
        );
        assert_eq!(
            timeline.events[2].metadata.tool_calls[0].name.as_deref(),
            Some("skill_view")
        );
    }

    #[test]
    fn load_timeline_deduplicates_adjacent_duplicate_messages() {
        let root = test_root("timeline-dedup");
        write_state_db(&root);
        let db = Connection::open(root.join("state.db")).expect("db");
        db.execute(
            "insert into messages (session_id, role, content, timestamp, active) values (?1, ?2, ?3, ?4, 1)",
            params!["hermes-feishu", "user", "Investigate handoff", 1780641496.0],
        )
        .expect("duplicate message");

        let timeline = HermesSourceAdapter::new(&root)
            .load_timeline("hermes-feishu")
            .expect("timeline");

        assert_eq!(
            timeline
                .events
                .iter()
                .filter(|event| event.detail == "Investigate handoff")
                .count(),
            1
        );
    }

    #[test]
    fn hermes_context_health_keeps_known_window_without_usage() {
        let context_health =
            hermes_context_health(None, Some("claude-sonnet-4-6"), 0).expect("context health");

        assert_eq!(context_health.used_tokens, None);
        assert_eq!(context_health.window_tokens, Some(1_000_000));
        assert_eq!(context_health.compact_layers, 0);
        assert!(context_health.source.contains("claude-sonnet-4-6"));
    }

    #[test]
    fn loads_explicit_hermes_session_outside_list_limit() {
        let root = test_root("explicit-outside-limit");
        write_state_db(&root);
        let adapter = HermesSourceAdapter::with_session_limit(&root, Some(1));

        let (listed, report) = adapter
            .list_sessions_with_report("included_real_store", "test")
            .expect("sessions");
        let found = adapter
            .find_session("hermes-cli")
            .expect("find session")
            .expect("old session");
        let timeline = adapter.load_timeline("hermes-cli").expect("old timeline");

        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, "hermes-feishu");
        assert_eq!(found.id, "hermes-cli");
        assert_eq!(timeline.source_session, "hermes-cli");
        assert_eq!(timeline.events[0].detail, "Fix CLI state");
        assert_eq!(report.fidelity.status, SourceFidelityStatus::Fallback);
        assert_eq!(report.fidelity.primary_surface, "hermes_local_sqlite");
        assert_eq!(
            report.fidelity.fallback_surface.as_deref(),
            Some("hermes_gateway_export_search")
        );
    }

    #[test]
    fn searches_continuation_points_from_local_sqlite_messages() {
        let root = test_root("continuation-search");
        write_state_db(&root);
        let adapter = HermesSourceAdapter::new(&root);

        let sessions = adapter
            .search_sessions("handoff", 1)
            .expect("search sessions");

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, "hermes-feishu");
        let metadata = sessions[0].provider_metadata.as_ref().expect("metadata");
        let search = metadata.search.as_ref().expect("search metadata");
        assert_eq!(search.backend, "local_sqlite_like");
        assert_eq!(search.query.as_deref(), Some("handoff"));
        assert_eq!(search.matched_message_count, 1);
        assert_eq!(search.continuation_point_count, 1);
        assert!(!search.truncated);
        let point = metadata
            .continuation_points
            .first()
            .expect("continuation point");
        assert_eq!(point.message_id, "4");
        assert_eq!(point.event_id.as_deref(), Some("evt-002"));
        assert_eq!(point.role, "user");
        assert!(point.snippet.contains("Investigate handoff"));
        assert_eq!(point.bookend_before.as_deref(), Some("source feishu"));
        assert!(
            point
                .bookend_after
                .as_deref()
                .expect("after bookend")
                .contains("skill_view")
        );
        assert_eq!(point.scroll_context.message_index, 2);
        assert_eq!(point.scroll_context.total_messages, 5);
        assert_eq!(point.scroll_context.before_message_id.as_deref(), Some("3"));
        assert_eq!(point.scroll_context.after_message_id.as_deref(), Some("5"));
    }

    fn test_root(name: &str) -> PathBuf {
        let root = env::temp_dir().join(format!(
            "moonbox-hermes-adapter-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("root");
        root
    }

    fn write_sessions_json(root: &std::path::Path, contents: &str) {
        let path = root.join("sessions").join("sessions.json");
        fs::create_dir_all(path.parent().expect("parent")).expect("dirs");
        fs::write(path, contents).expect("sessions json");
    }

    fn write_state_db(root: &std::path::Path) {
        let path = root.join("state.db");
        let db = Connection::open(path).expect("db");
        db.execute_batch(
            r#"
            create table sessions (
                id text primary key,
                source text not null,
                user_id text,
                model text,
                model_config text,
                system_prompt text,
                parent_session_id text,
                started_at real not null,
                ended_at real,
                end_reason text,
                message_count integer default 0,
                tool_call_count integer default 0,
                input_tokens integer default 0,
                output_tokens integer default 0,
                cache_read_tokens integer default 0,
                cache_write_tokens integer default 0,
                reasoning_tokens integer default 0,
                cwd text,
                billing_provider text,
                billing_base_url text,
                billing_mode text,
                estimated_cost_usd real,
                actual_cost_usd real,
                cost_status text,
                cost_source text,
                pricing_version text,
                title text,
                api_call_count integer default 0,
                handoff_state text,
                handoff_platform text,
                handoff_error text,
                rewind_count integer not null default 0,
                archived integer not null default 0
            );
            create table messages (
                id integer primary key autoincrement,
                session_id text not null,
                role text not null,
                content text,
                tool_call_id text,
                tool_calls text,
                tool_name text,
                timestamp real not null,
                token_count integer,
                finish_reason text,
                reasoning text,
                reasoning_content text,
                reasoning_details text,
                codex_reasoning_items text,
                codex_message_items text,
                platform_message_id text,
                observed integer default 0,
                active integer not null default 1
            );
            insert into sessions (
                id, source, user_id, model, model_config, system_prompt, parent_session_id,
                started_at, message_count, tool_call_count,
                input_tokens, output_tokens, cache_read_tokens, cache_write_tokens,
                reasoning_tokens, cwd, title, handoff_state, handoff_platform, handoff_error, archived
            ) values
                ('hermes-cli', 'cli', 'local-user', 'gpt-5', '{"temperature":0.2}', 'CLI system prompt', null, 1780640474, 2, 0, 10, 5, 0, 0, 0, '/repo', 'CLI bugfix', null, null, null, 0),
                ('hermes-feishu', 'feishu', 'ou_feishu', 'claude-sonnet-4-6', '{"mode":"ops"}', 'Feishu system prompt snapshot', 'parent-feishu', 1780641494, 5, 1, 0, 0, 0, 0, 0, null, null, 'ready', 'feishu', null, 0),
                ('hermes-discord-archived', 'discord', 'discord-user', 'gpt-5', null, null, null, 1780649999, 1, 0, 1, 1, 0, 0, 0, null, 'Archived Discord', null, null, null, 1);
            insert into messages (session_id, role, content, timestamp, active) values
                ('hermes-cli', 'user', 'Fix CLI state', 1780640475, 1),
                ('hermes-cli', 'assistant', 'Done', 1780640476, 1),
                ('hermes-feishu', 'session_meta', 'source feishu', 1780641495, 1),
                ('hermes-feishu', 'user', 'Investigate handoff', 1780641496, 1);
            insert into messages (session_id, role, tool_calls, timestamp, active) values
                ('hermes-feishu', 'assistant', '[{"name":"skill_view"}]', 1780641497, 1);
            insert into messages (session_id, role, content, tool_name, timestamp, active) values
                ('hermes-feishu', 'tool', 'skill body', 'skill_view', 1780641498, 1),
                ('hermes-feishu', 'assistant', 'Root cause found', 'skill_view', 1780641499, 1);
            update messages set platform_message_id = 'feishu-msg-4' where id = 4;
            "#,
        )
        .expect("schema");
    }

    fn write_legacy_state_db(root: &std::path::Path) {
        let path = root.join("state.db");
        let db = Connection::open(path).expect("db");
        db.execute_batch(
            r#"
            create table sessions (
                id text primary key,
                source text not null,
                model text,
                started_at real not null,
                message_count integer default 0,
                tool_call_count integer default 0,
                input_tokens integer default 0,
                output_tokens integer default 0,
                cache_read_tokens integer default 0,
                cache_write_tokens integer default 0,
                reasoning_tokens integer default 0,
                cwd text,
                title text
            );
            create table messages (
                id integer primary key autoincrement,
                session_id text not null,
                role text not null,
                content text,
                tool_calls text,
                tool_name text,
                timestamp real not null,
                token_count integer,
                finish_reason text,
                reasoning text,
                reasoning_content text,
                reasoning_details text,
                platform_message_id text
            );
            insert into sessions (
                id, source, model, started_at, message_count, input_tokens, output_tokens, title
            ) values
                ('legacy-cli', 'cli', 'gpt-5', 1780640474, 1, 1, 2, 'Legacy CLI'),
                ('legacy-feishu', 'feishu', 'claude-sonnet', 1780641494, 1, 2, 3, 'Legacy Feishu');
            insert into messages (session_id, role, content, timestamp) values
                ('legacy-cli', 'user', 'Fix legacy CLI', 1780640475),
                ('legacy-feishu', 'user', 'Fix legacy Feishu', 1780641495);
            "#,
        )
        .expect("legacy schema");
    }
}
