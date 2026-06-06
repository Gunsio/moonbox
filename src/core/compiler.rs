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

use super::model::{CapsuleCompileOutput, CapsuleCompileRequest, ChecklistItem, WorkCapsule};

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
        let (goal, state, risks) = session_profile(&session.id);
        let rewind_point = format!("{} / {}", request.rewind_event_id, rewind_event.detail);

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
                goal: goal.into(),
                state: state.into(),
                decisions: vec![
                    "Source sessions are read-only.".into(),
                    "Compression and compatibility live in replaceable compiler skills.".into(),
                    "TUI is a first-class workbench, not an fzf picker.".into(),
                ],
                todo: vec![
                    ChecklistItem {
                        done: true,
                        text: "Define canonical timeline and capsule schema.".into(),
                    },
                    ChecklistItem {
                        done: false,
                        text: "Implement source adapters for Codex, Claude, Hermes.".into(),
                    },
                    ChecklistItem {
                        done: false,
                        text: "Implement target launcher and verification loop.".into(),
                    },
                ],
                evidence: vec![
                    format!("session: {} ({})", session.id, session.cli),
                    format!("cwd: {}", session.cwd),
                    session
                        .health_reason
                        .clone()
                        .unwrap_or_else(|| "no health reason".into()),
                ],
                risks,
            },
        })
    }
}

pub fn default_compiler_id() -> String {
    configured_process_compiler_id().unwrap_or_else(|| DEFAULT_COMPILER_ID.into())
}

pub fn compiler_catalog() -> Vec<String> {
    let mut compilers = Vec::new();
    if let Some(id) = configured_process_compiler_id() {
        compilers.push(id);
    }
    for id in FIXTURE_COMPILER_IDS {
        if !compilers.iter().any(|compiler| compiler == id) {
            compilers.push(id.into());
        }
    }
    compilers
}

pub fn compile_with_configured_runner(
    request: &CapsuleCompileRequest,
) -> Result<CapsuleCompileOutput, CompilerError> {
    if let Some(compiler) = ProcessCapsuleCompiler::from_environment()?
        && request.compiler == compiler.id()
    {
        return compiler.compile(request);
    }
    FixtureCapsuleCompiler.compile(request)
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
    if timeout == 0 {
        return Err(CompilerError::InvalidConfig {
            name: "MOONBOX_COMPILER_TIMEOUT_MS".into(),
            reason: "must be greater than zero".into(),
        });
    }
    Ok(Duration::from_millis(timeout))
}

fn args_to_string(value: OsString, name: &str) -> Result<String, CompilerError> {
    value
        .into_string()
        .map_err(|_| CompilerError::InvalidConfig {
            name: name.into(),
            reason: "value is not valid UTF-8".into(),
        })
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

fn session_profile(session_id: &str) -> (&'static str, &'static str, Vec<String>) {
    match session_id {
        "claude-qc-platform" => (
            "Continue QC trace propagation repair without losing staging context.",
            "Trace propagation patch is drafted; staging verification is still pending.",
            vec![
                "Gateway fallback may hide upstream request_id bugs.".into(),
                "Staging traffic volume may not cover async retry paths.".into(),
            ],
        ),
        "hermes-cxcp-502" => (
            "Recover the cxcp investigation by avoiding raw copied-session resume.",
            "Raw resume failed with 502. The target path is Work Capsule handoff.",
            vec![
                "Copied session rows can miss hidden provider state.".into(),
                "Target CLI resume protocol may reject raw source metadata.".into(),
            ],
        ),
        _ => (
            "Build Moonbox as a cross-CLI session rewind workbench.",
            "Raw resume is rejected. The target path is new branch + Work Capsule.",
            vec![
                "Tool outputs and attachments can exceed target token budget.".into(),
                "Target CLI injection protocol may differ per tool.".into(),
            ],
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{data, model::CliTool};
    use std::{fs, path::PathBuf, time::Duration};

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

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

    #[cfg(unix)]
    fn executable_script(name: &str, contents: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "moonbox-process-compiler-{name}-{}.sh",
            std::process::id()
        ));
        fs::write(&path, contents).expect("script");
        let mut permissions = fs::metadata(&path).expect("metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions).expect("permissions");
        path
    }
}
