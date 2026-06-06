use std::{
    env, fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
};

use serde::Deserialize;
use serde_json::Value;

use super::{
    adapter::{AdapterError, SourceAdapter},
    model::{
        CanonicalTimeline, CliTool, SessionStatus, SessionSummary, TimelineEvent, TimelineKind,
    },
};

const CODEX_TOOL: CliTool = CliTool::Codex;
const DEFAULT_SESSION_LIMIT: usize = 200;

#[derive(Debug, Clone)]
pub struct CodexSourceAdapter {
    root: PathBuf,
}

#[derive(Debug, Deserialize)]
struct CodexRecord {
    timestamp: Option<String>,
    #[serde(rename = "type")]
    record_type: Option<String>,
    #[serde(default)]
    payload: Value,
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
    has_error: bool,
}

impl CodexSourceAdapter {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    #[cfg(not(test))]
    pub fn from_default_home() -> Option<Self> {
        if let Some(path) = env::var_os("MOONBOX_CODEX_HOME") {
            return Some(Self::new(path));
        }
        if let Some(path) = env::var_os("CODEX_HOME") {
            return Some(Self::new(path));
        }
        env::var_os("HOME").map(|home| Self::new(PathBuf::from(home).join(".codex")))
    }

    #[cfg(not(test))]
    pub fn has_session_store(&self) -> bool {
        self.sessions_dir().is_dir()
    }

    fn sessions_dir(&self) -> PathBuf {
        self.root.join("sessions")
    }

    fn session_files(&self) -> Result<Vec<PathBuf>, AdapterError> {
        let sessions_dir = self.sessions_dir();
        if !sessions_dir.exists() {
            return Ok(Vec::new());
        }

        let mut files = Vec::new();
        collect_jsonl_files(&sessions_dir, &mut files)?;
        files.sort_by(|left, right| right.cmp(left));
        if let Some(limit) = session_limit() {
            files.truncate(limit);
        }
        Ok(files)
    }

    fn parse_summary(&self, path: &Path) -> Result<SessionSummary, AdapterError> {
        let mut builder = SummaryBuilder::new(path);
        let reader = open_reader(path)?;

        for line in reader.lines() {
            let line = line.map_err(|error| read_error(path, error))?;
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<CodexRecord>(&line) {
                Ok(record) => builder.observe(record),
                Err(_) => builder.malformed_lines += 1,
            }
        }

        Ok(builder.finish())
    }

    fn find_session_path(&self, session_id: &str) -> Result<Option<PathBuf>, AdapterError> {
        for path in self.session_files()? {
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
    ) -> Result<CanonicalTimeline, AdapterError> {
        let reader = open_reader(path)?;
        let mut events = Vec::new();

        for (line_index, line) in reader.lines().enumerate() {
            let line = line.map_err(|error| read_error(path, error))?;
            if line.trim().is_empty() {
                continue;
            }

            let record = match serde_json::from_str::<CodexRecord>(&line) {
                Ok(record) => record,
                Err(error) => {
                    events.push(TimelineEvent {
                        id: event_id(events.len() + 1),
                        time: "??:??".into(),
                        kind: TimelineKind::Error,
                        title: "Malformed event".into(),
                        detail: format!("line {}: {}", line_index + 1, error),
                    });
                    continue;
                }
            };

            if let Some(event) = timeline_event(record, events.len() + 1) {
                events.push(event);
            }
        }

        Ok(CanonicalTimeline {
            version: 1,
            source_cli: CODEX_TOOL,
            source_session: session_id.into(),
            events,
        })
    }
}

impl SourceAdapter for CodexSourceAdapter {
    fn tool(&self) -> CliTool {
        CODEX_TOOL
    }

    fn list_sessions(&self) -> Result<Vec<SessionSummary>, AdapterError> {
        let mut sessions = Vec::new();
        for path in self.session_files()? {
            sessions.push(self.parse_summary(&path)?);
        }
        Ok(sessions)
    }

