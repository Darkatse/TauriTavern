use crate::domain::errors::DomainError;

pub(super) fn verify_integrity_match(
    existing_integrity: Option<&str>,
    incoming_integrity: Option<&str>,
) -> Result<(), DomainError> {
    match (existing_integrity, incoming_integrity) {
        (Some(existing), Some(incoming)) if existing == incoming => Ok(()),
        (Some(_), _) => Err(DomainError::InvalidData("integrity".to_string())),
        _ => Ok(()),
    }
}
