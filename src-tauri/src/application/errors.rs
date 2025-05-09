use thiserror::Error;

use crate::domain::errors::DomainError;

#[derive(Error, Debug)]
pub enum ApplicationError {
    #[error("Domain error: {0}")]
    DomainError(String),

    #[error("Internal error: {0}")]
    InternalError(String),

    #[error("Validation error: {0}")]
    ValidationError(String),

    #[error("Service error: {0}")]
    ServiceError(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Unauthorized: {0}")]
    Unauthorized(String),

    #[error("Permission denied: {0}")]
    PermissionDenied(String),
}

impl From<DomainError> for ApplicationError {
    fn from(error: DomainError) -> Self {
        match error {
            DomainError::NotFound(msg) => ApplicationError::NotFound(msg),
            DomainError::InvalidData(msg) => ApplicationError::ValidationError(msg),
            DomainError::PermissionDenied(msg) => ApplicationError::PermissionDenied(msg),
            DomainError::AuthenticationError(msg) => ApplicationError::Unauthorized(msg),
            DomainError::InternalError(msg) => ApplicationError::InternalError(msg),
        }
    }
}
