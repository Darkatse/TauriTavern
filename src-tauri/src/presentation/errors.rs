use crate::application::errors::ApplicationError;
use crate::domain::errors::DomainError;
use serde::Serialize;
use thiserror::Error;

#[derive(Error, Debug, Serialize)]
pub enum CommandError {
    #[error("Bad request: {0}")]
    BadRequest(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Unauthorized: {0}")]
    Unauthorized(String),

    #[error("Internal server error: {0}")]
    InternalServerError(String),
}

impl From<ApplicationError> for CommandError {
    fn from(error: ApplicationError) -> Self {
        match error {
            ApplicationError::ValidationError(msg) => CommandError::BadRequest(msg),
            ApplicationError::NotFound(msg) => CommandError::NotFound(msg),
            ApplicationError::Unauthorized(msg) => CommandError::Unauthorized(msg),
            ApplicationError::PermissionDenied(msg) => CommandError::Unauthorized(msg),
            ApplicationError::DomainError(msg) => CommandError::InternalServerError(msg),
            ApplicationError::InternalError(msg) => CommandError::InternalServerError(msg),
            ApplicationError::ServiceError(msg) => CommandError::InternalServerError(msg),
        }
    }
}

impl From<DomainError> for CommandError {
    fn from(error: DomainError) -> Self {
        match error {
            DomainError::NotFound(msg) => CommandError::NotFound(msg),
            DomainError::InvalidData(msg) => CommandError::BadRequest(msg),
            DomainError::PermissionDenied(msg) => CommandError::Unauthorized(msg),
            DomainError::AuthenticationError(msg) => CommandError::Unauthorized(msg),
            DomainError::InternalError(msg) => CommandError::InternalServerError(msg),
        }
    }
}

impl From<tauri::Error> for CommandError {
    fn from(error: tauri::Error) -> Self {
        CommandError::InternalServerError(error.to_string())
    }
}
