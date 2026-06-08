use std::{
    collections::HashMap,
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
        is_provider_context_text, push_timeline_event, read_error, text_from_value,
        timeline_preview_truncated_event, title_case, truncate,
    },
    model::{
        CanonicalTimeline, CliTool, SessionStatus, SessionSummary, SourceProvenance, TimelineEvent,
        TimelineKind,
    },
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
    model: Option<String>,
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
    archived: bool,
    active_message_count: usize,
    preview: String,
}

#[derive(Debug, Clone)]
struct HermesMessageRow {
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
        let query = format!(
            "{} {}",
            SESSION_SELECT,
            match limit {
                Some(_) =>
                    "where s.source = 'cli' and s.archived = 0 order by s.started_at desc limit :limit",
                None => "where s.source = 'cli' and s.archived = 0 order by s.started_at desc",
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
        let mut statement = db
            .prepare(&format!("{SESSION_SELECT} where s.id = ?1"))
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
        let limit_clause = if event_limit.is_some() {
            "limit ?2"
        } else {
            ""
        };
        let mut statement = db
            .prepare(&format!(
                r#"
                select
                    role,
                    content,
                    tool_calls,
                    tool_name,
                    strftime('%Y-%m-%dT%H:%M:%SZ', timestamp, 'unixepoch') as timestamp,
                    token_count,
                    finish_reason,
                    reasoning,
                    reasoning_content,
                    reasoning_details
                from messages
                where session_id = ?1 and active = 1
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

    fn summary_for_row(
        &self,
        row: HermesSessionRow,
        registry: &HashMap<String, SessionRegistryEntry>,
    ) -> SessionSummary {
        let supplement = registry.get(&row.id);
        let token_count = supplement
            .and_then(|entry| entry.total_tokens)
            .or_else(|| total_tokens(&row));
        let status = session_status(&row, supplement);
        let health_reason = health_reason(&row, supplement);
        let title = row
            .title
            .clone()
            .or_else(|| supplement.and_then(display_name))
            .or_else(|| {
                let preview = row.preview.trim();
                (!preview.is_empty() && !is_provider_context_text(preview))
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
            status,
            branch: None,
            token_count,
            health_reason: Some(health_reason),
            event_count: row.active_message_count.max(row.message_count),
            resume_command: format!("hermes --resume {}", row.id),
            source_provenance: SourceProvenance::Real,
            source_path: Some(self.state_db_path().display().to_string()),
            parse_skip_count: 0,
        }
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
            if let Some(event) = timeline_event(row, events.len() + 1) {
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

const SESSION_SELECT: &str = r#"
    select
        s.id,
        s.source,
        s.model,
        strftime(
            '%Y-%m-%dT%H:%M:%SZ',
            coalesce((select max(timestamp) from messages where session_id = s.id), s.ended_at, s.started_at),
            'unixepoch'
        ) as updated_at,
        s.end_reason,
        coalesce(s.message_count, 0) as message_count,
        coalesce(s.tool_call_count, 0) as tool_call_count,
        coalesce(s.input_tokens, 0) as input_tokens,
        coalesce(s.output_tokens, 0) as output_tokens,
        coalesce(s.cache_read_tokens, 0) as cache_read_tokens,
        coalesce(s.cache_write_tokens, 0) as cache_write_tokens,
        coalesce(s.reasoning_tokens, 0) as reasoning_tokens,
        s.cwd,
        s.title,
        s.handoff_state,
        s.handoff_platform,
        s.handoff_error,
        coalesce(s.rewind_count, 0) as rewind_count,
        s.archived != 0 as archived,
        coalesce((select count(*) from messages where session_id = s.id and active = 1), 0) as active_message_count,
        coalesce(
            (
                select substr(replace(replace(m.content, char(10), ' '), char(13), ' '), 1, 63)
                from messages m
                where m.session_id = s.id and m.role = 'user' and m.content is not null
                order by m.timestamp asc, m.id asc
                limit 1
            ),
            ''
        ) as preview
    from sessions s
"#;

fn session_row(row: &Row<'_>) -> rusqlite::Result<HermesSessionRow> {
    Ok(HermesSessionRow {
        id: row.get(0)?,
        source: row.get(1)?,
        model: row.get(2)?,
        updated_at: row.get(3)?,
        end_reason: row.get(4)?,
        message_count: integer(row, 5)?,
        tool_call_count: integer(row, 6)?,
        input_tokens: integer(row, 7)?,
        output_tokens: integer(row, 8)?,
        cache_read_tokens: integer(row, 9)?,
        cache_write_tokens: integer(row, 10)?,
        reasoning_tokens: integer(row, 11)?,
        cwd: row.get(12)?,
        title: row.get(13)?,
        handoff_state: row.get(14)?,
        handoff_platform: row.get(15)?,
        handoff_error: row.get(16)?,
        rewind_count: integer(row, 17)?,
        archived: row.get(18)?,
        active_message_count: integer(row, 19)?,
        preview: row.get(20)?,
    })
}

fn message_row(row: &Row<'_>) -> rusqlite::Result<HermesMessageRow> {
    Ok(HermesMessageRow {
        role: row.get(0)?,
        content: row.get(1)?,
        tool_calls: row.get(2)?,
        tool_name: row.get(3)?,
        timestamp: row.get(4)?,
        token_count: optional_integer(row, 5)?,
        finish_reason: row.get(6)?,
        reasoning: row.get(7)?,
        reasoning_content: row.get(8)?,
        reasoning_details: row.get(9)?,
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

fn total_tokens(row: &HermesSessionRow) -> Option<usize> {
    row.input_tokens
        .checked_add(row.output_tokens)?
        .checked_add(row.cache_read_tokens)?
        .checked_add(row.cache_write_tokens)?
        .checked_add(row.reasoning_tokens)
        .filter(|tokens| *tokens > 0)
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
    if let Some(model) = row.model.as_deref().filter(|model| !model.is_empty()) {
        parts.push(format!("model: {model}"));
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

fn timeline_event(row: HermesMessageRow, number: usize) -> Option<TimelineEvent> {
    let kind = timeline_kind(&row)?;
    let detail = timeline_detail(&row);
    if detail.is_empty() && !matches!(kind, TimelineKind::Error) {
        return None;
    }
    if kind == TimelineKind::User && is_provider_context_text(&detail) {
        return None;
    }

    Some(TimelineEvent {
        id: event_id(number),
        time: display_time(Some(&row.timestamp)),
        kind,
        title: timeline_title(&row),
        detail,
    })
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
        .map(|detail| truncate(&detail, 220))
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

fn short_id(id: &str) -> String {
    id.chars().take(8).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn lists_hermes_sessions_from_sqlite_store() {
        let root = test_root("list");
        write_state_db(&root);
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

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, "hermes-cli");
        assert_eq!(sessions[0].title, "CLI bugfix");
        assert_eq!(sessions[0].token_count, Some(15));
        assert_eq!(sessions[0].resume_command, "hermes --resume hermes-cli");
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
    fn loads_explicit_hermes_session_outside_list_limit() {
        let root = test_root("explicit-outside-limit");
        write_state_db(&root);
        let adapter = HermesSourceAdapter::with_session_limit(&root, Some(1));

        let listed = adapter.list_sessions().expect("sessions");
        let found = adapter
            .find_session("hermes-feishu")
            .expect("find session")
            .expect("old session");
        let timeline = adapter
            .load_timeline("hermes-feishu")
            .expect("old timeline");

        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, "hermes-cli");
        assert_eq!(found.id, "hermes-feishu");
        assert_eq!(timeline.source_session, "hermes-feishu");
        assert_eq!(timeline.events[1].detail, "Investigate handoff");
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
                id, source, model, started_at, message_count, tool_call_count,
                input_tokens, output_tokens, cache_read_tokens, cache_write_tokens,
                reasoning_tokens, cwd, title
            ) values
                ('hermes-cli', 'cli', 'gpt-5', 1780640474, 2, 0, 10, 5, 0, 0, 0, '/repo', 'CLI bugfix'),
                ('hermes-feishu', 'feishu', 'claude-sonnet', 1780641494, 5, 1, 0, 0, 0, 0, 0, null, null);
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
            "#,
        )
        .expect("schema");
    }
}
