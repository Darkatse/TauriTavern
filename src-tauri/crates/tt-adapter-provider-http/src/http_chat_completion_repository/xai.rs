use serde_json::{Value, json};

use tt_domain::errors::DomainError;
use tt_ports::repositories::chat_completion_repository::ChatCompletionApiConfig;

use super::{HttpChatCompletionRepository, openai};

/// `/models` also lists the image and video generation models, so the chat
/// catalog comes from `/language-models`, which additionally reports the
/// modalities the vision gate needs.
pub(super) async fn list_models(
    repository: &HttpChatCompletionRepository,
    config: &ChatCompletionApiConfig,
) -> Result<Value, DomainError> {
    let body =
        openai::list_models_with_path(repository, config, "xAI", "/language-models").await?;

    let models = body.get("models").and_then(Value::as_array).ok_or_else(|| {
        DomainError::InternalError(
            "Invalid xAI models response: models is not an array".to_string(),
        )
    })?;

    Ok(json!({ "data": models }))
}
