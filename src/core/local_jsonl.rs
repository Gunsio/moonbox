use std::{
    env, fs,
    io::BufReader,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::{Value, json};

use super::{
    adapter::{AdapterError, SourceScanStats},
    model::{CliTool, TimelineAttachment, TimelineEvent, TimelineKind},
};

pub const DEFAULT_SESSION_LIMIT: usize = 200;
pub const DEFAULT_SESSION_SCAN_ENTRY_LIMIT: usize = 5000;
pub const DEFAULT_SESSION_SUMMARY_LINE_LIMIT: usize = 800;
pub const DEFAULT_TIMELINE_DETAIL_CHAR_LIMIT: usize = 4000;

pub fn configured_session_limit() -> Option<usize> {
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

pub fn configured_session_scan_entry_limit() -> Option<usize> {
    match env::var("MOONBOX_SESSION_SCAN_LIMIT") {
        Ok(value) if value.trim() == "0" => None,
        Ok(value) => value
            .trim()
            .parse::<usize>()
            .ok()
            .filter(|limit| *limit > 0)
            .or(Some(DEFAULT_SESSION_SCAN_ENTRY_LIMIT)),
        Err(_) => Some(DEFAULT_SESSION_SCAN_ENTRY_LIMIT),
    }
}

pub fn configured_session_summary_line_limit() -> Option<usize> {
    match env::var("MOONBOX_SESSION_SUMMARY_LINE_LIMIT") {
        Ok(value) if value.trim() == "0" => None,
        Ok(value) => value
            .trim()
            .parse::<usize>()
            .ok()
            .filter(|limit| *limit > 0)
            .or(Some(DEFAULT_SESSION_SUMMARY_LINE_LIMIT)),
        Err(_) => Some(DEFAULT_SESSION_SUMMARY_LINE_LIMIT),
    }
}

pub fn configured_timeline_detail_char_limit() -> Option<usize> {
    match env::var("MOONBOX_TIMELINE_DETAIL_CHAR_LIMIT") {
        Ok(value) if value.trim() == "0" => None,
        Ok(value) => value
            .trim()
            .parse::<usize>()
            .ok()
            .filter(|limit| *limit > 0)
            .or(Some(DEFAULT_TIMELINE_DETAIL_CHAR_LIMIT)),
        Err(_) => Some(DEFAULT_TIMELINE_DETAIL_CHAR_LIMIT),
    }
}

pub fn truncate_timeline_detail(text: &str) -> String {
    configured_timeline_detail_char_limit()
        .map(|limit| truncate(text, limit))
        .unwrap_or_else(|| text.to_owned())
}

pub fn open_reader(tool: CliTool, path: &Path) -> Result<BufReader<fs::File>, AdapterError> {
    let file = fs::File::open(path).map_err(|error| read_error(tool, path, error))?;
    Ok(BufReader::new(file))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonlDiscovery {
    pub files: Vec<PathBuf>,
    pub scan_stats: SourceScanStats,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscoveryOrder {
    PathDesc,
    ModifiedDesc,
}

pub fn collect_jsonl_files(
    tool: CliTool,
    root: &Path,
    files: &mut Vec<PathBuf>,
) -> Result<(), AdapterError> {
    let entries = fs::read_dir(root).map_err(|error| read_error(tool, root, error))?;
    for entry in entries {
        let entry = entry.map_err(|error| read_error(tool, root, error))?;
        let path = entry.path();
        if path.is_dir() {
            collect_jsonl_files(tool, &path, files)?;
        } else if is_jsonl_file(&path) {
            files.push(path);
        }
    }
    Ok(())
}

pub fn discover_jsonl_files(
    tool: CliTool,
    root: &Path,
    list_limit: Option<usize>,
    scan_entry_limit: Option<usize>,
    order: DiscoveryOrder,
) -> Result<JsonlDiscovery, AdapterError> {
    let mut discovery = JsonlDiscoveryBuilder::new(list_limit, scan_entry_limit, order);
    if root.exists() {
        discover_jsonl_files_inner(tool, root, &mut discovery)?;
    }
    Ok(discovery.finish())
}

pub fn collect_project_jsonl_files(
    tool: CliTool,
    root: &Path,
) -> Result<Vec<PathBuf>, AdapterError> {
    if !root.exists() {
        return Ok(Vec::new());
    }

    let mut files = Vec::new();
    let entries = fs::read_dir(root).map_err(|error| read_error(tool, root, error))?;
    for entry in entries {
        let entry = entry.map_err(|error| read_error(tool, root, error))?;
        let path = entry.path();
        if path.is_file() {
            if is_jsonl_file(&path) {
                files.push(path);
            }
            continue;
        }
        if !path.is_dir() {
            continue;
        }

        let project_entries =
            fs::read_dir(&path).map_err(|error| read_error(tool, &path, error))?;
        for project_entry in project_entries {
            let project_entry = project_entry.map_err(|error| read_error(tool, &path, error))?;
            let session_path = project_entry.path();
            if session_path.is_file() && is_jsonl_file(&session_path) {
                files.push(session_path);
            }
        }
    }
    Ok(files)
}

pub fn discover_project_jsonl_files(
    tool: CliTool,
    root: &Path,
    list_limit: Option<usize>,
    scan_entry_limit: Option<usize>,
) -> Result<JsonlDiscovery, AdapterError> {
    let mut discovery =
        JsonlDiscoveryBuilder::new(list_limit, scan_entry_limit, DiscoveryOrder::ModifiedDesc);
    if !root.exists() {
        return Ok(discovery.finish());
    }

    for path in sorted_child_paths(tool, root, DiscoveryOrder::ModifiedDesc)? {
        if discovery.scan_limit_reached() {
            break;
        }
        discovery.observe_entry();
        if path.is_file() {
            if is_jsonl_file(&path) {
                discovery.push_candidate(path);
            }
            continue;
        }
        if !path.is_dir() {
            continue;
        }

        for session_path in sorted_child_paths(tool, &path, DiscoveryOrder::ModifiedDesc)? {
            if discovery.scan_limit_reached() {
                break;
            }
            discovery.observe_entry();
            if session_path.is_file() && is_jsonl_file(&session_path) {
                discovery.push_candidate(session_path);
            }
        }
    }

    Ok(discovery.finish())
}

pub fn sort_paths_by_modified_desc(files: &mut [PathBuf]) {
    files.sort_by(|left, right| {
        modified_time(right)
            .cmp(&modified_time(left))
            .then_with(|| right.cmp(left))
    });
}

pub fn read_error(tool: CliTool, path: &Path, error: impl ToString) -> AdapterError {
    AdapterError::ReadSource {
        tool,
        path: path.to_string_lossy().into_owned(),
        reason: error.to_string(),
    }
}

pub fn string_field<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

pub fn text_from_value(value: &Value) -> Option<String> {
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
                "content",
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

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TimelineImageMarkup {
    pub text: String,
    pub attachments: Vec<TimelineAttachment>,
}

pub fn extract_timeline_image_markup(text: &str) -> TimelineImageMarkup {
    let mut rest = text;
    let mut output = String::new();
    let mut attachments = Vec::new();

    while let Some(start) = rest.find("<image") {
        output.push_str(&rest[..start]);
        let marker = &rest[start..];
        let Some(tag_end) = marker.find('>') else {
            output.push_str(marker);
            rest = "";
            break;
        };
        let start_tag = &marker[..=tag_end];
        let after_tag = &marker[tag_end + 1..];
        let Some(close_start) = after_tag.find("</image>") else {
            output.push_str(marker);
            rest = "";
            break;
        };

        let inner = &after_tag[..close_start];
        let after_close = &after_tag[close_start + "</image>".len()..];
        let name = image_name_from_tag(start_tag)
            .or_else(|| bracketed_image_label(inner))
            .or_else(|| bracketed_image_label(after_close))
            .unwrap_or_else(|| format!("Image #{}", attachments.len() + 1));
        attachments.push(TimelineAttachment {
            id: Some(name.clone()),
            name: Some(name.clone()),
            mime_type: Some("image/unknown".into()),
            raw: Some(json!({
                "source": "inline_image_markup",
                "marker": start_tag.trim(),
                "label": name,
            })),
            ..TimelineAttachment::default()
        });
        rest = consume_duplicate_image_label(after_close, &name);
    }
    output.push_str(rest);

    TimelineImageMarkup {
        text: normalize_text(&output).unwrap_or_default(),
        attachments,
    }
}

fn image_name_from_tag(tag: &str) -> Option<String> {
    let name_start = tag.find("name=")?;
    let value = tag[name_start + "name=".len()..].trim_start();
    if let Some(value) = value.strip_prefix('"') {
        return value
            .find('"')
            .and_then(|end| clean_image_label(&value[..end]));
    }
    if let Some(value) = value.strip_prefix('\'') {
        return value
            .find('\'')
            .and_then(|end| clean_image_label(&value[..end]));
    }
    if let Some(value) = value.strip_prefix('[') {
        return value
            .find(']')
            .and_then(|end| clean_image_label(&format!("[{}]", &value[..end])));
    }
    let end = value
        .find(|character: char| character.is_whitespace() || character == '>')
        .unwrap_or(value.len());
    clean_image_label(&value[..end])
}

fn bracketed_image_label(text: &str) -> Option<String> {
    let trimmed = text.trim();
    let start = trimmed.find("[Image #")?;
    let label = &trimmed[start..];
    let end = label.find(']')?;
    clean_image_label(&label[..=end])
}

fn clean_image_label(label: &str) -> Option<String> {
    let label = label.trim().trim_matches(&['[', ']'][..]);
    (!label.is_empty()).then_some(label.to_owned())
}

fn consume_duplicate_image_label<'a>(text: &'a str, name: &str) -> &'a str {
    let trimmed = text.trim_start();
    let label = format!("[{name}]");
    trimmed.strip_prefix(&label).unwrap_or(text)
}

pub fn find_token_count(value: &Value) -> Option<usize> {
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

pub fn stable_value_digest(value: &Value) -> String {
    let serialized = serde_json::to_string(value).unwrap_or_default();
    stable_text_digest(&serialized)
}

pub fn stable_text_digest(value: &str) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("fnv64:{hash:016x}")
}

pub fn max_timestamp(current: Option<String>, candidate: &str) -> String {
    match current {
        Some(current) if current.as_str() > candidate => current,
        _ => candidate.into(),
    }
}

pub fn human_timestamp(timestamp: &str) -> String {
    let normalized = replace_time_dashes(timestamp);
    normalized
        .get(..16)
        .map(|prefix| prefix.replace('T', " "))
        .unwrap_or(normalized)
}

pub fn display_time(timestamp: Option<&str>) -> String {
    let Some(timestamp) = timestamp else {
        return "??:??".into();
    };
    let normalized = replace_time_dashes(timestamp);
    normalized
        .split('T')
        .nth(1)
        .and_then(|time| time.get(..5))
        .unwrap_or("??:??")
        .into()
}

pub fn event_id(number: usize) -> String {
    format!("evt-{number:03}")
}

pub fn push_timeline_event(
    events: &mut Vec<TimelineEvent>,
    event: TimelineEvent,
    event_limit: Option<usize>,
) -> bool {
    if events
        .last()
        .is_some_and(|previous| is_adjacent_duplicate_event(previous, &event))
    {
        return false;
    }
    events.push(event);
    if let Some(limit) = event_limit
        && events.len() >= limit
    {
        events.push(timeline_preview_truncated_event(events.len() + 1, limit));
        return true;
    }
    false
}

fn is_adjacent_duplicate_event(previous: &TimelineEvent, event: &TimelineEvent) -> bool {
    previous.time == event.time
        && previous.kind == event.kind
        && previous.title == event.title
        && previous.detail == event.detail
}

pub fn timeline_preview_truncated_event(number: usize, limit: usize) -> TimelineEvent {
    TimelineEvent {
        id: event_id(number),
        time: "--:--".into(),
        kind: TimelineKind::Tool,
        title: "Timeline preview truncated".into(),
        detail: format!(
            "showing first {limit} events; set MOONBOX_TIMELINE_EVENT_LIMIT=0 for full TUI preview"
        ),
        metadata: Default::default(),
    }
}

pub fn truncate(text: &str, max_chars: usize) -> String {
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

pub fn is_provider_context_text(text: &str) -> bool {
    let trimmed = text.trim_start();
    trimmed.starts_with("<environment_context>")
        || trimmed.starts_with("<system_context>")
        || trimmed.starts_with("<developer_context>")
        || trimmed.starts_with("# AGENTS.md instructions for")
        || (trimmed.starts_with("<skill>") && trimmed.contains("</skill>"))
        || (trimmed.starts_with("<turn_aborted>") && trimmed.contains("</turn_aborted>"))
        || (trimmed.contains("<environment_context>")
            && trimmed.contains("</environment_context>")
            && (trimmed.contains("<INSTRUCTIONS>")
                || trimmed.contains("</INSTRUCTIONS>")
                || trimmed.contains("<permission_profile")
                || trimmed.contains("<cwd>")))
}

pub fn is_moonbox_handoff_control_text(text: &str) -> bool {
    let trimmed = text.trim_start();
    trimmed.starts_with("$handoff You are running a Moonbox continuation handoff job")
        || trimmed.starts_with("You are running a Moonbox continuation handoff job.")
        || trimmed.starts_with("The following is the Codex agent history whose request action")
        || trimmed.starts_with("<selected_skill")
        || (trimmed.contains("Moonbox continuation handoff job")
            && trimmed.contains("<selected_skill")
            && trimmed.contains("TRANSCRIPT START"))
}

pub fn title_case(value: &str) -> String {
    value
        .split(['_', '-'])
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

pub fn replace_time_dashes(timestamp: &str) -> String {
    let Some((date, rest)) = timestamp.split_once('T') else {
        return timestamp.into();
    };
    let mut chars = rest.chars().collect::<Vec<_>>();
    if chars.len() >= 8 {
        chars[2] = ':';
        chars[5] = ':';
    }
    format!("{date}T{}", chars.into_iter().collect::<String>())
}

fn discover_jsonl_files_inner(
    tool: CliTool,
    root: &Path,
    discovery: &mut JsonlDiscoveryBuilder,
) -> Result<(), AdapterError> {
    for path in sorted_child_paths(tool, root, discovery.order)? {
        if discovery.scan_limit_reached() {
            break;
        }
        discovery.observe_entry();
        if path.is_dir() {
            discover_jsonl_files_inner(tool, &path, discovery)?;
        } else if is_jsonl_file(&path) {
            discovery.push_candidate(path);
        }
    }
    Ok(())
}

fn sorted_child_paths(
    tool: CliTool,
    root: &Path,
    order: DiscoveryOrder,
) -> Result<Vec<PathBuf>, AdapterError> {
    let entries = fs::read_dir(root).map_err(|error| read_error(tool, root, error))?;
    let mut paths = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| read_error(tool, root, error))?;
        paths.push(entry.path());
    }
    sort_paths(&mut paths, order);
    Ok(paths)
}

#[derive(Debug, Clone)]
struct JsonlDiscoveryBuilder {
    files: Vec<PathBuf>,
    list_limit: Option<usize>,
    scan_entry_limit: Option<usize>,
    scan_entry_count: usize,
    scan_truncated: bool,
    order: DiscoveryOrder,
}

impl JsonlDiscoveryBuilder {
    fn new(
        list_limit: Option<usize>,
        scan_entry_limit: Option<usize>,
        order: DiscoveryOrder,
    ) -> Self {
        Self {
            files: Vec::new(),
            list_limit,
            scan_entry_limit,
            scan_entry_count: 0,
            scan_truncated: false,
            order,
        }
    }

    fn observe_entry(&mut self) {
        self.scan_entry_count += 1;
    }

    fn scan_limit_reached(&mut self) -> bool {
        if self
            .scan_entry_limit
            .is_some_and(|limit| self.scan_entry_count >= limit)
        {
            self.scan_truncated = true;
            true
        } else {
            false
        }
    }

    fn push_candidate(&mut self, path: PathBuf) {
        self.files.push(path);
        sort_paths(&mut self.files, self.order);
        if let Some(limit) = self.list_limit
            && self.files.len() > limit
        {
            self.files.truncate(limit);
        }
    }

    fn finish(mut self) -> JsonlDiscovery {
        sort_paths(&mut self.files, self.order);
        JsonlDiscovery {
            scan_stats: SourceScanStats {
                list_limit: self.list_limit,
                scan_entry_limit: self.scan_entry_limit,
                scan_entry_count: self.scan_entry_count,
                scan_truncated: self.scan_truncated,
                ..SourceScanStats::default()
            },
            files: self.files,
        }
    }
}

fn sort_paths(files: &mut [PathBuf], order: DiscoveryOrder) {
    match order {
        DiscoveryOrder::PathDesc => files.sort_by(|left, right| right.cmp(left)),
        DiscoveryOrder::ModifiedDesc => sort_paths_by_modified_desc(files),
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

fn is_jsonl_file(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension == "jsonl")
}

fn modified_time(path: &Path) -> SystemTime {
    path.metadata()
        .and_then(|metadata| metadata.modified())
        .unwrap_or(UNIX_EPOCH)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn timeline_event(number: usize, detail: &str) -> TimelineEvent {
        TimelineEvent {
            id: event_id(number),
            time: "10:00".into(),
            kind: TimelineKind::User,
            title: "User".into(),
            detail: detail.into(),
            metadata: Default::default(),
        }
    }

    #[test]
    fn extracts_inline_image_markup_from_timeline_text() {
        let markup = extract_timeline_image_markup(
            "<image name=[Image #1]> </image> [Image #1]\n看下这个问题",
        );

        assert_eq!(markup.text, "看下这个问题");
        assert_eq!(markup.attachments.len(), 1);
        assert_eq!(markup.attachments[0].name.as_deref(), Some("Image #1"));
        assert_eq!(
            markup.attachments[0].mime_type.as_deref(),
            Some("image/unknown")
        );
    }

    #[test]
    fn push_timeline_event_skips_adjacent_duplicate_events() {
        let mut events = Vec::new();

        assert!(!push_timeline_event(
            &mut events,
            timeline_event(1, "same"),
            None
        ));
        assert!(!push_timeline_event(
            &mut events,
            timeline_event(2, "same"),
            None
        ));
        assert!(!push_timeline_event(
            &mut events,
            timeline_event(3, "different"),
            None,
        ));

        assert_eq!(events.len(), 2);
        assert_eq!(events[0].detail, "same");
        assert_eq!(events[1].detail, "different");
    }

    #[test]
    fn skipped_duplicate_events_do_not_consume_preview_limit() {
        let mut events = Vec::new();

        assert!(!push_timeline_event(
            &mut events,
            timeline_event(1, "first"),
            Some(2),
        ));
        assert!(!push_timeline_event(
            &mut events,
            timeline_event(2, "first"),
            Some(2),
        ));
        assert!(push_timeline_event(
            &mut events,
            timeline_event(3, "second"),
            Some(2),
        ));

        assert_eq!(events.len(), 3);
        assert_eq!(events[0].detail, "first");
        assert_eq!(events[1].detail, "second");
        assert_eq!(events[2].title, "Timeline preview truncated");
    }
}
