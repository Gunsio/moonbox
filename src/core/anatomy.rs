use std::{
    collections::HashMap,
    fs::{self, File},
    io::{BufRead, BufReader, Read, Seek, SeekFrom},
    path::Path,
};

use serde_json::Value;

use super::{
    local_jsonl::{
        find_token_count, is_moonbox_handoff_control_text, is_skill_context_text, text_from_value,
    },
    model::{
        AnatomyMetric, AnatomySignal, CliTool, CompactFrontier, SessionAnatomy,
        SessionAnatomyStatus, SessionSidecarSummary, SessionSummary, TokenBreakdown,
    },
};

const DEFAULT_FULL_SCAN_LIMIT_BYTES: u64 = 64 * 1024 * 1024;
const DEFAULT_TAIL_SCAN_BYTES: u64 = 64 * 1024 * 1024;
const MAX_PROFILE_ROWS: usize = 8;
const MAX_SIDECAR_ROWS: usize = 6;

pub fn enrich_session_summary(mut session: SessionSummary) -> SessionSummary {
    session.anatomy = Some(analyze_session(&session));
    session
}

pub fn analyze_session(session: &SessionSummary) -> SessionAnatomy {
    let Some(source_path) = session
        .source_path
        .as_deref()
        .filter(|path| !path.is_empty())
    else {
        return missing_anatomy("No local source path is available for this session.");
    };
    let path = Path::new(source_path);
    let Ok(metadata) = fs::metadata(path) else {
        return failed_anatomy(format!("Source path is not readable: {source_path}"));
    };
    if !metadata.is_file() {
        return failed_anatomy(format!("Source path is not a file: {source_path}"));
    }

    let source_size = metadata.len();
    let full_limit = configured_limit(
        "MOONBOX_ANATOMY_FULL_SCAN_BYTES",
        DEFAULT_FULL_SCAN_LIMIT_BYTES,
    );
    let tail_limit = configured_limit("MOONBOX_ANATOMY_TAIL_BYTES", DEFAULT_TAIL_SCAN_BYTES);

    let scan_result = if source_size <= full_limit {
        scan_full_file(path, session.cli, source_size)
    } else {
        scan_tail_sample(path, session.cli, source_size, tail_limit)
    };

    match scan_result {
        Ok(mut anatomy) => {
            anatomy.source_size_bytes = Some(source_size);
            anatomy.sidecars = sidecars_for_source(path, session.cli);
            anatomy.value_signals = value_signals(session, &anatomy);
            anatomy
        }
        Err(reason) => failed_anatomy(reason),
    }
}

fn configured_limit(env_name: &str, default: u64) -> u64 {
    std::env::var(env_name)
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default)
}

fn missing_anatomy(reason: impl Into<String>) -> SessionAnatomy {
    SessionAnatomy {
        status: SessionAnatomyStatus::Missing,
        scan_scope: "unavailable".into(),
        notes: vec![reason.into()],
        ..SessionAnatomy::default()
    }
}

fn failed_anatomy(reason: impl Into<String>) -> SessionAnatomy {
    SessionAnatomy {
        status: SessionAnatomyStatus::Failed,
        scan_scope: "failed".into(),
        notes: vec![reason.into()],
        ..SessionAnatomy::default()
    }
}

fn scan_full_file(path: &Path, cli: CliTool, source_size: u64) -> Result<SessionAnatomy, String> {
    let file = File::open(path).map_err(|error| format!("cannot open source: {error}"))?;
    let reader = BufReader::new(file);
    let mut builder = AnatomyBuilder::new(cli, false, source_size, "full");

    for (index, line) in reader.lines().enumerate() {
        let line = line.map_err(|error| format!("cannot read source: {error}"))?;
        let bytes = line.len() as u64 + 1;
        builder.observe_line(&line, Some(index + 1), bytes);
    }

    Ok(builder.finish())
}

