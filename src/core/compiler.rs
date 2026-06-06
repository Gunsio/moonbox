use std::{error::Error, fmt};

use super::model::{CapsuleCompileOutput, CapsuleCompileRequest, ChecklistItem, WorkCapsule};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompilerError {
    MissingRewind {
        compiler: String,
        rewind_event_id: String,
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
        }
    }
}

impl Error for CompilerError {}

pub trait CapsuleCompiler {
    fn compile(
        &self,
        request: &CapsuleCompileRequest,
    ) -> Result<CapsuleCompileOutput, CompilerError>;
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

pub fn default_rewind_event_id(session_id: &str) -> &'static str {
    match session_id {
        "claude-qc-platform" => "evt-074",
        "hermes-cxcp-502" => "evt-052",
        _ => "evt-091",
    }
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

    #[test]
    fn fixture_compiler_rejects_missing_rewind() {
        let request =
            data::compile_request(CliTool::Codex, CliTool::Hermes, "missing").expect("request");

        let error = FixtureCapsuleCompiler
            .compile(&request)
            .expect_err("missing rewind");

        assert!(error.to_string().contains("missing"));
    }
}
