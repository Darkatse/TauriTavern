use serde_json::{Value, json};

use tt_domain::errors::DomainError;
use tt_ports::repositories::chat_completion_repository::ChatCompletionApiConfig;

use super::{HttpChatCompletionRepository, openai};

pub(super) async fn list_models(
    repository: &HttpChatCompletionRepository,
    config: &ChatCompletionApiConfig,
) -> Result<Value, DomainError> {
    let body = openai::list_models(repository, config, "Pollinations").await?;

    let models = body.as_array().ok_or_else(|| {
        DomainError::InternalError(
            "Invalid Pollinations models response: body is not an array".to_string(),
        )
    })?;

    let data = models
        .iter()
        .map(|model| {
            let object = model.as_object().ok_or_else(|| {
                DomainError::InternalError(
                    "Invalid Pollinations models response: model is not an object".to_string(),
                )
            })?;
            let name = object
                .get("name")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    DomainError::InternalError(
                        "Invalid Pollinations models response: model name is missing".to_string(),
                    )
                })?;
            let mut object = object.clone();
            object.insert("id".to_string(), Value::String(name.to_string()));
            Ok(Value::Object(object))
        })
        .collect::<Result<Vec<_>, DomainError>>()?;

    Ok(json!({ "data": data }))
}
