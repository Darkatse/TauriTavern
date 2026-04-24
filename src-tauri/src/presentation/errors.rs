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

    #[error("{0}")]
    Cancelled(String),

    #[error("{0}")]
    TooManyRequests(String),

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
            ApplicationError::RateLimited(msg) => CommandError::TooManyRequests(msg),
            ApplicationError::Cancelled(msg) => CommandError::Cancelled(msg),
            ApplicationError::InternalError(msg) => CommandError::InternalServerError(msg),
        }
    }
}

impl From<DomainError> for CommandError {
    fn from(error: DomainError) -> Self {
        match error {
            DomainError::NotFound(msg) => CommandError::NotFound(msg),
            DomainError::InvalidData(msg) => CommandError::BadRequest(msg),
            DomainError::AuthenticationError(msg) => CommandError::Unauthorized(msg),
            DomainError::Cancelled(msg) => CommandError::Cancelled(msg),
            DomainError::InternalError(msg) => CommandError::InternalServerError(msg),
            DomainError::RateLimited { message } => CommandError::TooManyRequests(message),
        }
    }
}

impl From<tauri::Error> for CommandError {
    fn from(error: tauri::Error) -> Self {
        CommandError::InternalServerError(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use crate::domain::errors::GENERATION_CANCELLED_BY_USER_MESSAGE;

    use super::*;

    #[test]
    fn domain_cancelled_maps_to_command_cancelled() {
        let error: CommandError = DomainError::generation_cancelled_by_user().into();

        assert!(matches!(
            &error,
            CommandError::Cancelled(message) if message == GENERATION_CANCELLED_BY_USER_MESSAGE
        ));
    }

    #[test]
    fn application_cancelled_maps_to_command_cancelled() {
        let error: CommandError = ApplicationError::Cancelled("Job cancelled".to_string()).into();

        assert!(matches!(
            &error,
            CommandError::Cancelled(message) if message == "Job cancelled"
        ));
    }
}
