use thiserror::Error;

use crate::domain::errors::DomainError;

#[derive(Error, Debug)]
pub enum ApplicationError {
    #[error("{0}")]
    RateLimited(String),

    #[error("{0}")]
    Cancelled(String),

    #[error("Internal error: {0}")]
    InternalError(String),

    #[error("Validation error: {0}")]
    ValidationError(String),

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
            DomainError::AuthenticationError(msg) => ApplicationError::Unauthorized(msg),
            DomainError::Cancelled(msg) => ApplicationError::Cancelled(msg),
            DomainError::InternalError(msg) => ApplicationError::InternalError(msg),
            DomainError::RateLimited { message } => ApplicationError::RateLimited(message),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::domain::errors::GENERATION_CANCELLED_BY_USER_MESSAGE;

    use super::*;

    #[test]
    fn domain_cancelled_maps_to_application_cancelled() {
        let error: ApplicationError = DomainError::generation_cancelled_by_user().into();

        assert!(matches!(
            &error,
            ApplicationError::Cancelled(message) if message == GENERATION_CANCELLED_BY_USER_MESSAGE
        ));
    }
}
