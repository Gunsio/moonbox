use std::{error::Error, fmt};

use super::{adapter::AdapterError, compiler::CompilerError};

#[derive(Debug)]
pub enum CoreError {
    Adapter(AdapterError),
    Compiler(CompilerError),
    CapsuleRead { path: String, reason: String },
    CapsuleParse { path: String, reason: String },
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
