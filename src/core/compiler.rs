use std::{
    env,
    error::Error,
    ffi::OsString,
    fmt, io,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    thread,
    time::{Duration, Instant},
};

use super::{
    config::{self, CompilerPresetConfig},
    model::{
        CapsuleCompileOutput, CapsuleCompileRequest, ChecklistItem, CompilerPresetInfo,
        CompilerPresetKind, CompilerPresetStatus, WorkCapsule,
    },
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompilerError {
    MissingRewind {
        compiler: String,
        rewind_event_id: String,
    },
    InvalidConfig {
        name: String,
        reason: String,
    },
    Spawn {
        compiler: String,
        program: String,
        reason: String,
    },
    Io {
        compiler: String,
        stream: &'static str,
        reason: String,
    },
    Timeout {
        compiler: String,
        timeout_ms: u64,
    },
    ProcessFailed {
        compiler: String,
        status: String,
        stderr: String,
    },
    InvalidOutput {
        compiler: String,
        reason: String,
    },
}

impl fmt::Display for CompilerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingRewind {
                compiler,
                rewind_event_id,
            } => write!(
                f,
                "{compiler} cannot compile missing rewind event {rewind_event_id}"
            ),
            Self::InvalidConfig { name, reason } => {
                write!(f, "invalid compiler config {name}: {reason}")
            }
            Self::Spawn {
                compiler,
                program,
                reason,
            } => write!(f, "{compiler} cannot start {program}: {reason}"),
            Self::Io {
                compiler,
                stream,
                reason,
            } => write!(f, "{compiler} {stream} I/O failed: {reason}"),
            Self::Timeout {
                compiler,
                timeout_ms,
            } => write!(f, "{compiler} timed out after {timeout_ms} ms"),
            Self::ProcessFailed {
                compiler,
                status,
                stderr,
            } => write!(f, "{compiler} exited with {status}: {stderr}"),
            Self::InvalidOutput { compiler, reason } => {
                write!(f, "{compiler} returned invalid output: {reason}")
            }
        }
    }
}

impl Error for CompilerError {}

pub const DEFAULT_COMPILER_ID: &str = "engineering-handoff";
pub const FIXTURE_COMPILER_IDS: [&str; 3] =
    [DEFAULT_COMPILER_ID, "bugfix-continuation", "design-review"];
const DEFAULT_PROCESS_TIMEOUT_MS: u64 = 30_000;

pub trait CapsuleCompiler {
    fn compile(
        &self,
        request: &CapsuleCompileRequest,
    ) -> Result<CapsuleCompileOutput, CompilerError>;
}

#[derive(Debug, Clone)]
pub struct ProcessCapsuleCompiler {
    id: String,
    program: PathBuf,
    args: Vec<String>,
    timeout: Duration,
}

