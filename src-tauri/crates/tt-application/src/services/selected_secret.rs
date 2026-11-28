use tt_ports::repositories::secret_repository::SecretRepository;

use crate::errors::ApplicationError;

/// The `secret_id` a request names; an empty one names none.
pub(crate) fn requested_secret_id(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|id| !id.is_empty())
}

/// The secret a request uses: the one `secret_id` names, or the active one when it names none.
/// A `secret_id` that matches no secret is an error; an empty value is no secret.
pub(crate) async fn read_selected_secret(
    secret_repository: &dyn SecretRepository,
    secret_key: &str,
    secret_id: Option<&str>,
) -> Result<Option<String>, ApplicationError> {
    let secret = secret_repository.read_secret(secret_key, secret_id).await?;
    if let (Some(id), None) = (secret_id, secret.as_ref()) {
        return Err(ApplicationError::ValidationError(format!(
            "Secret id not found for {secret_key}: {id}"
        )));
    }

    Ok(secret.filter(|value| !value.trim().is_empty()))
}