fn scan_tail_sample(
    path: &Path,
    cli: CliTool,
    source_size: u64,
    tail_limit: u64,
) -> Result<SessionAnatomy, String> {
    let mut file = File::open(path).map_err(|error| format!("cannot open source: {error}"))?;
    let start = source_size.saturating_sub(tail_limit);
    file.seek(SeekFrom::Start(start))
        .map_err(|error| format!("cannot seek source tail: {error}"))?;

    let mut sample = String::new();
    file.read_to_string(&mut sample)
        .map_err(|error| format!("cannot read source tail: {error}"))?;
    if start > 0 {
        if let Some(first_newline) = sample.find('\n') {
            sample = sample[first_newline + 1..].to_owned();
        } else {
            sample.clear();
        }
    }

    let analyzed_bytes = sample.len() as u64;
    let mut builder = AnatomyBuilder::new(cli, true, analyzed_bytes, "tail sample");
    builder.notes.push(format!(
        "Large source sampled from the newest {}.",
        format_bytes(analyzed_bytes)
    ));
    for line in sample.lines() {
        let bytes = line.len() as u64 + 1;
        builder.observe_line(line, None, bytes);
    }
    Ok(builder.finish())
}

struct AnatomyBuilder {
    cli: CliTool,
    sampled: bool,
    analyzed_bytes: u64,
    scan_scope: String,
    line_count: usize,
    malformed_lines: usize,
    size_profile: HashMap<String, MetricAccumulator>,
    event_profile: HashMap<String, MetricAccumulator>,
    content_profile: HashMap<String, MetricAccumulator>,
    compact: Option<CompactAccumulator>,
    token_profile: Option<TokenBreakdown>,
    notes: Vec<String>,
}

#[derive(Default)]
struct MetricAccumulator {
    count: usize,
    bytes: u64,
}

struct CompactAccumulator {
    label: String,
    line_number: Option<usize>,
    tail_lines: usize,
    tail_bytes: u64,
}

impl AnatomyBuilder {
    fn new(cli: CliTool, sampled: bool, analyzed_bytes: u64, scan_scope: &str) -> Self {
        Self {
            cli,
            sampled,
            analyzed_bytes,
            scan_scope: scan_scope.into(),
            line_count: 0,
            malformed_lines: 0,
            size_profile: HashMap::new(),
            event_profile: HashMap::new(),
            content_profile: HashMap::new(),
            compact: None,
            token_profile: None,
            notes: Vec::new(),
        }
    }

    fn observe_line(&mut self, line: &str, line_number: Option<usize>, bytes: u64) {
        if line.trim().is_empty() {
            return;
        }
        self.line_count += 1;
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            self.malformed_lines += 1;
            self.observe_metric("malformed", bytes, MetricKind::Event);
            return;
        };

        let record_type = string_field(&value, "type").unwrap_or("unknown");
        self.observe_metric(record_type, bytes, MetricKind::Size);

        let event_key = event_key(self.cli, &value, record_type);
        self.observe_metric(&event_key, bytes, MetricKind::Event);

        for content_key in content_keys(self.cli, &value, record_type) {
            self.observe_metric(&content_key, bytes, MetricKind::Content);
        }

        if let Some(token_profile) = token_profile_from_record(self.cli, &value) {
            self.token_profile = Some(token_profile);
        }

