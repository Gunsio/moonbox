#![cfg_attr(test, allow(dead_code))]

use std::{
    env, fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Map, Value, json};
use time::OffsetDateTime;

use super::{
    adapter::AdapterError,
    local_jsonl::{
        display_time, event_id, human_timestamp, is_provider_context_text, push_timeline_event,
        text_from_value, title_case, truncate, truncate_timeline_detail,
    },
    model::{
        CanonicalTimeline, CliTool, SessionRuntimeStatus, SessionStatus, SessionSummary,
        SourceProvenance, TimelineEvent, TimelineKind,
    },
};

const CODEX_TOOL: CliTool = CliTool::Codex;
const APP_SOURCE_PREFIX: &str = "codex-app-server://threads/";
const DEFAULT_PROXY_TIMEOUT_MS: u64 = 2_000;

pub(crate) const CODEX_APP_SERVER_FIXTURE_ENV: &str = "MOONBOX_CODEX_APP_SERVER_FIXTURE";
pub(crate) const CODEX_APP_SERVER_PROXY_ENV: &str = "MOONBOX_CODEX_APP_SERVER_PROXY";
pub(crate) const CODEX_APP_SERVER_SOCKET_ENV: &str = "MOONBOX_CODEX_APP_SERVER_SOCKET";
pub(crate) const CODEX_APP_SERVER_TIMEOUT_MS_ENV: &str = "MOONBOX_CODEX_APP_SERVER_TIMEOUT_MS";
pub(crate) const CODEX_BIN_ENV: &str = "MOONBOX_CODEX_BIN";

#[derive(Debug, Clone)]
pub(crate) struct CodexAppServerSource {
    transport: CodexAppServerTransport,
}

#[derive(Debug, Clone)]
enum CodexAppServerTransport {
    Fixture(PathBuf),
    Proxy {
        program: String,
        args: Vec<String>,
        timeout: Duration,
    },
}

#[derive(Debug, Deserialize)]
struct FixtureRpc {
    responses: Vec<FixtureRpcResponse>,
}

#[derive(Debug, Deserialize)]
struct FixtureRpcResponse {
    method: String,
    #[serde(default)]
    thread_id: Option<String>,
    #[serde(default)]
    cursor: Option<String>,
    result: Value,
}

#[derive(Debug, Deserialize)]
struct ThreadListResponse {
    data: Vec<CodexAppThread>,
}

#[derive(Debug, Deserialize)]
struct ThreadReadResponse {
    thread: CodexAppThread,
}

#[derive(Debug, Deserialize)]
struct ThreadTurnsListResponse {
    data: Vec<CodexAppTurn>,
    #[serde(default, rename = "nextCursor")]
    next_cursor: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct CodexAppThread {
    id: String,
    #[serde(default)]
    name: Option<String>,
    cwd: String,
    #[serde(default)]
    preview: String,
    #[serde(default)]
    path: Option<String>,
    #[serde(default, rename = "updatedAt")]
    updated_at: Option<i64>,
    #[serde(default, rename = "createdAt")]
    created_at: Option<i64>,
    #[serde(default, rename = "gitInfo")]
    git_info: Option<CodexAppGitInfo>,
    #[serde(default)]
    status: Value,
    #[serde(default)]
    turns: Vec<CodexAppTurn>,
}

#[derive(Debug, Clone, Deserialize)]
struct CodexAppGitInfo {
    #[serde(default)]
    branch: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct CodexAppTurn {
    #[serde(default, rename = "startedAt")]
    started_at: Option<i64>,
    #[serde(default, rename = "completedAt")]
    completed_at: Option<i64>,
    #[serde(default)]
    status: String,
    #[serde(default)]
    error: Option<CodexAppTurnError>,
    #[serde(default)]
    items: Vec<Value>,
}

#[derive(Debug, Clone, Deserialize)]
struct CodexAppTurnError {
    message: String,
    #[serde(default, rename = "additionalDetails")]
    additional_details: Option<String>,
}

impl CodexAppServerSource {
    pub(crate) fn from_env() -> Option<Self> {
        if let Some(path) = env::var_os(CODEX_APP_SERVER_FIXTURE_ENV) {
            return Some(Self::fixture(path));
        }
        if !env_enabled(CODEX_APP_SERVER_PROXY_ENV) {
            return None;
        }

        let program = env::var(CODEX_BIN_ENV).unwrap_or_else(|_| "codex".into());
        let mut args = vec!["app-server".into(), "proxy".into()];
        if let Some(socket) = env::var_os(CODEX_APP_SERVER_SOCKET_ENV) {
            args.push("--sock".into());
            args.push(socket.to_string_lossy().into_owned());
        }

        Some(Self {
            transport: CodexAppServerTransport::Proxy {
                program,
                args,
                timeout: configured_timeout(),
            },
        })
    }

