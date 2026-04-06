use reqwest::header::{ACCEPT, CONTENT_TYPE};
use serde_json::{Value, json};

use crate::domain::errors::DomainError;
use crate::domain::repositories::chat_completion_repository::{
    ChatCompletionApiConfig, ChatCompletionCancelReceiver, ChatCompletionStreamSender,
};

use super::HttpChatCompletionRepository;

pub(super) async fn list_models(
    repository: &HttpChatCompletionRepository,
    config: &ChatCompletionApiConfig,
) -> Result<Value, DomainError> {
    let url = HttpChatCompletionRepository::build_url(&config.base_url, "/models");

    let client = repository.client()?;
    let request = client.get(url).header(ACCEPT, "application/json");
    let request = HttpChatCompletionRepository::apply_bearer_auth(request, &config.api_key);
    let request = HttpChatCompletionRepository::apply_extra_headers(request, &config.extra_headers);

    let response = request
        .send()
        .await
        .map_err(|error| DomainError::InternalError(format!("Status request failed: {error}")))?;

    if !response.status().is_success() {
        return Err(HttpChatCompletionRepository::map_error_response(
            "Cohere",
            response,
            "Failed to list models",
        )
        .await);
    }

    let body = response.json::<Value>().await.map_err(|error| {
        DomainError::InternalError(format!("Failed to parse models JSON: {error}"))
    })?;

    Ok(json!({ "data": normalize_models(&body) }))
}

pub(super) async fn generate(
    repository: &HttpChatCompletionRepository,
    config: &ChatCompletionApiConfig,
    endpoint_path: &str,
    payload: &Value,
) -> Result<Value, DomainError> {
    let endpoint_path = normalize_endpoint_path(endpoint_path);
    let url = HttpChatCompletionRepository::build_url(&config.base_url, endpoint_path);

    let client = repository.client()?;
    let request = client
        .post(url)
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "application/json")
        .json(payload);

    let request = HttpChatCompletionRepository::apply_bearer_auth(request, &config.api_key);
    let request = HttpChatCompletionRepository::apply_extra_headers(request, &config.extra_headers);

    let response = request.send().await.map_err(|error| {
        DomainError::InternalError(format!("Generation request failed: {error}"))
    })?;

    if !response.status().is_success() {
        return Err(HttpChatCompletionRepository::map_error_response(
            "Cohere",
            response,
            "Generation request failed",
        )
        .await);
    }

    response.json::<Value>().await.map_err(|error| {
        DomainError::InternalError(format!("Failed to parse generation JSON: {error}"))
    })
}

pub(super) async fn generate_stream(
    repository: &HttpChatCompletionRepository,
    config: &ChatCompletionApiConfig,
    endpoint_path: &str,
    payload: &Value,
    sender: ChatCompletionStreamSender,
    cancel: ChatCompletionCancelReceiver,
) -> Result<(), DomainError> {
    let endpoint_path = normalize_endpoint_path(endpoint_path);
    let url = HttpChatCompletionRepository::build_url(&config.base_url, endpoint_path);

    let client = repository.stream_client()?;
    let request = client
        .post(url)
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "text/event-stream")
        .json(payload);

    let request = HttpChatCompletionRepository::apply_bearer_auth(request, &config.api_key);
    let request = HttpChatCompletionRepository::apply_extra_headers(request, &config.extra_headers);

    let response = request.send().await.map_err(|error| {
        DomainError::InternalError(format!("Generation request failed: {error}"))
    })?;

    if !response.status().is_success() {
        return Err(HttpChatCompletionRepository::map_error_response(
            "Cohere",
            response,
            "Generation request failed",
        )
        .await);
    }

    HttpChatCompletionRepository::stream_sse_response("Cohere", response, sender, cancel).await
}

fn normalize_endpoint_path(endpoint_path: &str) -> &str {
    let endpoint_path = endpoint_path.trim();
    if endpoint_path.is_empty() {
        "/chat"
    } else {
        endpoint_path
    }
}

fn normalize_models(body: &Value) -> Vec<Value> {
    let Some(entries) = body
        .get("models")
        .and_then(Value::as_array)
        .or_else(|| body.get("data").and_then(Value::as_array))
    else {
        return Vec::new();
    };

    entries.iter().filter_map(normalize_model_entry).collect()
}

fn normalize_model_entry(entry: &Value) -> Option<Value> {
    match entry {
        Value::String(value) => {
            let value = value.trim();
            if value.is_empty() {
                None
            } else {
                Some(json!({ "id": value }))
            }
        }
        Value::Object(object) => {
            if object
                .get("id")
                .and_then(Value::as_str)
                .map(str::trim)
                .is_some_and(|value| !value.is_empty())
            {
                return Some(Value::Object(object.clone()));
            }

            let name = object
                .get("name")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())?;

            let mut model = object.clone();
            model.insert("id".to_string(), Value::String(name.to_string()));
            Some(Value::Object(model))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::normalize_models;

    #[test]
    fn normalize_models_converts_name_to_id() {
        let payload = json!({
            "models": [
                {"name": "command-r-plus", "context_length": 1},
                {"name": "command-r", "context_length": 2}
            ]
        });

        let models = normalize_models(&payload);
        assert_eq!(models.len(), 2);
        assert_eq!(models[0]["id"], "command-r-plus");
        assert_eq!(models[0]["context_length"], 1);
        assert_eq!(models[1]["id"], "command-r");
        assert_eq!(models[1]["context_length"], 2);
    }
}