        if is_compact_frontier(self.cli, &value, record_type, &event_key) {
            self.compact = Some(CompactAccumulator {
                label: event_key,
                line_number,
                tail_lines: 0,
                tail_bytes: 0,
            });
        } else if let Some(compact) = &mut self.compact {
            compact.tail_lines += 1;
            compact.tail_bytes = compact.tail_bytes.saturating_add(bytes);
        }
    }

    fn observe_metric(&mut self, label: &str, bytes: u64, kind: MetricKind) {
        let target = match kind {
            MetricKind::Size => &mut self.size_profile,
            MetricKind::Event => &mut self.event_profile,
            MetricKind::Content => &mut self.content_profile,
        };
        let metric = target.entry(label.to_owned()).or_default();
        metric.count += 1;
        metric.bytes = metric.bytes.saturating_add(bytes);
    }

    fn finish(self) -> SessionAnatomy {
        let status = if self.malformed_lines > 0 {
            SessionAnatomyStatus::Partial
        } else {
            SessionAnatomyStatus::Ready
        };
        let mut notes = self.notes;
        if self.line_count == 0 {
            notes.push("No parseable JSONL rows were found in the analyzed window.".into());
        }
        if self.sampled {
            notes.push("Counts describe the analyzed sample, not the whole source file.".into());
        }

        SessionAnatomy {
            status,
            scan_scope: self.scan_scope,
            analyzed_bytes: self.analyzed_bytes,
            sampled: self.sampled,
            total_lines: (!self.sampled).then_some(self.line_count),
            malformed_lines: self.malformed_lines,
            size_profile: sorted_metrics(self.size_profile),
            event_profile: sorted_metrics(self.event_profile),
            content_profile: sorted_metrics(self.content_profile),
            compact: self.compact.map(|compact| CompactFrontier {
                label: compact.label,
                line_number: compact.line_number,
                tail_lines: compact.tail_lines,
                tail_bytes: compact.tail_bytes,
                detail: "newest content after this frontier is the active continuation window"
                    .into(),
            }),
            token_profile: self.token_profile,
            notes,
            ..SessionAnatomy::default()
        }
    }
}

enum MetricKind {
    Size,
    Event,
    Content,
}

fn sorted_metrics(metrics: HashMap<String, MetricAccumulator>) -> Vec<AnatomyMetric> {
    let mut rows = metrics
        .into_iter()
        .map(|(label, metric)| AnatomyMetric {
            label,
            count: metric.count,
            bytes: metric.bytes,
        })
        .collect::<Vec<_>>();
    rows.sort_by(|left, right| {
        right
            .bytes
            .cmp(&left.bytes)
            .then_with(|| right.count.cmp(&left.count))
            .then_with(|| left.label.cmp(&right.label))
    });
    rows.truncate(MAX_PROFILE_ROWS);
    rows
}

fn event_key(cli: CliTool, value: &Value, record_type: &str) -> String {
    match cli {
        CliTool::Codex => value
            .get("payload")
            .and_then(|payload| string_field(payload, "type"))
            .unwrap_or(record_type)
            .to_owned(),
        CliTool::Claude => match string_field(value, "subtype") {
            Some(subtype) if !subtype.is_empty() => format!("{record_type}/{subtype}"),
            _ => record_type.to_owned(),
        },
        CliTool::Hermes => record_type.to_owned(),
    }
}

fn content_keys(cli: CliTool, value: &Value, record_type: &str) -> Vec<String> {
    let mut keys = match cli {
        CliTool::Codex => codex_content_keys(value),
        CliTool::Claude => claude_content_keys(value, record_type),
        CliTool::Hermes => Vec::new(),
    };
    if let Some(text) = record_text_for_context_profile(cli, value) {
        if is_skill_context_text(&text) {
            push_unique(&mut keys, "control:skill");
        }
        if is_moonbox_handoff_control_text(&text) {
            push_unique(&mut keys, "control:handoff");
        }
    }
    keys
}

fn record_text_for_context_profile(cli: CliTool, value: &Value) -> Option<String> {
    match cli {
        CliTool::Codex => value
            .get("payload")
            .and_then(text_from_value)
            .or_else(|| text_from_value(value)),
        CliTool::Claude => value
            .get("message")
            .and_then(|message| message.get("content"))
            .and_then(text_from_value)
            .or_else(|| text_from_value(value)),
        CliTool::Hermes => text_from_value(value),
    }
}

fn push_unique(keys: &mut Vec<String>, key: &str) {
    if !keys.iter().any(|existing| existing == key) {
        keys.push(key.into());
    }
}