    pub(crate) fn fixture(path: impl Into<PathBuf>) -> Self {
        Self {
            transport: CodexAppServerTransport::Fixture(path.into()),
        }
    }

    pub(crate) fn description(&self) -> String {
        match &self.transport {
            CodexAppServerTransport::Fixture(path) => {
                format!("Codex app-server fixture {}", path.display())
            }
            CodexAppServerTransport::Proxy { program, args, .. } => {
                format!("{} {}", program, args.join(" "))
            }
        }
    }

    pub(crate) fn store_path(&self) -> Option<String> {
        match &self.transport {
            CodexAppServerTransport::Fixture(path) => Some(path.display().to_string()),
            CodexAppServerTransport::Proxy { program, args, .. } => {
                Some(format!("{} {}", program, args.join(" ")))
            }
        }
    }

    pub(crate) fn thread_source_path(thread_id: &str) -> String {
        format!("{APP_SOURCE_PREFIX}{thread_id}")
    }

    pub(crate) fn is_thread_source_path(path: &str) -> bool {
        path.starts_with(APP_SOURCE_PREFIX)
    }

    pub(crate) fn deep_link(thread_id: &str) -> String {
        format!("codex://threads/{thread_id}")
    }

    pub(crate) fn list_threads(
        &self,
        limit: Option<usize>,
    ) -> Result<Vec<CodexAppThread>, AdapterError> {
        let mut params = Map::new();
        params.insert("archived".into(), Value::Bool(false));
        params.insert("sortDirection".into(), Value::String("desc".into()));
        params.insert("sortKey".into(), Value::String("updated_at".into()));
        if let Some(limit) = limit {
            params.insert("limit".into(), json!(limit));
        }
        let response = self.request::<ThreadListResponse>("thread/list", Value::Object(params))?;
        Ok(response.data)
    }

    pub(crate) fn read_thread(&self, thread_id: &str) -> Result<CodexAppThread, AdapterError> {
        let response = self.request::<ThreadReadResponse>(
            "thread/read",
            json!({
                "threadId": thread_id,
                "includeTurns": false,
            }),
        )?;
        Ok(response.thread)
    }

    pub(crate) fn load_timeline_limited(
        &self,
        thread_id: &str,
        event_limit: Option<usize>,
    ) -> Result<CanonicalTimeline, AdapterError> {
        let mut turns = Vec::new();
        let mut cursor = None;
        loop {
            let response = self.list_turns_page(thread_id, cursor.as_deref(), event_limit)?;
            turns.extend(response.data);
            cursor = response.next_cursor;
            if cursor.is_none() || event_limit.is_some_and(|limit| turns.len() >= limit) {
                break;
            }
        }
        Ok(timeline_from_turns(thread_id, &turns, event_limit))
    }

    fn list_turns_page(
        &self,
        thread_id: &str,
        cursor: Option<&str>,
        event_limit: Option<usize>,
    ) -> Result<ThreadTurnsListResponse, AdapterError> {
        let mut params = Map::new();
        params.insert("threadId".into(), Value::String(thread_id.into()));
        params.insert("sortDirection".into(), Value::String("asc".into()));
        params.insert("itemsView".into(), Value::String("full".into()));
        if let Some(cursor) = cursor {
            params.insert("cursor".into(), Value::String(cursor.into()));
        }
        if let Some(limit) = event_limit {
            params.insert("limit".into(), json!(limit));
        }
        self.request("thread/turns/list", Value::Object(params))
    }

