use thiserror::Error;

#[derive(Error, Debug)]
pub enum DomainError {
    #[error("Entity not found: {0}")]
    NotFound(String),

    #[error("Invalid data: {0}")]
    InvalidData(String),

    #[error("Authentication error: {0}")]
    AuthenticationError(String),

    #[error("Internal error: {0}")]
    InternalError(String),

    #[error("{message}")]
    RateLimited { message: String },
}

impl DomainError {
    pub fn rate_limited(message: impl Into<String>) -> Self {
        Self::RateLimited {
            message: message.into(),
        }
    }
}
