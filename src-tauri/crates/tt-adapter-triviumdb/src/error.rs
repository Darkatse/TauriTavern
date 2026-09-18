use triviumdb::TriviumError;
use tt_domain::errors::DomainError;

pub(crate) fn engine_error(namespace: &str, error: TriviumError) -> DomainError {
    let message = format!("Database {namespace}: {error}");
    match error {
        TriviumError::NodeNotFound(_) => DomainError::NotFound(message),
        TriviumError::NodeAlreadyExists(_)
        | TriviumError::UniqueConstraintViolation { .. }
        | TriviumError::ConditionalUpdateNotMatched { .. }
        | TriviumError::DatabaseLocked(_)
        | TriviumError::DatabaseClosed
        | TriviumError::ReadOnlyViolation { .. } => DomainError::Conflict(message),
        TriviumError::DimensionMismatch { .. }
        | TriviumError::InvalidVector { .. }
        | TriviumError::InvalidInput(_)
        | TriviumError::PayloadTooLarge { .. }
        | TriviumError::QueryParse(_)
        | TriviumError::QueryExecution(_) => DomainError::InvalidData(message),
        TriviumError::QueryCancelled => DomainError::Cancelled(message),
        TriviumError::CapacityReservationRejected { .. }
        | TriviumError::QueryRowBudgetExceeded { .. }
        | TriviumError::PayloadQueryBudgetExceeded { .. }
        | TriviumError::TraversalBudgetExceeded { .. } => DomainError::rate_limited(message),
        _ => DomainError::InternalError(message),
    }
}