    fn request<T: DeserializeOwned>(&self, method: &str, params: Value) -> Result<T, AdapterError> {
        let result = match &self.transport {
            CodexAppServerTransport::Fixture(path) => fixture_result(path, method, &params),
            CodexAppServerTransport::Proxy {
                program,
                args,
                timeout,
            } => proxy_result(program, args, *timeout, method, params),
        }
        .map_err(|reason| self.read_error(method, reason))?;
        serde_json::from_value(result)
            .map_err(|error| self.read_error(method, format!("invalid {method} response: {error}")))
    }

    fn read_error(&self, method: &str, reason: impl Into<String>) -> AdapterError {
        AdapterError::ReadSource {
            tool: CODEX_TOOL,
            path: format!("{} {method}", self.description()),
            reason: reason.into(),
        }
    }
}

pub(crate) fn app_thread_summary(thread: CodexAppThread) -> SessionSummary {
    let updated_at = thread
        .updated_at
        .or(thread.created_at)
        .and_then(unix_seconds_to_rfc3339)
        .unwrap_or_else(|| "1970-01-01T00:00:00.000Z".into());
    let title = first_non_empty([
        thread.name.as_deref(),
        Some(thread.preview.as_str()),
        thread.path.as_deref(),
    ])
    .filter(|title| !is_provider_context_text(title))
    .map(|title| truncate(&title.split_whitespace().collect::<Vec<_>>().join(" "), 160))
    .unwrap_or_else(|| format!("Codex thread {}", short_id(&thread.id)));
    let status_type = thread_status_type(&thread.status);
    let (status, runtime_status) = match status_type {
        "active" => (SessionStatus::Healthy, SessionRuntimeStatus::Active),
        "idle" => (SessionStatus::Healthy, SessionRuntimeStatus::Inactive),
        "systemError" => (SessionStatus::Failed, SessionRuntimeStatus::Unknown),
        _ => (SessionStatus::Warning, SessionRuntimeStatus::Unknown),
    };
    let branch = thread.git_info.as_ref().and_then(|git| git.branch.clone());
    let event_count = app_thread_event_count(&thread);

    SessionSummary {
        id: thread.id.clone(),
        cli: CODEX_TOOL,
        title,
        cwd: if thread.cwd.trim().is_empty() {
            "~".into()
        } else {
            thread.cwd
        },
        updated: human_timestamp(&updated_at),
        updated_at,
        runtime_status,
        runtime_reason: Some(format!("Codex app-server thread status: {status_type}")),
        status,
        branch,
        token_count: None,
        health_reason: Some("Codex app-server thread/list/read source".into()),
        event_count,
        resume_command: format!("codex resume {}", thread.id),
        source_provenance: SourceProvenance::Real,
        source_path: Some(CodexAppServerSource::thread_source_path(&thread.id)),
        parse_skip_count: 0,
        provider_metadata: None,
    }
}

fn fixture_result(path: &Path, method: &str, params: &Value) -> Result<Value, String> {
    let contents = fs::read_to_string(path)
        .map_err(|error| format!("cannot read app-server fixture: {error}"))?;
    let fixture = serde_json::from_str::<FixtureRpc>(&contents)
        .map_err(|error| format!("invalid app-server fixture: {error}"))?;
    let thread_id = params.get("threadId").and_then(Value::as_str);
    let cursor = params.get("cursor").and_then(Value::as_str);
    fixture
        .responses
        .into_iter()
        .find(|response| {
            response.method == method
                && response
                    .thread_id
                    .as_deref()
                    .is_none_or(|id| Some(id) == thread_id)
                && response
                    .cursor
                    .as_deref()
                    .is_none_or(|value| Some(value) == cursor)
        })
        .map(|response| response.result)
        .ok_or_else(|| format!("fixture has no response for {method}"))
}

fn proxy_result(
    program: &str,
    args: &[String],
    timeout: Duration,
    method: &str,
    params: Value,
) -> Result<Value, String> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("cannot start Codex app-server proxy: {error}"))?;

    if let Some(mut stdin) = child.stdin.take() {
        let request = json!({
            "id": 1,
            "method": method,
            "params": params,
        });
        serde_json::to_writer(&mut stdin, &request)
            .map_err(|error| format!("cannot encode JSON-RPC request: {error}"))?;
        stdin
            .write_all(b"\n")
            .map_err(|error| format!("cannot write JSON-RPC request: {error}"))?;
    }

