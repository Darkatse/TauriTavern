use reqwest::header::{ACCEPT, CONTENT_TYPE};
use serde_json::{Value, json};

use tt_domain::errors::DomainError;
use tt_ports::repositories::chat_completion_repository::{
    ChatCompletionApiConfig, ChatCompletionCancelReceiver, ChatCompletionStreamSender,
};

use super::HttpChatCompletionRepository;
use super::response_body::read_upstream_json_body;

pub(super) async fn list_models(
    repository: &HttpChatCompletionRepository,
    config: &ChatCompletionApiConfig,
) -> Result<Value, DomainError> {
    let url = HttpChatCompletionRepository::build_url(&config.base_url, "/models")?;

    let client = repository.metadata_client(config)?;
    let request = client.get(url).header(ACCEPT, "application/json");
    let request = HttpChatCompletionRepository::apply_bearer_auth(request, &config.api_key);
    let request = HttpChatCompletionRepository::apply_extra_headers(request, &config.extra_headers);
    let request = HttpChatCompletionRepository::apply_additional_headers(request, config);

    let response =
        HttpChatCompletionRepository::send_checked(request, "Cohere", "Failed to list models")
            .await?;

    let body = read_upstream_json_body("Cohere", "list_models", response).await?;

    Ok(json!({ "data": normalize_models(&body) }))
}

pub(super) async fn generate(
    repository: &HttpChatCompletionRepository,
    config: &ChatCompletionApiConfig,
    endpoint_path: &str,
    payload: &Value,
) -> Result<Value, DomainError> {
    let endpoint_path = normalize_endpoint_path(endpoint_path);
    let url = HttpChatCompletionRepository::build_url(&config.base_url, endpoint_path)?;

    let client = repository.client(config)?;
    let request = client
        .post(url)
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "application/json")
        .json(payload);

    let request = HttpChatCompletionRepository::apply_bearer_auth(request, &config.api_key);
    let request = HttpChatCompletionRepository::apply_extra_headers(request, &config.extra_headers);
    let request = HttpChatCompletionRepository::apply_additional_headers(request, config);

    let response =
        HttpChatCompletionRepository::send_checked(request, "Cohere", "Generation request failed")
            .await?;

    read_upstream_json_body("Cohere", "generate", response).await
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
    let url = HttpChatCompletionRepository::build_url(&config.base_url, endpoint_path)?;

    let client = repository.stream_client(config)?;
    let request = client
        .post(url)
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "text/event-stream")
        .json(payload);

    let request = HttpChatCompletionRepository::apply_bearer_auth(request, &config.api_key);
    let request = HttpChatCompletionRepository::apply_extra_headers(request, &config.extra_headers);
    let request = HttpChatCompletionRepository::apply_additional_headers(request, config);

    let response =
        HttpChatCompletionRepository::send_checked(request, "Cohere", "Generation request failed")
            .await?;

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