impl ProcessCapsuleCompiler {
    pub fn new(id: impl Into<String>, program: impl Into<PathBuf>, args: Vec<String>) -> Self {
        Self {
            id: id.into(),
            program: program.into(),
            args,
            timeout: Duration::from_millis(DEFAULT_PROCESS_TIMEOUT_MS),
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn from_environment() -> Result<Option<Self>, CompilerError> {
        let Some(program) = env::var_os("MOONBOX_COMPILER") else {
            return Ok(None);
        };
        let program = PathBuf::from(program);
        let id = configured_process_compiler_id_for_program(&program);
        let args = configured_process_compiler_args()?;
        let timeout = configured_process_compiler_timeout()?;
        Ok(Some(Self::new(id, program, args).with_timeout(timeout)))
    }

    pub fn from_preset(preset: &CompilerPresetConfig) -> Result<Self, CompilerError> {
        validate_configured_timeout(preset.timeout_ms, &preset.id)?;
        let timeout = preset
            .timeout_ms
            .map(Duration::from_millis)
            .unwrap_or_else(|| Duration::from_millis(DEFAULT_PROCESS_TIMEOUT_MS));
        Ok(Self::new(
            preset.id.clone(),
            PathBuf::from(&preset.command),
            preset.args.clone(),
        )
        .with_timeout(timeout))
    }
}

impl CapsuleCompiler for ProcessCapsuleCompiler {
    fn compile(
        &self,
        request: &CapsuleCompileRequest,
    ) -> Result<CapsuleCompileOutput, CompilerError> {
        let input =
            serde_json::to_vec_pretty(request).map_err(|error| CompilerError::InvalidOutput {
                compiler: self.id.clone(),
                reason: format!("cannot serialize request: {error}"),
            })?;
        let mut child = Command::new(&self.program)
            .args(&self.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| CompilerError::Spawn {
                compiler: self.id.clone(),
                program: self.program.to_string_lossy().into_owned(),
                reason: error.to_string(),
            })?;

        let stdin = child.stdin.take().ok_or_else(|| CompilerError::Io {
            compiler: self.id.clone(),
            stream: "stdin",
            reason: "stdin was not piped".into(),
        })?;
        let stdout = child.stdout.take().ok_or_else(|| CompilerError::Io {
            compiler: self.id.clone(),
            stream: "stdout",
            reason: "stdout was not piped".into(),
        })?;
        let stderr = child.stderr.take().ok_or_else(|| CompilerError::Io {
            compiler: self.id.clone(),
            stream: "stderr",
            reason: "stderr was not piped".into(),
        })?;

        let writer = write_child_stdin(stdin, input);
        let stdout_reader = read_child_pipe(stdout);
        let stderr_reader = read_child_pipe(stderr);
        let outcome =
            wait_for_child(&mut child, self.timeout).map_err(|error| CompilerError::Io {
                compiler: self.id.clone(),
                stream: "process",
                reason: error.to_string(),
            })?;
        let stdin_result = join_io_thread(writer, &self.id, "stdin")?;
        let stdout = join_io_thread(stdout_reader, &self.id, "stdout")?.map_err(|error| {
            CompilerError::Io {
                compiler: self.id.clone(),
                stream: "stdout",
                reason: error.to_string(),
            }
        })?;
        let stderr = join_io_thread(stderr_reader, &self.id, "stderr")?.map_err(|error| {
            CompilerError::Io {
                compiler: self.id.clone(),
                stream: "stderr",
                reason: error.to_string(),
            }
        })?;

        let stderr = String::from_utf8_lossy(&stderr).trim().to_string();
        match outcome {
            WaitOutcome::TimedOut => {
                return Err(CompilerError::Timeout {
                    compiler: self.id.clone(),
                    timeout_ms: duration_millis(self.timeout),
                });
            }
            WaitOutcome::Exited(status) if !status.success() => {
                return Err(CompilerError::ProcessFailed {
                    compiler: self.id.clone(),
                    status: status_label(status),
                    stderr: truncate(&stderr, 500),
                });
            }
            WaitOutcome::Exited(_) => {}
        }
        if let Err(error) = stdin_result {
            return Err(CompilerError::Io {
                compiler: self.id.clone(),
                stream: "stdin",
                reason: error.to_string(),
            });
        }

        let stdout = String::from_utf8(stdout).map_err(|error| CompilerError::InvalidOutput {
            compiler: self.id.clone(),
            reason: error.to_string(),
        })?;
        serde_json::from_str::<CapsuleCompileOutput>(&stdout).map_err(|error| {
            CompilerError::InvalidOutput {
                compiler: self.id.clone(),
                reason: error.to_string(),
            }
        })
    }
}

#[derive(Debug, Clone, Copy)]
pub struct FixtureCapsuleCompiler;

impl CapsuleCompiler for FixtureCapsuleCompiler {
    fn compile(
        &self,
        request: &CapsuleCompileRequest,
    ) -> Result<CapsuleCompileOutput, CompilerError> {
        let Some(rewind_event) = request
            .timeline
            .events
            .iter()
            .find(|event| event.id == request.rewind_event_id)
        else {
            return Err(CompilerError::MissingRewind {
                compiler: request.compiler.clone(),
                rewind_event_id: request.rewind_event_id.clone(),
            });
        };

        let session = &request.source_session;
        let rewind_point = format!("{} / {}", request.rewind_event_id, rewind_event.detail);
        let title = truncate(&session.title, 96);
        let rewind_detail = truncate(&rewind_event.detail, 140);
        let source_health = session
            .health_reason
            .clone()
            .unwrap_or_else(|| "source session indexed successfully".into());
        let mut risks = vec![
            "Built-in draft compiler may omit tool outputs, attachments, and hidden provider state."
                .into(),
            format!(
                "{} target may need a stronger external compiler skill before execution.",
                request.target_cli
            ),
        ];
        if session.status != super::model::SessionStatus::Healthy {
            risks.push(format!("Source health needs review: {source_health}"));
        }

        Ok(CapsuleCompileOutput {
            version: 1,
            capsule: WorkCapsule {
                version: 1,
                source_cli: session.cli,
                target_cli: request.target_cli,
                source_session: session.id.clone(),
                rewind_point,
                compiler: request.compiler.clone(),
                target_branch: format!(
                    "moonbox/{}-rewind-{}",
                    request.target_cli.id(),
                    request.rewind_event_id
                ),
                goal: format!("Continue {title} from selected rewind point."),
                state: "draft_from_builtin_compiler".into(),
                decisions: vec![
                    "Source session is read-only; Moonbox only builds a handoff plan.".into(),
                    "This preview uses the built-in deterministic draft compiler.".into(),
                    "Production handoff should use a configured external compiler skill.".into(),
                ],
                todo: vec![
                    ChecklistItem {
                        done: true,
                        text: format!("Selected rewind point {}", request.rewind_event_id),
                    },
                    ChecklistItem {
                        done: false,
                        text: "Compile with an external skill for high-fidelity context.".into(),
                    },
                    ChecklistItem {
                        done: false,
                        text: format!(
                            "Review verifier output before launching {}",
                            request.target_cli
                        ),
                    },
                ],
                evidence: vec![
                    format!("session: {} ({})", session.id, session.cli),
                    format!("title: {title}"),
                    format!("cwd: {}", session.cwd),
                    format!("rewind: {rewind_detail}"),
                    format!("source health: {source_health}"),
                ],
                risks,
            },
        })
    }
}

pub fn default_compiler_id() -> String {
    configured_process_compiler_id()
        .or_else(|| {
            config::load_default_compiler().filter(|id| {
                compiler_catalog_entries().iter().any(|compiler| {
                    compiler.id == *id && compiler.status != CompilerPresetStatus::Disabled
                })
            })
        })
        .unwrap_or_else(|| DEFAULT_COMPILER_ID.into())
}

pub fn compiler_catalog() -> Vec<String> {
    compiler_catalog_entries()
        .into_iter()
        .map(|entry| entry.id)
        .collect()
}

pub fn compiler_catalog_entries() -> Vec<CompilerPresetInfo> {
    let mut compilers = Vec::new();
    if let Ok(Some(compiler)) = ProcessCapsuleCompiler::from_environment() {
        push_unique_compiler(&mut compilers, process_compiler_info(&compiler));
    } else if let Some(id) = configured_process_compiler_id() {
        push_unique_compiler(
            &mut compilers,
            CompilerPresetInfo {
                id,
                kind: CompilerPresetKind::Environment,
                status: CompilerPresetStatus::Warning,
                score: 25,
                command: env::var("MOONBOX_COMPILER").ok(),
                args: Vec::new(),
                timeout_ms: None,
                reason: "environment compiler is configured but invalid".into(),
            },
        );
    }
    for preset in config::load_compiler_presets() {
        push_unique_compiler(&mut compilers, preset_info(&preset));
    }
    for id in FIXTURE_COMPILER_IDS {
        push_unique_compiler(&mut compilers, builtin_info(id));
    }
    compilers
}

pub fn compile_with_configured_runner(
    request: &CapsuleCompileRequest,
) -> Result<CapsuleCompileOutput, CompilerError> {
    if let Some(compiler_id) = configured_process_compiler_id()
        && request.compiler == compiler_id
    {
        let compiler = ProcessCapsuleCompiler::from_environment()?.ok_or_else(|| {
            CompilerError::InvalidConfig {
                name: request.compiler.clone(),
                reason: "environment compiler disappeared before compile".into(),
            }
        })?;
        return compiler.compile(request);
    }
    for preset in config::load_compiler_presets() {
        if request.compiler == preset.id {
            if !preset.enabled {
                return Err(CompilerError::InvalidConfig {
                    name: preset.id,
                    reason: "compiler preset is disabled".into(),
                });
            }
            return ProcessCapsuleCompiler::from_preset(&preset)?.compile(request);
        }
    }
    if FIXTURE_COMPILER_IDS.contains(&request.compiler.as_str()) {
        return FixtureCapsuleCompiler.compile(request);
    }
    Err(CompilerError::InvalidConfig {
        name: request.compiler.clone(),
        reason: "compiler id is not configured".into(),
    })
}

pub fn default_rewind_event_id(session_id: &str) -> &'static str {
    match session_id {
        "claude-qc-platform" => "evt-074",
        "hermes-cxcp-502" => "evt-052",
        _ => "evt-091",
    }
}

fn configured_process_compiler_id() -> Option<String> {
    env::var("MOONBOX_COMPILER")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(|program| configured_process_compiler_id_for_program(Path::new(&program)))
}

fn configured_process_compiler_id_for_program(program: &Path) -> String {
    env::var("MOONBOX_COMPILER_ID")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| {
            program
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("external-compiler")
                .into()
        })
}