    let timed_out = wait_for_child(&mut child, timeout)?;
    let mut stdout = String::new();
    if let Some(mut pipe) = child.stdout.take() {
        pipe.read_to_string(&mut stdout)
            .map_err(|error| format!("cannot read app-server proxy stdout: {error}"))?;
    }
    let mut stderr = String::new();
    if let Some(mut pipe) = child.stderr.take() {
        pipe.read_to_string(&mut stderr)
            .map_err(|error| format!("cannot read app-server proxy stderr: {error}"))?;
    }
    if timed_out {
        return Err(format!(
            "app-server proxy timed out after {} ms",
            timeout.as_millis()
        ));
    }
    parse_rpc_result(method, &stdout).map_err(|error| {
        let stderr = stderr.trim();
        if stderr.is_empty() {
            error
        } else {
            format!("{error}; stderr: {stderr}")
        }
    })
}

fn wait_for_child(child: &mut std::process::Child, timeout: Duration) -> Result<bool, String> {
    let deadline = Instant::now() + timeout;
    loop {
        if child
            .try_wait()
            .map_err(|error| format!("cannot wait for app-server proxy: {error}"))?
            .is_some()
        {
            return Ok(false);
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(true);
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn parse_rpc_result(method: &str, stdout: &str) -> Result<Value, String> {
    for line in stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        let value = serde_json::from_str::<Value>(line)
            .map_err(|error| format!("invalid JSON-RPC response for {method}: {error}"))?;
        if let Some(result) = value.get("result") {
            return Ok(result.clone());
        }
        if let Some(error) = value.get("error") {
            return Err(format!("{method} returned JSON-RPC error: {error}"));
        }
    }
    Err(format!("{method} returned no JSON-RPC result"))
}

fn timeline_from_turns(
    thread_id: &str,
    turns: &[CodexAppTurn],
    event_limit: Option<usize>,
) -> CanonicalTimeline {
    let mut events = Vec::new();
    'turns: for turn in turns {
        if let Some(event) = turn_error_event(turn, events.len() + 1)
            && push_timeline_event(&mut events, event, event_limit)
        {
            break 'turns;
        }
        for item in &turn.items {
            if let Some(event) = app_thread_item_event(turn, item, events.len() + 1)
                && push_timeline_event(&mut events, event, event_limit)
            {
                break 'turns;
            }
        }
    }
    CanonicalTimeline {
        version: 1,
        source_cli: CODEX_TOOL,
        source_session: thread_id.into(),
        events,
    }
}

fn app_thread_item_event(
    turn: &CodexAppTurn,
    item: &Value,
    number: usize,
) -> Option<TimelineEvent> {
    let item_type = item.get("type").and_then(Value::as_str).unwrap_or_default();
    let (kind, title, detail) = match item_type {
        "userMessage" => (
            TimelineKind::User,
            "User".into(),
            user_message_detail(item)?,
        ),
        "agentMessage" => (
            TimelineKind::Assistant,
            "Assistant".into(),
            string_value(item, "text")?,
        ),
        "plan" => (
            TimelineKind::Assistant,
            "Plan".into(),
            string_value(item, "text")?,
        ),
        "reasoning" => (
            TimelineKind::Tool,
            "Reasoning".into(),
            reasoning_detail(item)?,
        ),
        "commandExecution" => (
            TimelineKind::Tool,
            "Command".into(),
            command_execution_detail(item)?,
        ),
        "fileChange" => (
            TimelineKind::GitDiff,
            "File change".into(),
            text_from_value(item)?,
        ),
        "mcpToolCall" => (
            TimelineKind::Tool,
            "MCP tool".into(),
            tool_call_detail(item)?,
        ),
        "dynamicToolCall" => (
            TimelineKind::Tool,
            "Dynamic tool".into(),
            tool_call_detail(item)?,
        ),
        "collabAgentToolCall" => (
            TimelineKind::Tool,
            "Collab agent".into(),
            tool_call_detail(item)?,
        ),
        "webSearch" => (
            TimelineKind::Tool,
            "Web search".into(),
            string_value(item, "query")?,
        ),
        "hookPrompt" => (
            TimelineKind::Tool,
            "Hook prompt".into(),
            hook_prompt_detail(item)?,
        ),
        "contextCompaction" => (
            TimelineKind::Compact,
            "Context compaction".into(),
            "Codex compacted thread context".into(),
        ),
        "imageView" => (
            TimelineKind::Tool,
            "Image".into(),
            string_value(item, "path")?,
        ),
        "imageGeneration" => (
            TimelineKind::Tool,
            "Image generation".into(),
            string_value(item, "result").unwrap_or_else(|| "image generation".into()),
        ),
        "enteredReviewMode" | "exitedReviewMode" => (
            TimelineKind::Tool,
            title_case(item_type),
            string_value(item, "review").unwrap_or_else(|| title_case(item_type)),
        ),
        _ => return None,
    };
    if kind == TimelineKind::User && is_provider_context_text(&detail) {
        return None;
    }
    Some(TimelineEvent {
        id: event_id(number),
        time: turn_time(turn),
        kind,
        title,
        detail: truncate_timeline_detail(&detail),
    })
}

fn turn_error_event(turn: &CodexAppTurn, number: usize) -> Option<TimelineEvent> {
    if turn.status != "failed" {
        return None;
    }
    let error = turn.error.as_ref()?;
    let mut detail = error.message.clone();
    if let Some(additional) = error
        .additional_details
        .as_deref()
        .filter(|additional| !additional.trim().is_empty())
    {
        detail.push_str(": ");
        detail.push_str(additional);
    }
    Some(TimelineEvent {
        id: event_id(number),
        time: turn_time(turn),
        kind: TimelineKind::Error,
        title: "Turn failed".into(),
        detail: truncate_timeline_detail(&detail),
    })
}

fn user_message_detail(item: &Value) -> Option<String> {
    let content = item.get("content")?;
    if let Some(text) = text_from_value(content) {
        return Some(text);
    }
    content
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(user_input_label)
                .collect::<Vec<_>>()
                .join(" ")
        })
        .filter(|text| !text.trim().is_empty())
}

