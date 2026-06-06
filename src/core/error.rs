use std::{error::Error, fmt};

use super::{adapter::AdapterError, compiler::CompilerError};

#[derive(Debug)]
pub enum CoreError {
    Adapter(AdapterError),
    Compiler(CompilerError),
    CapsuleRead { path: String, reason: String },
    CapsuleParse { path: String, reason: String },
    ExecuteRequiresSession { action: &'static str },
    LaunchPrepare { reason: String },
    LaunchBlocked { reason: String },
    LaunchStart { command: String, reason: String },
    ReplayEval { reason: String },
}

impl fmt::Display for CoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Adapter(error) => error.fmt(f),
            Self::Compiler(error) => error.fmt(f),
            Self::CapsuleRead { path, reason } => {
                write!(f, "cannot read Work Capsule {path}: {reason}")
            }
            Self::CapsuleParse { path, reason } => {
                write!(f, "cannot parse Work Capsule {path}: {reason}")
            }
            Self::ExecuteRequiresSession { action } => write!(
                f,
                "{action} execution requires an explicit --session; run a dry-run first or pass --session to avoid opening the newest active session by accident"
            ),
            Self::LaunchPrepare { reason } => {
                write!(f, "cannot prepare target launch: {reason}")
            }
            Self::LaunchBlocked { reason } => write!(f, "target launch blocked: {reason}"),
            Self::LaunchStart { command, reason } => {
                write!(f, "cannot start target launch `{command}`: {reason}")
            }
            Self::ReplayEval { reason } => write!(f, "replay eval failed: {reason}"),
        }
    }
}

impl Error for CoreError {}

impl From<AdapterError> for CoreError {
    fn from(error: AdapterError) -> Self {
        Self::Adapter(error)
    }
}

impl From<CompilerError> for CoreError {
    fn from(error: CompilerError) -> Self {
        Self::Compiler(error)
    }
}
