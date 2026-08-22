use std::fmt;

use thiserror::Error;

use crate::models::upstream_failure::UpstreamFailure;

pub const GENERATION_CANCELLED_BY_USER_MESSAGE: &str = "Generation cancelled by user";

#[derive(Error, Debug)]
pub enum DomainError {
    #[error("Entity not found: {0}")]
    NotFound(String),

    #[error("Invalid data: {0}")]
    InvalidData(String),

    #[error("Conflict: {0}")]
    Conflict(String),

    #[error("Authentication error: {0}")]
    AuthenticationError(String),

    #[error("{0}")]
    Cancelled(String),

    #[error("Internal error: {0}")]
    InternalError(String),

    #[error("{message}")]
    RateLimited { message: String },

    #[error("{0}")]
    Transient(String),

    #[error("{0}")]
    UpstreamFailure(UpstreamFailure),

    #[error("Workspace path is a directory: {path}")]
    WorkspacePathIsDirectory { path: String },

    #[error("Workspace file is not UTF-8 text: {path}")]
    WorkspaceFileNotText { path: String },

    #[error("Workspace write conflict at {path}: {kind}")]
    WorkspaceWriteConflict {
        path: String,
        kind: WorkspaceWriteConflictKind,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceWriteConflictKind {
    AlreadyExists {
        actual_sha256: String,
    },
    Stale {
        expected_sha256: String,
        actual_sha256: Option<String>,
    },
}

impl fmt::Display for WorkspaceWriteConflictKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyExists { actual_sha256 } => write!(
                formatter,
                "file already exists with sha256 {actual_sha256}; read it before rewriting it"
            ),
            Self::Stale {
                expected_sha256,
                actual_sha256: Some(actual_sha256),
            } => write!(
                formatter,
                "file changed since last read or write: expected sha256 {expected_sha256}, current sha256 {actual_sha256}"
            ),
            Self::Stale {
                expected_sha256,
                actual_sha256: None,
            } => write!(
                formatter,
                "file changed since last read or write: expected sha256 {expected_sha256}, but the file no longer exists"
            ),
        }
    }
}

impl DomainError {
    pub fn cancelled(message: impl Into<String>) -> Self {
        Self::Cancelled(message.into())
    }

    pub fn generation_cancelled_by_user() -> Self {
        Self::Cancelled(GENERATION_CANCELLED_BY_USER_MESSAGE.to_string())
    }

    pub fn rate_limited(message: impl Into<String>) -> Self {
        Self::RateLimited {
            message: message.into(),
        }
    }

    pub fn transient(message: impl Into<String>) -> Self {
        Self::Transient(message.into())
    }

    pub fn upstream_failure(failure: UpstreamFailure) -> Self {
        Self::UpstreamFailure(failure)
    }

    pub fn workspace_path_is_directory(path: impl Into<String>) -> Self {
        Self::WorkspacePathIsDirectory { path: path.into() }
    }

    pub fn workspace_file_not_text(path: impl Into<String>) -> Self {
        Self::WorkspaceFileNotText { path: path.into() }
    }

    pub fn workspace_write_conflict(
        path: impl Into<String>,
        kind: WorkspaceWriteConflictKind,
    ) -> Self {
        Self::WorkspaceWriteConflict {
            path: path.into(),
            kind,
        }
    }
}