fn user_input_label(value: &Value) -> Option<String> {
    let kind = value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    match kind {
        "text" => string_value(value, "text"),
        "image" => string_value(value, "url").map(|url| format!("[image] {url}")),
        "localImage" => string_value(value, "path").map(|path| format!("[image] {path}")),
        "skill" | "mention" => {
            let name = string_value(value, "name")?;
            let path = string_value(value, "path").unwrap_or_default();
            Some(format!("@{name} {path}").trim().into())
        }
        _ => None,
    }
}

fn reasoning_detail(item: &Value) -> Option<String> {
    let summary = item.get("summary").and_then(text_from_value);
    let content = item.get("content").and_then(text_from_value);
    first_non_empty([summary.as_deref(), content.as_deref()]).map(str::to_owned)
}

fn command_execution_detail(item: &Value) -> Option<String> {
    let command = string_value(item, "command")?;
    let status = string_value(item, "status").unwrap_or_else(|| "unknown".into());
    let mut detail = format!("{command} [{status}]");
    if let Some(exit_code) = item.get("exitCode").and_then(Value::as_i64) {
        detail.push_str(&format!(" exit={exit_code}"));
    }
    if let Some(output) =
        string_value(item, "aggregatedOutput").filter(|output| !output.trim().is_empty())
    {
        detail.push('\n');
        detail.push_str(&output);
    }
    Some(detail)
}

fn tool_call_detail(item: &Value) -> Option<String> {
    let tool = string_value(item, "tool")
        .or_else(|| string_value(item, "server"))
        .or_else(|| text_from_value(item))?;
    let status = string_value(item, "status");
    Some(match status {
        Some(status) => format!("{tool} [{status}]"),
        None => tool,
    })
}

fn hook_prompt_detail(item: &Value) -> Option<String> {
    item.get("fragments")
        .and_then(text_from_value)
        .or_else(|| Some("hook prompt".into()))
}

fn string_value(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn turn_time(turn: &CodexAppTurn) -> String {
    turn.started_at
        .or(turn.completed_at)
        .and_then(unix_seconds_to_rfc3339)
        .map(|timestamp| display_time(Some(&timestamp)))
        .unwrap_or_else(|| "??:??".into())
}

fn thread_status_type(value: &Value) -> &str {
    value
        .get("type")
        .and_then(Value::as_str)
        .or_else(|| value.as_str())
        .unwrap_or("unknown")
}

fn app_thread_event_count(thread: &CodexAppThread) -> usize {
    let item_count = thread
        .turns
        .iter()
        .map(|turn| turn.items.len())
        .sum::<usize>();
    if item_count == 0 {
        thread.turns.len()
    } else {
        item_count
    }
}

fn unix_seconds_to_rfc3339(seconds: i64) -> Option<String> {
    let time = OffsetDateTime::from_unix_timestamp(seconds).ok()?;
    Some(format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.000Z",
        time.year(),
        u8::from(time.month()),
        time.day(),
        time.hour(),
        time.minute(),
        time.second(),
    ))
}

