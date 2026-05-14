use serde_json::{Value, json};

use crate::domain::errors::DomainError;
use crate::domain::repositories::chat_completion_repository::ChatCompletionApiConfig;

use super::{HttpChatCompletionRepository, openai};

pub(super) async fn list_models(
    repository: &HttpChatCompletionRepository,
    config: &ChatCompletionApiConfig,
) -> Result<Value, DomainError> {
    let body = openai::list_models_with_path(
        repository,
        config,
        "Cloudflare Workers AI",
        "/models/search?task=Text%20Generation&per_page=1000",
    )
    .await?;

    let result = body
        .get("result")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            DomainError::InternalError(
                "Invalid Workers AI models response: result is not an array".to_string(),
            )
        })?;

    let data = result
        .iter()
        .map(|model| {
            let object = model.as_object().ok_or_else(|| {
                DomainError::InternalError(
                    "Invalid Workers AI models response: model is not an object".to_string(),
                )
            })?;
            let name = object
                .get("name")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    DomainError::InternalError(
                        "Invalid Workers AI models response: model name is missing".to_string(),
                    )
                })?;
            let mut object = object.clone();
            object.insert("id".to_string(), Value::String(name.to_string()));
            Ok(Value::Object(object))
        })
        .collect::<Result<Vec<_>, DomainError>>()?;

    Ok(json!({ "data": data }))
}