fn configured_process_compiler_args() -> Result<Vec<String>, CompilerError> {
    let Some(args) = env::var_os("MOONBOX_COMPILER_ARGS") else {
        return Ok(Vec::new());
    };
    let args = args_to_string(args, "MOONBOX_COMPILER_ARGS")?;
    if args.trim().is_empty() {
        return Ok(Vec::new());
    }
    serde_json::from_str::<Vec<String>>(&args).map_err(|error| CompilerError::InvalidConfig {
        name: "MOONBOX_COMPILER_ARGS".into(),
        reason: format!("expected JSON string array: {error}"),
    })
}

fn configured_process_compiler_timeout() -> Result<Duration, CompilerError> {
    let Some(timeout) = env::var_os("MOONBOX_COMPILER_TIMEOUT_MS") else {
        return Ok(Duration::from_millis(DEFAULT_PROCESS_TIMEOUT_MS));
    };
    let timeout = args_to_string(timeout, "MOONBOX_COMPILER_TIMEOUT_MS")?;
    let timeout = timeout
        .trim()
        .parse::<u64>()
        .map_err(|error| CompilerError::InvalidConfig {
            name: "MOONBOX_COMPILER_TIMEOUT_MS".into(),
            reason: error.to_string(),
        })?;
    validate_configured_timeout(Some(timeout), "MOONBOX_COMPILER_TIMEOUT_MS")?;
    Ok(Duration::from_millis(timeout))
}

