use serde_json::{Value, json};

use tt_domain::errors::DomainError;
use tt_ports::generation_stream::{GenerationCancelReceiver, GenerationStreamSender};
use tt_ports::repositories::chat_completion_repository::ChatCompletionApiConfig;

use std::borrow::Cow;

use super::{GenerationRequest, HttpChatCompletionRepository, response_format};
use crate::provider_request::ProviderRequest;
use crate::provider_response::{self, read_upstream_json_body};

pub(super) async fn list_models(
    repository: &HttpChatCompletionRepository,
    config: &ChatCompletionApiConfig,
) -> Result<Value, DomainError> {
    let url = HttpChatCompletionRepository::build_url(&config.base_url, "/models")?;
    let mut request = ProviderRequest::get_json(url);
    apply_headers(&mut request, config)?;

    let client = repository.metadata_client(config)?;
    let response = provider_response::send_checked(
        request.with_client(&client),
        "Cohere",
        "Failed to list models",
    )
    .await?;

    let body = read_upstream_json_body("Cohere", "list_models", response).await?;

    Ok(json!({ "data": normalize_models(&body) }))
}

pub(super) fn generation_request<'a>(
    config: &ChatCompletionApiConfig,
    endpoint_path: &str,
    payload: &'a Value,
    stream: bool,
) -> Result<GenerationRequest<'a>, DomainError> {
    let url = HttpChatCompletionRepository::build_url(
        &config.base_url,
        normalize_endpoint_path(endpoint_path),
    )?;
    let mut request = ProviderRequest::post_json(url, response_format(stream));
    apply_headers(&mut request, config)?;
    Ok(GenerationRequest::http(request, Cow::Borrowed(payload)))
}

fn apply_headers(
    request: &mut ProviderRequest,
    config: &ChatCompletionApiConfig,
) -> Result<(), DomainError> {
    HttpChatCompletionRepository::apply_bearer_auth(request, &config.api_key)?;
    HttpChatCompletionRepository::apply_extra_headers(request, &config.extra_headers)?;
    HttpChatCompletionRepository::apply_additional_headers(request, config)
}

pub(super) async fn generate(
    repository: &HttpChatCompletionRepository,
    config: &ChatCompletionApiConfig,
    endpoint_path: &str,
    payload: &Value,
) -> Result<Value, DomainError> {
    let request = generation_request(config, endpoint_path, payload, false)?;
    let response = repository
        .send_generation(config, request, false, "Cohere")
        .await?;

    read_upstream_json_body("Cohere", "generate", response).await
}

pub(super) async fn generate_stream(
    repository: &HttpChatCompletionRepository,
    config: &ChatCompletionApiConfig,
    endpoint_path: &str,
    payload: &Value,
    sender: GenerationStreamSender,
    cancel: GenerationCancelReceiver,
) -> Result<(), DomainError> {
    let request = generation_request(config, endpoint_path, payload, true)?;
    let response = repository
        .send_generation(config, request, true, "Cohere")
        .await?;

    provider_response::stream_sse_response("Cohere", response, sender, cancel).await
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
