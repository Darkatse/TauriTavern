use thiserror::Error;

pub const GENERATION_CANCELLED_BY_USER_MESSAGE: &str = "Generation cancelled by user";

#[derive(Error, Debug)]
pub enum DomainError {
    #[error("Entity not found: {0}")]
    NotFound(String),

    #[error("Invalid data: {0}")]
    InvalidData(String),

    #[error("Authentication error: {0}")]
    AuthenticationError(String),

    #[error("{0}")]
    Cancelled(String),

    #[error("Internal error: {0}")]
    InternalError(String),

    #[error("{message}")]
    RateLimited { message: String },
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generation_cancelled_by_user_is_cancelled_variant() {
        let error = DomainError::generation_cancelled_by_user();

        assert!(matches!(
            &error,
            DomainError::Cancelled(message) if message == GENERATION_CANCELLED_BY_USER_MESSAGE
        ));
    }

    #[test]
    fn cancelled_constructor_keeps_message() {
        let error = DomainError::cancelled("Job cancelled");

        assert!(matches!(
            &error,
            DomainError::Cancelled(message) if message == "Job cancelled"
        ));
    }
}