fn codex_content_keys(value: &Value) -> Vec<String> {
    let payload = value.get("payload").unwrap_or(&Value::Null);
    let payload_type = string_field(payload, "type").unwrap_or("unknown");
    let mut keys = Vec::new();
    if let Some(role) = string_field(payload, "role") {
        keys.push(format!("role:{role}"));
    }
    if let Some(items) = payload.get("content").and_then(Value::as_array) {
        for item in items {
            keys.push(format!(
                "content:{}",
                string_field(item, "type").unwrap_or("text")
            ));
        }
    }
    if keys.is_empty() {
        keys.push(payload_type.to_owned());
    }
    keys
}

fn claude_content_keys(value: &Value, record_type: &str) -> Vec<String> {
    let mut keys = Vec::new();
    match value
        .get("message")
        .and_then(|message| message.get("content"))
    {
        Some(Value::Array(items)) => {
            for item in items {
                keys.push(format!(
                    "content:{}",
                    string_field(item, "type").unwrap_or("text")
                ));
            }
        }
        Some(Value::String(_)) => keys.push("content:string".into()),
        _ => {}
    }
    if record_type == "attachment" {
        keys.push("attachment".into());
    }
    if value
        .get("toolUseResult")
        .is_some_and(|tool| !tool.is_null())
    {
        keys.push("tool_result_payload".into());
    }
    if keys.is_empty() {
        keys.push(record_type.to_owned());
    }
    keys
}

fn is_compact_frontier(cli: CliTool, value: &Value, record_type: &str, event_key: &str) -> bool {
    match cli {
        CliTool::Codex => {
            record_type == "compacted"
                || event_key.contains("compact")
                || value
                    .get("payload")
                    .and_then(|payload| string_field(payload, "type"))
                    .is_some_and(|payload_type| payload_type.contains("compact"))
        }
        CliTool::Claude => {
            record_type == "summary"
                || string_field(value, "subtype").is_some_and(|subtype| {
                    subtype == "compact_boundary" || subtype.contains("summary")
                })
        }
        CliTool::Hermes => false,
    }
}

fn token_profile_from_record(cli: CliTool, value: &Value) -> Option<TokenBreakdown> {
    match cli {
        CliTool::Codex => {
            let payload = value.get("payload").unwrap_or(value);
            if string_field(payload, "type") != Some("token_count") {
                return None;
            }
            find_token_count(payload).map(|total| TokenBreakdown {
                total,
                ..TokenBreakdown::default()
            })
        }
        CliTool::Claude => value
            .get("message")
            .and_then(|message| message.get("usage"))
            .and_then(token_breakdown_from_usage),
        CliTool::Hermes => None,
    }
}

fn token_breakdown_from_usage(usage: &Value) -> Option<TokenBreakdown> {
    let input = value_usize(usage, "input_tokens");
    let output = value_usize(usage, "output_tokens");
    let cache_read = value_usize(usage, "cache_read_input_tokens")
        .or_else(|| value_usize(usage, "cache_creation_input_tokens"))
        .unwrap_or(0);
    let cache_write = value_usize(usage, "cache_write_input_tokens").unwrap_or(0);
    let reasoning = value_usize(usage, "reasoning_output_tokens").unwrap_or(0);
    let input = input.unwrap_or(0);
    let output = output.unwrap_or(0);
    let total = input
        .saturating_add(output)
        .saturating_add(cache_read)
        .saturating_add(cache_write)
        .saturating_add(reasoning);
    (total > 0).then_some(TokenBreakdown {
        input,
        output,
        cache_read,
        cache_write,
        reasoning,
        total,
    })
}

fn value_usize(value: &Value, key: &str) -> Option<usize> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|count| usize::try_from(count).ok())
}

fn string_field<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

fn sidecars_for_source(path: &Path, cli: CliTool) -> Vec<SessionSidecarSummary> {
    if cli != CliTool::Claude {
        return Vec::new();
    }
    let sidecar_root = path.with_extension("");
    if !sidecar_root.is_dir() {
        return Vec::new();
    }
    let mut groups: HashMap<String, SessionSidecarSummary> = HashMap::new();
    collect_sidecars(&sidecar_root, &sidecar_root, &mut groups);
    let mut rows = groups.into_values().collect::<Vec<_>>();
    rows.sort_by(|left, right| {
        right
            .bytes
            .cmp(&left.bytes)
            .then_with(|| right.file_count.cmp(&left.file_count))
            .then_with(|| left.kind.cmp(&right.kind))
    });
    rows.truncate(MAX_SIDECAR_ROWS);
    rows
}