fn validate_configured_timeout(timeout_ms: Option<u64>, name: &str) -> Result<(), CompilerError> {
    if timeout_ms == Some(0) {
        return Err(CompilerError::InvalidConfig {
            name: name.into(),
            reason: "timeout_ms must be greater than zero".into(),
        });
    }
    Ok(())
}

fn args_to_string(value: OsString, name: &str) -> Result<String, CompilerError> {
    value
        .into_string()
        .map_err(|_| CompilerError::InvalidConfig {
            name: name.into(),
            reason: "value is not valid UTF-8".into(),
        })
}

fn push_unique_compiler(compilers: &mut Vec<CompilerPresetInfo>, compiler: CompilerPresetInfo) {
    if !compilers.iter().any(|entry| entry.id == compiler.id) {
        compilers.push(compiler);
    }
}

fn process_compiler_info(compiler: &ProcessCapsuleCompiler) -> CompilerPresetInfo {
    process_info(
        compiler.id().into(),
        CompilerPresetKind::Environment,
        compiler.program.to_string_lossy().into_owned(),
        compiler.args.clone(),
        Some(duration_millis(compiler.timeout)),
        true,
    )
}

fn preset_info(preset: &CompilerPresetConfig) -> CompilerPresetInfo {
    process_info(
        preset.id.clone(),
        CompilerPresetKind::Config,
        preset.command.clone(),
        preset.args.clone(),
        preset.timeout_ms,
        preset.enabled,
    )
}