fn first_non_empty<'a>(values: impl IntoIterator<Item = Option<&'a str>>) -> Option<&'a str> {
    values
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|value| !value.is_empty() && *value != "-")
}

fn short_id(id: &str) -> String {
    id.chars().take(8).collect()
}

fn env_enabled(key: &str) -> bool {
    env::var(key)
        .ok()
        .map(|value| {
            let normalized = value.trim().to_ascii_lowercase();
            matches!(normalized.as_str(), "1" | "true" | "yes" | "proxy")
        })
        .unwrap_or(false)
}

fn configured_timeout() -> Duration {
    let millis = env::var(CODEX_APP_SERVER_TIMEOUT_MS_ENV)
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|millis| *millis >= 100)
        .unwrap_or(DEFAULT_PROXY_TIMEOUT_MS);
    Duration::from_millis(millis)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_transport_returns_matching_thread_response() {
        let root =
            env::temp_dir().join(format!("moonbox-codex-app-fixture-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("root");
        let fixture_path = root.join("app-server.json");
        fs::write(
            &fixture_path,
            format!(
                r#"{{
                  "responses": [
                    {{"method":"thread/list","result":{{"data":[{}]}}}},
                    {{"method":"thread/read","thread_id":"codex-app-1","result":{{"thread":{}}}}},
                    {{"method":"thread/turns/list","thread_id":"codex-app-1","result":{{"data":[{}]}}}}
                  ]
                }}"#,
                thread_json(),
                thread_json(),
                turn_json()
            ),
        )
        .expect("fixture");
        let source = CodexAppServerSource::fixture(fixture_path);

        let sessions = source
            .list_threads(Some(10))
            .expect("threads")
            .into_iter()
            .map(app_thread_summary)
            .collect::<Vec<_>>();
        let timeline = source
            .load_timeline_limited("codex-app-1", None)
            .expect("timeline");

        assert_eq!(sessions[0].id, "codex-app-1");
        assert_eq!(sessions[0].title, "App Server Thread");
        assert_eq!(sessions[0].runtime_status, SessionRuntimeStatus::Active);
        assert_eq!(
            sessions[0].source_path.as_deref(),
            Some("codex-app-server://threads/codex-app-1")
        );
        assert_eq!(
            timeline
                .events
                .iter()
                .map(|event| (&event.kind, event.detail.as_str()))
                .collect::<Vec<_>>(),
            vec![
                (&TimelineKind::User, "Use app-server history"),
                (&TimelineKind::Assistant, "App-server response"),
                (&TimelineKind::Tool, "cargo test [completed] exit=0\nok")
            ]
        );
    }

    fn thread_json() -> &'static str {
        r#"{
          "cliVersion":"0.0.0-test",
          "createdAt":1780732800,
          "cwd":"/repo",
          "ephemeral":false,
          "id":"codex-app-1",
          "modelProvider":"openai",
          "name":"App Server Thread",
          "preview":"Use app-server history",
          "sessionId":"codex-app-1",
          "source":"cli",
          "status":{"type":"active","activeFlags":[]},
          "turns":[],
          "updatedAt":1780736400,
          "gitInfo":{"branch":"main"}
        }"#
    }

    fn turn_json() -> &'static str {
        r#"{
          "id":"turn-1",
          "startedAt":1780732860,
          "status":"completed",
          "items":[
            {"id":"item-1","type":"userMessage","content":[{"type":"text","text":"Use app-server history"}]},
            {"id":"item-2","type":"agentMessage","text":"App-server response"},
            {"id":"item-3","type":"commandExecution","command":"cargo test","commandActions":[],"cwd":"/repo","status":"completed","exitCode":0,"aggregatedOutput":"ok"}
          ]
        }"#
    }
}