fn collect_sidecars(
    root: &Path,
    current: &Path,
    groups: &mut HashMap<String, SessionSidecarSummary>,
) {
    let Ok(entries) = fs::read_dir(current) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if metadata.is_dir() {
            collect_sidecars(root, &path, groups);
            continue;
        }
        if !metadata.is_file() {
            continue;
        }
        let kind = sidecar_kind(root, &path);
        let row = groups
            .entry(kind.clone())
            .or_insert_with(|| SessionSidecarSummary {
                kind,
                path: root.display().to_string(),
                ..SessionSidecarSummary::default()
            });
        row.file_count += 1;
        row.bytes = row.bytes.saturating_add(metadata.len());
    }
}

fn sidecar_kind(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .ok()
        .and_then(|relative| relative.components().next())
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "sidecar".into())
}

fn value_signals(session: &SessionSummary, anatomy: &SessionAnatomy) -> Vec<AnatomySignal> {
    let mut signals = Vec::new();
    if let Some(compact) = &anatomy.compact {
        signals.push(signal(
            1,
            "Continuation",
            "Active tail",
            format!(
                "{} / {}",
                format_bytes(compact.tail_bytes),
                plural(compact.tail_lines, "row")
            ),
            compact.detail.clone(),
        ));
    } else {
        signals.push(signal(
            1,
            "Continuation",
            "Compact frontier",
            "not found",
            "No compact boundary was visible in the analyzed source window.",
        ));
    }

    if let Some(tokens) = &anatomy.token_profile {
        signals.push(signal(
            1,
            "Continuation",
            "Token profile",
            format_token_count(tokens.total),
            token_detail(tokens),
        ));
    } else if let Some(tokens) = session.token_count {
        signals.push(signal(
            1,
            "Continuation",
            "Token profile",
            format_token_count(tokens),
            "Indexed summary token count; no detailed usage row in anatomy window.",
        ));
    }

    signals.push(signal(
        2,
        "Trust",
        "Source health",
        format!("{:?}", anatomy.status).to_lowercase(),
        if anatomy.malformed_lines == 0 {
            format!(
                "{} provenance; no malformed rows in scope.",
                session.source_provenance
            )
        } else {
            format!(
                "{} provenance; {} malformed row(s) in scope.",
                session.source_provenance, anatomy.malformed_lines
            )
        },
    ));

    if let Some(metric) = anatomy.size_profile.first() {
        signals.push(signal(
            3,
            "Debug",
            "Largest source slice",
            metric.label.clone(),
            format!(
                "{} across {}",
                format_bytes(metric.bytes),
                plural(metric.count, "row")
            ),
        ));
    } else {
        signals.push(signal(
            3,
            "Debug",
            "Source profile",
            "empty",
            "No profile rows were produced for this source.",
        ));
    }

    signals.push(signal(
        4,
        "Trace",
        "Source file",
        session
            .source_size_bytes
            .or(anatomy.source_size_bytes)
            .map(format_bytes)
            .unwrap_or_else(|| "size unknown".into()),
        session
            .source_path
            .clone()
            .unwrap_or_else(|| "path unavailable".into()),
    ));
    if !anatomy.sidecars.is_empty() {
        let bytes = anatomy
            .sidecars
            .iter()
            .fold(0_u64, |sum, row| sum.saturating_add(row.bytes));
        let files = anatomy
            .sidecars
            .iter()
            .fold(0_usize, |sum, row| sum.saturating_add(row.file_count));
        signals.push(signal(
            4,
            "Trace",
            "Sidecars",
            format!("{} / {}", format_bytes(bytes), plural(files, "file")),
            "Provider sidecar inventory is available for trace/debug, not default handoff input.",
        ));
    }

    signals
}