fn process_info(
    id: String,
    kind: CompilerPresetKind,
    command: String,
    args: Vec<String>,
    timeout_ms: Option<u64>,
    enabled: bool,
) -> CompilerPresetInfo {
    if !enabled {
        return CompilerPresetInfo {
            id,
            kind,
            status: CompilerPresetStatus::Disabled,
            score: 0,
            command: Some(command),
            args,
            timeout_ms,
            reason: "disabled in config".into(),
        };
    }
    if timeout_ms == Some(0) {
        return CompilerPresetInfo {
            id,
            kind,
            status: CompilerPresetStatus::Warning,
            score: 10,
            command: Some(command),
            args,
            timeout_ms,
            reason: "timeout_ms must be greater than zero".into(),
        };
    }
    if command_available(&command) {
        CompilerPresetInfo {
            id,
            kind,
            status: CompilerPresetStatus::Ready,
            score: 85,
            command: Some(command),
            args,
            timeout_ms,
            reason: "process compiler is available".into(),
        }
    } else {
        CompilerPresetInfo {
            id,
            kind,
            status: CompilerPresetStatus::Warning,
            score: 35,
            command: Some(command),
            args,
            timeout_ms,
            reason: "compiler command was not found on disk or PATH".into(),
        }
    }
}

fn builtin_info(id: &str) -> CompilerPresetInfo {
    CompilerPresetInfo {
        id: id.into(),
        kind: CompilerPresetKind::Builtin,
        status: CompilerPresetStatus::Ready,
        score: 45,
        command: None,
        args: Vec::new(),
        timeout_ms: None,
        reason: "built-in deterministic draft compiler; configure an external skill for production handoff".into(),
    }
}

fn command_available(command: &str) -> bool {
    let path = Path::new(command);
    if path.components().count() > 1 {
        return command_is_executable(path);
    }
    env::var_os("PATH")
        .map(|paths| env::split_paths(&paths).any(|dir| command_is_executable(&dir.join(command))))
        .unwrap_or(false)
}

fn command_is_executable(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata()
            .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        true
    }
}

enum WaitOutcome {
    Exited(ExitStatus),
    TimedOut,
}