    fn load_timeline(&self, session_id: &str) -> Result<CanonicalTimeline, AdapterError> {
        let Some(path) = self.find_session_path(session_id)? else {
            return Err(AdapterError::SessionNotFound {
                tool: CODEX_TOOL,
                session_id: session_id.into(),
            });
        };
        self.parse_timeline(session_id, &path)
    }
}

impl SummaryBuilder {
    fn new(path: &Path) -> Self {
        Self {
            path: path.into(),
            id: None,
            title: None,
            cwd: None,
            updated_at: timestamp_from_filename(path),
            branch: None,
            token_count: None,
            event_count: 0,
            malformed_lines: 0,
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
        {
            self.title = Some(truncate(&text, 72));
        }
    }

    fn observe_event_msg(&mut self, payload: &Value) {
        if self.title.is_none()
            && string_field(payload, "type") == Some("user_message")
            && let Some(text) = text_from_value(payload)
        {
            self.title = Some(truncate(&text, 72));
        }
        if self.token_count.is_none()
            && string_field(payload, "type") == Some("token_count")
            && let Some(count) = find_token_count(payload)
        {
            self.token_count = Some(count);
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
        let health_reason = if self.malformed_lines > 0 {
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
            status,
            branch: self.branch,
            token_count: self.token_count,
            health_reason: Some(health_reason),
            event_count: self.event_count,
            resume_command: format!("codex resume {id}"),
        }
    }
}

fn collect_jsonl_files(root: &Path, files: &mut Vec<PathBuf>) -> Result<(), AdapterError> {
    let entries = fs::read_dir(root).map_err(|error| read_error(root, error))?;
    for entry in entries {
        let entry = entry.map_err(|error| read_error(root, error))?;
        let path = entry.path();
        if path.is_dir() {
            collect_jsonl_files(&path, files)?;
        } else if path
            .extension()
            .is_some_and(|extension| extension == "jsonl")
        {
            files.push(path);
        }
    }
    Ok(())
}

fn session_limit() -> Option<usize> {
    match env::var("MOONBOX_SESSION_LIMIT") {
        Ok(value) if value.trim() == "0" => None,
        Ok(value) => value
            .trim()
            .parse::<usize>()
            .ok()
            .filter(|limit| *limit > 0)
            .or(Some(DEFAULT_SESSION_LIMIT)),
        Err(_) => Some(DEFAULT_SESSION_LIMIT),
    }
}

fn open_reader(path: &Path) -> Result<BufReader<fs::File>, AdapterError> {
    let file = fs::File::open(path).map_err(|error| read_error(path, error))?;
    Ok(BufReader::new(file))
}

fn read_error(path: &Path, error: impl ToString) -> AdapterError {
    AdapterError::ReadSource {
        tool: CODEX_TOOL,
        path: path.to_string_lossy().into_owned(),
        reason: error.to_string(),
    }
}

fn timeline_event(record: CodexRecord, number: usize) -> Option<TimelineEvent> {
    let record_type = record.record_type.as_deref().unwrap_or_default();
    let payload_type = string_field(&record.payload, "type").unwrap_or_default();
    let role = string_field(&record.payload, "role");
    let kind = timeline_kind(record_type, payload_type, role)?;
    let title = timeline_title(record_type, payload_type, role);
    let detail = timeline_detail(&record.payload, record_type, payload_type);
    if detail.is_empty() && !matches!(kind, TimelineKind::Error) {
        return None;
    }

    Some(TimelineEvent {
        id: event_id(number),
        time: display_time(record.timestamp.as_deref()),
        kind,
        title,
        detail,
    })
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
        .map(|text| truncate(&text, 220))
        .unwrap_or_default()
}

fn text_from_value(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => normalize_text(text),
        Value::Array(items) => {
            let text = items
                .iter()
                .filter_map(text_from_value)
                .collect::<Vec<_>>()
                .join(" ");
            normalize_text(&text)
        }
        Value::Object(object) => {
            for key in [
                "text",
                "message",
                "cmd",
                "command",
                "name",
                "last_agent_message",
            ] {
                if let Some(value) = object.get(key)
                    && let Some(text) = text_from_value(value)
                {
                    return Some(text);
                }
            }
            None
        }
        _ => None,
    }
}

fn normalize_text(text: &str) -> Option<String> {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

fn find_token_count(value: &Value) -> Option<usize> {
    match value {
        Value::Number(number) => number
            .as_u64()
            .and_then(|count| usize::try_from(count).ok()),
        Value::Array(items) => items.iter().find_map(find_token_count),
        Value::Object(object) => {
            for key in ["total_tokens", "total_token_count", "used_tokens"] {
                if let Some(value) = object.get(key)
                    && let Some(count) = find_token_count(value)
                {
                    return Some(count);
                }
            }
            object.values().find_map(find_token_count)
        }
        _ => None,
    }
}

fn string_field<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

fn is_error_record(record_type: &str, payload_type: &str) -> bool {
    record_type.contains("error") || payload_type.contains("error")
}

fn max_timestamp(current: Option<String>, candidate: &str) -> String {
    match current {
        Some(current) if current.as_str() > candidate => current,
        _ => candidate.into(),
    }
}

fn timestamp_from_filename(path: &Path) -> Option<String> {
    let stem = path.file_stem()?.to_str()?;
    let timestamp = stem.strip_prefix("rollout-")?.get(..19)?;
    Some(format!("{}+00:00", timestamp.replace_time_dashes()))
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

fn human_timestamp(timestamp: &str) -> String {
    let normalized = timestamp.replace_time_dashes();
    normalized
        .get(..16)
        .map(|prefix| prefix.replace('T', " "))
        .unwrap_or_else(|| normalized)
}

fn display_time(timestamp: Option<&str>) -> String {
    let Some(timestamp) = timestamp else {
        return "??:??".into();
    };
    let normalized = timestamp.replace_time_dashes();
    normalized
        .split('T')
        .nth(1)
        .and_then(|time| time.get(..5))
        .unwrap_or("??:??")
        .into()
}

fn event_id(number: usize) -> String {
    format!("evt-{number:03}")
}

fn truncate(text: &str, max_chars: usize) -> String {
    let mut output = String::new();
    for (index, character) in text.chars().enumerate() {
        if index == max_chars {
            output.push_str("...");
            return output;
        }
        output.push(character);
    }
    output
}

fn title_case(value: &str) -> String {
    value
        .split('_')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => format!("{}{}", first.to_ascii_uppercase(), chars.as_str()),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

trait TimeDashReplace {
    fn replace_time_dashes(&self) -> String;
}

impl TimeDashReplace for str {
    fn replace_time_dashes(&self) -> String {
        let Some((date, rest)) = self.split_once('T') else {
            return self.into();
        };
        let mut chars = rest.chars().collect::<Vec<_>>();
        if chars.len() >= 8 {
            chars[2] = ':';
            chars[5] = ':';
        }
        format!("{date}T{}", chars.into_iter().collect::<String>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn lists_codex_sessions_from_jsonl_store() {
        let root = test_root("list");
        write_session(
            &root,
            "2026/06/06/rollout-2026-06-06T08-00-00-test.jsonl",
            r#"{"timestamp":"2026-06-06T08:00:00.000Z","type":"session_meta","payload":{"id":"codex-real-1","cwd":"/repo","git":{"branch":"main"}}}
{"timestamp":"2026-06-06T08:01:00.000Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"Implement a real adapter"}]}}
{"timestamp":"2026-06-06T08:02:00.000Z","type":"event_msg","payload":{"type":"token_count","info":{"total_tokens":42}}}
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
        assert_eq!(sessions[0].resume_command, "codex resume codex-real-1");
    }

    #[test]
    fn loads_canonical_timeline_from_jsonl_store() {
        let root = test_root("timeline");
        write_session(
            &root,
            "2026/06/06/rollout-2026-06-06T08-00-00-test.jsonl",
            r#"{"timestamp":"2026-06-06T08:00:00.000Z","type":"session_meta","payload":{"id":"codex-real-2","cwd":"/repo"}}
{"timestamp":"2026-06-06T08:01:00.000Z","type":"event_msg","payload":{"type":"user_message","message":"Start here"}}
{"timestamp":"2026-06-06T08:02:00.000Z","type":"event_msg","payload":{"type":"agent_message","message":"Done"}}
{"timestamp":"2026-06-06T08:03:00.000Z","type":"event_msg","payload":{"type":"error","message":"resume failed"}}
"#,
        );

        let timeline = CodexSourceAdapter::new(&root)
            .load_timeline("codex-real-2")
            .expect("timeline");

        assert_eq!(timeline.source_cli, CliTool::Codex);
        assert_eq!(timeline.source_session, "codex-real-2");
        assert_eq!(timeline.events[0].kind, TimelineKind::Tool);
        assert_eq!(timeline.events[1].kind, TimelineKind::User);
        assert_eq!(timeline.events[2].kind, TimelineKind::Assistant);
        assert_eq!(timeline.events[3].kind, TimelineKind::Error);
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

    fn write_session(root: &Path, relative_path: &str, contents: &str) {
        let path = root.join("sessions").join(relative_path);
        fs::create_dir_all(path.parent().expect("parent")).expect("dirs");
        let mut file = fs::File::create(path).expect("file");
        file.write_all(contents.as_bytes()).expect("write");
    }
}