fn signal(
    rank: u8,
    group: impl Into<String>,
    label: impl Into<String>,
    value: impl Into<String>,
    detail: impl Into<String>,
) -> AnatomySignal {
    AnatomySignal {
        rank,
        group: group.into(),
        label: label.into(),
        value: value.into(),
        detail: detail.into(),
    }
}

fn token_detail(tokens: &TokenBreakdown) -> String {
    let mut parts = Vec::new();
    if tokens.input > 0 {
        parts.push(format!("input {}", format_token_count(tokens.input)));
    }
    if tokens.output > 0 {
        parts.push(format!("output {}", format_token_count(tokens.output)));
    }
    if tokens.cache_read > 0 {
        parts.push(format!(
            "cache read {}",
            format_token_count(tokens.cache_read)
        ));
    }
    if tokens.cache_write > 0 {
        parts.push(format!(
            "cache write {}",
            format_token_count(tokens.cache_write)
        ));
    }
    if tokens.reasoning > 0 {
        parts.push(format!(
            "reasoning {}",
            format_token_count(tokens.reasoning)
        ));
    }
    if parts.is_empty() {
        "total token usage only".into()
    } else {
        parts.join(" · ")
    }
}

fn format_token_count(tokens: usize) -> String {
    match tokens {
        0..=999 => tokens.to_string(),
        1_000..=999_999 => format!("{}K", tokens / 1_000),
        _ => format!("{}M", tokens / 1_000_000),
    }
}

fn plural(count: usize, noun: &str) -> String {
    if count == 1 {
        format!("1 {noun}")
    } else {
        format!("{count} {noun}s")
    }
}

fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let bytes_f = bytes as f64;
    if bytes_f >= GB {
        format!("{:.1}GB", bytes_f / GB)
    } else if bytes_f >= MB {
        format!("{:.1}MB", bytes_f / MB)
    } else if bytes_f >= KB {
        format!("{:.1}KB", bytes_f / KB)
    } else {
        format!("{bytes}B")
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::*;
    use crate::core::model::{SessionRuntimeStatus, SessionStatus, SourceProvenance};

    #[test]
    fn codex_anatomy_reports_compact_tail_and_token_profile() {
        let root = temp_root("codex");
        fs::create_dir_all(&root).expect("temp root");
        let source_path = root.join("rollout.jsonl");
        fs::write(
            &source_path,
            [
                r#"{"type":"session_meta","payload":{"id":"codex-1","cwd":"/repo"}}"#,
                r#"{"type":"response_item","payload":{"role":"user","content":[{"type":"input_text","text":"start"}]}}"#,
                r#"{"type":"compacted","payload":{"type":"context_compacted"}}"#,
                r#"{"type":"event_msg","payload":{"type":"token_count","info":{"total_tokens":42000}}}"#,
                r#"{"type":"response_item","payload":{"role":"assistant","content":[{"type":"output_text","text":"done"}]}}"#,
            ]
            .join("\n"),
        )
        .expect("write codex fixture");

        let anatomy = analyze_session(&test_session(CliTool::Codex, &source_path));

        assert_eq!(anatomy.status, SessionAnatomyStatus::Ready);
        assert_eq!(anatomy.total_lines, Some(5));
        assert_eq!(
            anatomy.compact.as_ref().map(|compact| compact.tail_lines),
            Some(2)
        );
        assert_eq!(
            anatomy.token_profile.as_ref().map(|tokens| tokens.total),
            Some(42_000)
        );
        assert!(
            anatomy
                .size_profile
                .iter()
                .any(|metric| metric.label == "compacted")
        );
        assert!(
            anatomy
                .value_signals
                .iter()
                .any(|signal| signal.group == "Continuation")
        );

        cleanup(root);
    }

    #[test]
    fn claude_anatomy_reports_content_mix_and_sidecars() {
        let root = temp_root("claude");
        fs::create_dir_all(&root).expect("temp root");
        let source_path = root.join("session.jsonl");
        fs::write(
            &source_path,
            [
                r#"{"type":"system","subtype":"compact_boundary","sessionId":"claude-1","cwd":"/repo"}"#,
                r#"{"type":"assistant","message":{"usage":{"input_tokens":120,"output_tokens":30},"content":[{"type":"text","text":"分析完成"}]}}"#,
                r#"{"type":"user","message":{"content":[{"type":"tool_result","content":"ok"}]},"toolUseResult":{"stdout":"ok"}}"#,
                r#"{"type":"attachment","message":{"content":[{"type":"image","source":{"type":"base64","data":"AA=="}}]}}"#,
            ]
            .join("\n"),
        )
        .expect("write claude fixture");
        let sidecar_dir = root.join("session").join("subagents");
        fs::create_dir_all(&sidecar_dir).expect("sidecar dir");
        fs::write(sidecar_dir.join("agent.jsonl"), "{}\n").expect("sidecar file");

        let anatomy = analyze_session(&test_session(CliTool::Claude, &source_path));

        assert_eq!(anatomy.status, SessionAnatomyStatus::Ready);
        assert_eq!(
            anatomy.compact.as_ref().map(|compact| compact.tail_lines),
            Some(3)
        );
        assert!(
            anatomy
                .content_profile
                .iter()
                .any(|metric| metric.label == "content:image")
        );
        assert!(
            anatomy
                .sidecars
                .iter()
                .any(|sidecar| sidecar.kind == "subagents" && sidecar.file_count == 1)
        );
        assert_eq!(
            anatomy.token_profile.as_ref().map(|tokens| tokens.total),
            Some(150)
        );

        cleanup(root);
    }

    #[test]
    fn codex_anatomy_counts_skill_control_blocks_without_timeline_exposure() {
        let root = temp_root("codex-skill-control");
        fs::create_dir_all(&root).expect("temp root");
        let source_path = root.join("session.jsonl");
        fs::write(
            &source_path,
            [
                r#"{"type":"session_meta","payload":{"id":"codex-1","cwd":"/repo"}}"#,
                r#"{"type":"event_msg","payload":{"type":"user_message","message":"[ <skill> <name>qc-login</name> <path>/Users/me/.codex/skills/qc-login/SKILL.md</path> --- name: qc-login description: prepare browser state"}}"#,
                r#"{"type":"event_msg","payload":{"type":"user_message","message":"real user request"}}"#,
            ]
            .join("\n"),
        )
        .expect("write codex fixture");

        let anatomy = analyze_session(&test_session(CliTool::Codex, &source_path));

        assert!(
            anatomy
                .content_profile
                .iter()
                .any(|metric| metric.label == "control:skill" && metric.count == 1),
            "{:?}",
            anatomy.content_profile
        );

        cleanup(root);
    }

    fn test_session(cli: CliTool, source_path: &Path) -> SessionSummary {
        SessionSummary {
            id: "session-id".into(),
            cli,
            title: "Session".into(),
            cwd: "/repo".into(),
            updated_at: "2026-06-11T00:00:00Z".into(),
            updated: "2026-06-11 00:00".into(),
            runtime_status: SessionRuntimeStatus::Unknown,
            runtime_reason: None,
            status: SessionStatus::Healthy,
            branch: None,
            token_count: None,
            health_reason: None,
            event_count: 0,
            resume_command: format!("{} resume session-id", cli.id()),
            source_provenance: SourceProvenance::Fixture,
            source_path: Some(source_path.display().to_string()),
            source_size_bytes: None,
            parse_skip_count: 0,
            provider_metadata: None,
            context_health: None,
            anatomy: None,
        }
    }

    fn temp_root(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "moonbox-anatomy-{label}-{}-{nonce}",
            std::process::id()
        ))
    }

    fn cleanup(path: PathBuf) {
        let _ = fs::remove_dir_all(path);
    }
}