fn wait_for_child(child: &mut Child, timeout: Duration) -> io::Result<WaitOutcome> {
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(WaitOutcome::Exited(status));
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(WaitOutcome::TimedOut);
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn write_child_stdin(
    mut stdin: impl Write + Send + 'static,
    input: Vec<u8>,
) -> thread::JoinHandle<io::Result<()>> {
    thread::spawn(move || {
        stdin.write_all(&input)?;
        stdin.write_all(b"\n")?;
        Ok(())
    })
}

fn read_child_pipe(
    mut pipe: impl Read + Send + 'static,
) -> thread::JoinHandle<io::Result<Vec<u8>>> {
    thread::spawn(move || {
        let mut output = Vec::new();
        pipe.read_to_end(&mut output)?;
        Ok(output)
    })
}

fn join_io_thread<T>(
    handle: thread::JoinHandle<io::Result<T>>,
    compiler: &str,
    stream: &'static str,
) -> Result<io::Result<T>, CompilerError> {
    handle.join().map_err(|_| CompilerError::Io {
        compiler: compiler.into(),
        stream,
        reason: "worker thread panicked".into(),
    })
}

fn status_label(status: ExitStatus) -> String {
    status
        .code()
        .map(|code| format!("exit code {code}"))
        .unwrap_or_else(|| "terminated by signal".into())
}

fn duration_millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{data, model::CliTool};
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicUsize, Ordering},
        time::{Duration, SystemTime, UNIX_EPOCH},
    };

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    #[cfg(unix)]
    static SCRIPT_COUNTER: AtomicUsize = AtomicUsize::new(0);

    #[test]
    fn fixture_compiler_rejects_missing_rewind() {
        let request =
            data::compile_request(CliTool::Codex, CliTool::Hermes, "missing").expect("request");

        let error = FixtureCapsuleCompiler
            .compile(&request)
            .expect_err("missing rewind");

        assert!(error.to_string().contains("missing"));
    }

    #[cfg(unix)]
    #[test]
    fn process_compiler_reads_request_and_returns_capsule() {
        let script = executable_script(
            "success",
            r#"#!/bin/sh
cat >/dev/null
cat <<'JSON'
{"version":1,"capsule":{"version":1,"source_cli":"codex","target_cli":"hermes","source_session":"codex-cxcp-design","rewind_point":"evt-091 / external","compiler":"process-skill","target_branch":"moonbox/hermes-rewind-evt-091","goal":"external compiler","state":"compiled","decisions":["external"],"todo":[{"done":false,"text":"verify"}],"evidence":["stdin read"],"risks":[]}}
JSON
"#,
        );
        let compiler = ProcessCapsuleCompiler::new("process-skill", script, Vec::new())
            .with_timeout(Duration::from_secs(5));
        let request = data::compile_request_with_compiler(
            CliTool::Codex,
            CliTool::Hermes,
            "evt-091",
            "process-skill",
        )
        .expect("request");

        let output = compiler.compile(&request).expect("output");

        assert_eq!(output.version, 1);
        assert_eq!(output.capsule.compiler, "process-skill");
        assert_eq!(output.capsule.goal, "external compiler");
    }

    #[cfg(unix)]
    #[test]
    fn process_compiler_rejects_invalid_json_output() {
        let script = executable_script(
            "invalid-json",
            r#"#!/bin/sh
cat >/dev/null
echo not-json
"#,
        );
        let compiler = ProcessCapsuleCompiler::new("bad-skill", script, Vec::new())
            .with_timeout(Duration::from_secs(5));
        let request = data::compile_request_with_compiler(
            CliTool::Codex,
            CliTool::Hermes,
            "evt-091",
            "bad-skill",
        )
        .expect("request");

        let error = compiler.compile(&request).expect_err("invalid json");

        assert!(matches!(error, CompilerError::InvalidOutput { .. }));
    }

    #[cfg(unix)]
    #[test]
    fn process_compiler_times_out() {
        let script = executable_script(
            "timeout",
            r#"#!/bin/sh
sleep 1
"#,
        );
        let compiler = ProcessCapsuleCompiler::new("slow-skill", script, Vec::new())
            .with_timeout(Duration::from_millis(10));
        let request = data::compile_request_with_compiler(
            CliTool::Codex,
            CliTool::Hermes,
            "evt-091",
            "slow-skill",
        )
        .expect("request");

        let error = compiler.compile(&request).expect_err("timeout");

        assert!(matches!(error, CompilerError::Timeout { .. }));
    }

    #[test]
    fn built_in_compilers_have_catalog_quality_signal() {
        let info = builtin_info(DEFAULT_COMPILER_ID);

        assert_eq!(info.id, DEFAULT_COMPILER_ID);
        assert_eq!(info.kind, CompilerPresetKind::Builtin);
        assert_eq!(info.status, CompilerPresetStatus::Ready);
        assert_eq!(info.score, 45);
        assert_eq!(info.command, None);
    }

    #[cfg(unix)]
    #[test]
    fn available_config_preset_has_ready_quality_signal() {
        let script = executable_script(
            "catalog-ready",
            r#"#!/bin/sh
cat >/dev/null
"#,
        );
        let preset = CompilerPresetConfig {
            id: "ready-skill".into(),
            command: script.to_string_lossy().into_owned(),
            args: vec!["--mode".into(), "handoff".into()],
            timeout_ms: Some(12_000),
            enabled: true,
        };

        let info = preset_info(&preset);

        assert_eq!(info.kind, CompilerPresetKind::Config);
        assert_eq!(info.status, CompilerPresetStatus::Ready);
        assert_eq!(info.score, 85);
        assert_eq!(info.args, ["--mode", "handoff"]);
        assert_eq!(info.timeout_ms, Some(12_000));
    }

    #[test]
    fn disabled_config_preset_is_not_runnable() {
        let preset = CompilerPresetConfig {
            id: "disabled-skill".into(),
            command: "/bin/moonbox-disabled".into(),
            args: Vec::new(),
            timeout_ms: None,
            enabled: false,
        };

        let info = preset_info(&preset);

        assert_eq!(info.status, CompilerPresetStatus::Disabled);
        assert_eq!(info.score, 0);
        assert!(info.reason.contains("disabled"));
    }

    #[test]
    fn missing_config_command_has_warning_quality_signal() {
        let preset = CompilerPresetConfig {
            id: "missing-skill".into(),
            command: format!("/tmp/moonbox-missing-compiler-{}", std::process::id()),
            args: Vec::new(),
            timeout_ms: Some(30_000),
            enabled: true,
        };

        let info = preset_info(&preset);

        assert_eq!(info.status, CompilerPresetStatus::Warning);
        assert_eq!(info.score, 35);
        assert!(info.reason.contains("not found"));
    }

    #[test]
    fn preset_compiler_rejects_zero_timeout() {
        let preset = CompilerPresetConfig {
            id: "zero-timeout".into(),
            command: "/bin/moonbox-handoff".into(),
            args: Vec::new(),
            timeout_ms: Some(0),
            enabled: true,
        };

        let error = ProcessCapsuleCompiler::from_preset(&preset).expect_err("invalid timeout");

        assert!(matches!(error, CompilerError::InvalidConfig { .. }));
        assert!(error.to_string().contains("timeout_ms"));
    }

    #[cfg(unix)]
    #[test]
    fn preset_compiler_executes_configured_command() {
        let script = executable_script(
            "preset-success",
            r#"#!/bin/sh
cat >/dev/null
cat <<'JSON'
{"version":1,"capsule":{"version":1,"source_cli":"codex","target_cli":"hermes","source_session":"codex-cxcp-design","rewind_point":"evt-091 / preset","compiler":"preset-skill","target_branch":"moonbox/hermes-rewind-evt-091","goal":"preset compiler","state":"compiled","decisions":["preset"],"todo":[{"done":false,"text":"verify"}],"evidence":["stdin read"],"risks":[]}}
JSON
"#,
        );
        let preset = CompilerPresetConfig {
            id: "preset-skill".into(),
            command: script.to_string_lossy().into_owned(),
            args: Vec::new(),
            timeout_ms: Some(5_000),
            enabled: true,
        };
        let compiler = ProcessCapsuleCompiler::from_preset(&preset).expect("preset compiler");
        let request = data::compile_request_with_compiler(
            CliTool::Codex,
            CliTool::Hermes,
            "evt-091",
            "preset-skill",
        )
        .expect("request");

        let output = compiler.compile(&request).expect("output");

        assert_eq!(output.capsule.compiler, "preset-skill");
        assert_eq!(output.capsule.goal, "preset compiler");
    }

    #[test]
    fn configured_runner_rejects_unknown_compiler_id() {
        let compiler_id = format!("unknown-skill-{}", std::process::id());
        let request = data::compile_request_with_compiler(
            CliTool::Codex,
            CliTool::Hermes,
            "evt-091",
            &compiler_id,
        )
        .expect("request");

        let error = compile_with_configured_runner(&request).expect_err("unknown compiler");

        assert!(matches!(error, CompilerError::InvalidConfig { .. }));
        assert!(error.to_string().contains("not configured"));
    }

    #[cfg(unix)]
    fn executable_script(name: &str, contents: &str) -> PathBuf {
        let unique = SCRIPT_COUNTER.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "moonbox-process-compiler-{name}-{}-{nanos}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("script dir");
        let path = dir.join("compiler.sh");
        fs::write(&path, contents).expect("script");
        let mut permissions = fs::metadata(&path).expect("metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions).expect("permissions");
        path
    }
}
