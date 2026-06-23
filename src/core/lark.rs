use std::{
    env, fs, io,
    path::{Path, PathBuf},
    process::{Command, Output},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{
    error::CoreError,
    model::{CapsuleCompileRequest, WorkCapsule},
};

const LARK_CLI_ENV: &str = "MOONBOX_LARK_CLI_BIN";
const LARK_OPEN_DISABLE_ENV: &str = "MOONBOX_LARK_DISABLE_OPEN";
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LarkCliState {
    Ready,
    Missing,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LarkAuthState {
    Unknown,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LarkCliReadiness {
    pub state: LarkCliState,
    pub auth_state: LarkAuthState,
    pub command: String,
    pub version: Option<String>,
    pub supports_docs_create_v2: bool,
    pub reason: String,
    pub setup_command: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LarkExportPlan {
    pub version: u16,
    pub destination: String,
    pub mode: String,
    pub dry_run: bool,
    pub execute_ready: bool,
    pub title: String,
    pub session: String,
    pub source_cli: String,
    pub target_cli: String,
    pub compiler: String,
    pub rewind: String,
    pub sections: Vec<String>,
    pub risks: Vec<String>,
    pub lark_cli: LarkCliReadiness,
    pub command: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LarkExportExecution {
    pub plan: LarkExportPlan,
    pub url: Option<String>,
    pub stdout: String,
    pub browser_opened: bool,
    pub browser_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LarkTitlePatchRequest {
    pub params: String,
    pub data: String,
}

#[derive(Debug, Clone, Default)]
pub struct LarkExportOptions {
    pub parent_token: Option<String>,
    pub parent_position: Option<i32>,
}

pub fn readiness(setup_command: Option<String>) -> LarkCliReadiness {
    let command = lark_cli_bin_display();
    let version = run_lark_cli(["--version"])
        .ok()
        .and_then(version_from_output);
    if version.is_none() {
        return LarkCliReadiness {
            state: LarkCliState::Missing,
            auth_state: LarkAuthState::Unknown,
            command,
            version: None,
            supports_docs_create_v2: false,
            reason: "lark-cli was not found on PATH".into(),
            setup_command,
        };
    }

    let supports_docs_create_v2 =
        run_lark_cli(["docs", "+create", "--api-version", "v2", "--help"])
            .ok()
            .is_some_and(|output| {
                output.status.success()
                    && output_contains(&output, "--content")
                    && output_contains(&output, "--doc-format")
            });
    if !supports_docs_create_v2 {
        return LarkCliReadiness {
            state: LarkCliState::Unsupported,
            auth_state: LarkAuthState::Unknown,
            command,
            version,
            supports_docs_create_v2,
            reason:
                "lark-cli does not expose docs +create --api-version v2 with Markdown file input"
                    .into(),
            setup_command,
        };
    }

    LarkCliReadiness {
        state: LarkCliState::Ready,
        auth_state: LarkAuthState::Unknown,
        command,
        version,
        supports_docs_create_v2,
        reason: "lark-cli docs +create v2 is available; user auth is verified when execute runs"
            .into(),
        setup_command,
    }
}

pub fn dry_run_plan(
    request: &CapsuleCompileRequest,
    options: &LarkExportOptions,
    setup_command: Option<String>,
) -> LarkExportPlan {
    let title = document_title(&request.source_session.title, &request.source_session.id);
    let lark_cli = readiness(setup_command);
    let execute_ready = lark_cli.state == LarkCliState::Ready;
    LarkExportPlan {
        version: 1,
        destination: "lark".into(),
        mode: "handoff".into(),
        dry_run: true,
        execute_ready,
        title: title.clone(),
        session: request.source_session.id.clone(),
        source_cli: request.source_cli.id().into(),
        target_cli: request.target_cli.id().into(),
        compiler: request.compiler.clone(),
        rewind: request.rewind_event_id.clone(),
        sections: vec![
            "Generated handoff Markdown".into(),
            "Feishu/Lark document creation".into(),
        ],
        risks: vec![
            "Dry-run does not call the configured handoff runner or create a remote document."
                .into(),
            "Execute requires lark-cli user authentication and explicit --execute.".into(),
        ],
        command: create_command_preview(&title, options),
        lark_cli,
    }
}

pub fn execute_export(
    capsule: &WorkCapsule,
    options: &LarkExportOptions,
    setup_command: Option<String>,
) -> Result<LarkExportExecution, CoreError> {
    let title = document_title(&capsule.handoff_label, &capsule.source_session);
    let lark_cli = readiness(setup_command);
    if lark_cli.state != LarkCliState::Ready {
        return Err(CoreError::LarkExport {
            reason: lark_cli.reason,
        });
    }

    let content = handoff_markdown_content(capsule)?;
    let command = create_command_preview(&title, options);
    let output = run_lark_create(&markdown_with_document_title(&title, &content), options)?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    if !output.status.success() {
        return Err(CoreError::LarkExport {
            reason: if stderr.trim().is_empty() {
                format!(
                    "lark-cli docs +create exited with {}",
                    status_label(&output)
                )
            } else {
                format!(
                    "lark-cli docs +create exited with {}: {}",
                    status_label(&output),
                    stderr.trim()
                )
            },
        });
    }

    let url = extract_first_url(&stdout).or_else(|| extract_first_url(&stderr));
    if let Some(url) = url.as_deref()
        && let Some(output) = run_lark_title_patch(url, &title)?
    {
        let patch_stderr = String::from_utf8_lossy(&output.stderr);
        if !output.status.success() {
            return Err(CoreError::LarkExport {
                reason: if patch_stderr.trim().is_empty() {
                    format!(
                        "lark-cli drive files patch exited with {}",
                        status_label(&output)
                    )
                } else {
                    format!(
                        "lark-cli drive files patch exited with {}: {}",
                        status_label(&output),
                        patch_stderr.trim()
                    )
                },
            });
        }
    }
    let (browser_opened, browser_error) = match url.as_deref() {
        Some(url) => open_lark_url(url),
        None => (
            false,
            Some("lark-cli output did not include a document URL".into()),
        ),
    };
    let plan = LarkExportPlan {
        version: 1,
        destination: "lark".into(),
        mode: "handoff".into(),
        dry_run: false,
        execute_ready: true,
        title,
        session: capsule.source_session.clone(),
        source_cli: capsule.source_cli.id().into(),
        target_cli: capsule.target_cli.id().into(),
        compiler: capsule.compiler.clone(),
        rewind: capsule.rewind_point.clone(),
        sections: vec![
            "Generated handoff Markdown".into(),
            "Feishu/Lark document creation".into(),
        ],
        risks: capsule.risks.clone(),
        lark_cli,
        command,
    };
    Ok(LarkExportExecution {
        plan,
        url,
        stdout,
        browser_opened,
        browser_error,
    })
}

fn handoff_markdown_content(capsule: &WorkCapsule) -> Result<String, CoreError> {
    if let Some(path) = capsule
        .handoff_artifact_path
        .as_deref()
        .filter(|path| !path.trim().is_empty())
    {
        let content = fs::read_to_string(path).map_err(|error| CoreError::LarkExport {
            reason: format!("failed to read handoff artifact file {path}: {error}"),
        })?;
        if content.trim().is_empty() {
            return Err(CoreError::LarkExport {
                reason: format!("handoff artifact file is empty: {path}"),
            });
        }
        return Ok(content);
    }
    capsule
        .handoff_artifact
        .clone()
        .filter(|artifact| !artifact.trim().is_empty())
        .ok_or_else(|| CoreError::LarkExport {
            reason: "handoff runner did not produce a Markdown artifact for Lark export".into(),
        })
}

fn create_command_preview(_title: &str, options: &LarkExportOptions) -> Vec<String> {
    let mut command = vec![
        lark_cli_bin_display(),
        "docs".into(),
        "+create".into(),
        "--api-version".into(),
        "v2".into(),
        "--as".into(),
        "user".into(),
        "--doc-format".into(),
        "markdown".into(),
        "--content".into(),
        "@<titled handoff markdown file>".into(),
    ];
    if let Some(parent_token) = &options.parent_token {
        command.push("--parent-token".into());
        command.push(parent_token.clone());
    }
    if let Some(parent_position) = options.parent_position {
        command.push("--parent-position".into());
        command.push(parent_position.to_string());
    }
    command
}

fn run_lark_create(content: &str, options: &LarkExportOptions) -> Result<Output, CoreError> {
    let markdown_path = write_markdown_temp(content).map_err(|error| CoreError::LarkExport {
        reason: format!("failed to prepare Markdown file for lark-cli: {error}"),
    })?;
    let markdown_arg = markdown_file_arg(&markdown_path);
    let mut command = Command::new(lark_cli_bin());
    command
        .arg("docs")
        .arg("+create")
        .arg("--api-version")
        .arg("v2")
        .arg("--as")
        .arg("user")
        .arg("--doc-format")
        .arg("markdown")
        .arg("--content")
        .arg(&markdown_arg);
    if let Some(parent_token) = &options.parent_token {
        command.arg("--parent-token").arg(parent_token);
    }
    if let Some(parent_position) = options.parent_position {
        command
            .arg("--parent-position")
            .arg(parent_position.to_string());
    }
    let output = command.output();
    let _ = fs::remove_file(&markdown_path);
    output.map_err(|error| CoreError::LarkExport {
        reason: format!("failed to start lark-cli docs +create: {error}"),
    })
}

pub fn write_markdown_temp(content: &str) -> io::Result<PathBuf> {
    for attempt in 0..16 {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or_default();
        let path = PathBuf::from(format!(
            ".moonbox-lark-handoff-{}-{millis}-{attempt}.md",
            std::process::id()
        ));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .and_then(|mut file| {
                use io::Write;
                file.write_all(content.as_bytes())
            }) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not allocate unique Moonbox Lark Markdown temp file",
    ))
}

pub fn markdown_file_arg(path: &Path) -> String {
    format!("@{}", path.display())
}

fn run_lark_title_patch(url: &str, title: &str) -> Result<Option<Output>, CoreError> {
    let Some(request) = title_patch_request(url, title) else {
        return Ok(None);
    };
    Command::new(lark_cli_bin())
        .arg("drive")
        .arg("files")
        .arg("patch")
        .arg("--as")
        .arg("user")
        .arg("--params")
        .arg(request.params)
        .arg("--data")
        .arg(request.data)
        .output()
        .map(Some)
        .map_err(|error| CoreError::LarkExport {
            reason: format!("failed to start lark-cli drive files patch: {error}"),
        })
}

pub fn title_patch_request(url: &str, title: &str) -> Option<LarkTitlePatchRequest> {
    let (file_token, file_type) = lark_file_ref_from_url(url)?;
    Some(LarkTitlePatchRequest {
        params: json!({
            "file_token": file_token,
            "type": file_type,
        })
        .to_string(),
        data: json!({
            "new_title": title,
        })
        .to_string(),
    })
}

fn lark_file_ref_from_url(url: &str) -> Option<(String, String)> {
    let clean = url.split(['?', '#']).next().unwrap_or(url);
    let mut segments = clean.split('/').filter(|segment| !segment.is_empty());
    while let Some(segment) = segments.next() {
        let file_type = match segment {
            "docx" => Some("docx"),
            "docs" | "doc" => Some("doc"),
            "sheets" | "sheet" => Some("sheet"),
            "base" | "bitable" => Some("bitable"),
            "slides" => Some("slides"),
            _ => None,
        };
        if let Some(file_type) = file_type
            && let Some(token) = segments.next()
            && !token.is_empty()
        {
            return Some((token.into(), file_type.into()));
        }
    }
    None
}

fn run_lark_cli<const N: usize>(args: [&str; N]) -> std::io::Result<Output> {
    Command::new(lark_cli_bin()).args(args).output()
}

fn lark_cli_bin() -> PathBuf {
    env::var_os(LARK_CLI_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("lark-cli"))
}

fn lark_cli_bin_display() -> String {
    lark_cli_bin().display().to_string()
}

fn version_from_output(output: Output) -> Option<String> {
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    text.split_whitespace()
        .find(|part| part.chars().next().is_some_and(|ch| ch.is_ascii_digit()))
        .map(str::to_string)
}

fn output_contains(output: &Output, needle: &str) -> bool {
    String::from_utf8_lossy(&output.stdout).contains(needle)
        || String::from_utf8_lossy(&output.stderr).contains(needle)
}

pub fn document_title(title: &str, session_id: &str) -> String {
    let clean = title.trim();
    if clean.is_empty() {
        format!("Moonbox Handoff - {session_id}")
    } else {
        format!("Moonbox Handoff - {}", truncate_chars(clean, 90))
    }
}

pub fn markdown_with_document_title(title: &str, markdown: &str) -> String {
    let title = title.trim();
    let markdown = markdown.trim();
    if title.is_empty() || markdown.starts_with(&format!("# {title}\n")) {
        return markdown.into();
    }
    format!("# {title}\n\n{markdown}")
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    let mut output = String::new();
    for (index, ch) in text.chars().enumerate() {
        if index >= max_chars {
            output.push_str("\n\n[truncated]");
            return output;
        }
        output.push(ch);
    }
    output
}

fn extract_first_url(text: &str) -> Option<String> {
    if let Some(value) = first_json_value(text)
        && let Some(url) = url_from_json(&value)
    {
        return Some(url);
    }
    text.split_whitespace().find_map(|part| {
        let clean = part.trim_matches(|ch: char| {
            ch == '"' || ch == '\'' || ch == ',' || ch == ')' || ch == ']'
        });
        clean
            .starts_with("http")
            .then(|| clean.trim_end_matches('.').to_string())
    })
}

fn first_json_value(text: &str) -> Option<Value> {
    let start = text.find('{')?;
    serde_json::from_str(&text[start..]).ok()
}

fn url_from_json(value: &Value) -> Option<String> {
    match value {
        Value::String(text) if text.starts_with("http") => Some(text.clone()),
        Value::Array(items) => items.iter().find_map(url_from_json),
        Value::Object(map) => map.values().find_map(url_from_json),
        _ => None,
    }
}

fn open_lark_url(url: &str) -> (bool, Option<String>) {
    if env::var_os(LARK_OPEN_DISABLE_ENV).is_some() {
        return (false, None);
    }
    match Command::new("open").arg(url).status() {
        Ok(status) if status.success() => (true, None),
        Ok(status) => (
            false,
            Some(format!(
                "open exited with {}",
                status_label_from_code(status.code())
            )),
        ),
        Err(error) => (false, Some(format!("open failed to start: {error}"))),
    }
}

fn status_label(output: &Output) -> String {
    status_label_from_code(output.status.code())
}

fn status_label_from_code(code: Option<i32>) -> String {
    code.map_or_else(|| "signal".into(), |code| format!("code {code}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_url_from_nested_json() {
        let url =
            extract_first_url(r#"ok {"data":{"document":{"url":"https://example.test/doc"}}}"#);
        assert_eq!(url.as_deref(), Some("https://example.test/doc"));
    }

    #[test]
    fn markdown_export_content_gets_document_title_heading() {
        let content = markdown_with_document_title(
            "Moonbox Handoff - session title",
            "Reviewed handoff body",
        );

        assert_eq!(
            content,
            "# Moonbox Handoff - session title\n\nReviewed handoff body"
        );
    }

    #[test]
    fn markdown_export_content_does_not_duplicate_existing_title() {
        let content = markdown_with_document_title(
            "Moonbox Handoff - session title",
            "# Moonbox Handoff - session title\n\nReviewed handoff body",
        );

        assert_eq!(
            content,
            "# Moonbox Handoff - session title\n\nReviewed handoff body"
        );
    }

    #[test]
    fn title_patch_request_extracts_docx_token_from_url() {
        let request = title_patch_request(
            "https://example.feishu.cn/docx/ABC123?from=moonbox",
            "Moonbox Handoff - demo",
        )
        .expect("request");

        assert!(request.params.contains(r#""file_token":"ABC123""#));
        assert!(request.params.contains(r#""type":"docx""#));
        assert!(
            request
                .data
                .contains(r#""new_title":"Moonbox Handoff - demo""#)
        );
    }
}
