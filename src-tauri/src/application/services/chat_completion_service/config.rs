use std::collections::HashMap;
use std::sync::Arc;

use serde_json::Value;

use crate::application::dto::chat_completion_dto::{
    ChatCompletionGenerateRequestDto, ChatCompletionStatusRequestDto,
};
use crate::application::errors::ApplicationError;
use crate::domain::models::secret::SecretKeys;
use crate::domain::repositories::chat_completion_repository::{
    ChatCompletionApiConfig, ChatCompletionSource,
};
use crate::domain::repositories::secret_repository::SecretRepository;

use super::custom_parameters;

const OPENAI_API_BASE: &str = "https://api.openai.com/v1";
const CLAUDE_API_BASE: &str = "https://api.anthropic.com/v1";
const GEMINI_API_BASE: &str = "https://generativelanguage.googleapis.com";

pub(super) async fn resolve_status_api_config(
    source: ChatCompletionSource,
    dto: &ChatCompletionStatusRequestDto,
    secret_repository: &Arc<dyn SecretRepository>,
) -> Result<ChatCompletionApiConfig, ApplicationError> {
    let reverse_proxy = dto.reverse_proxy.trim();
    let proxy_password = dto.proxy_password.trim();

    let custom_url = dto.custom_url.trim();
    let custom_headers_raw = dto.custom_include_headers.as_str();

    resolve_api_config(
        source,
        reverse_proxy,
        proxy_password,
        custom_url,
        custom_headers_raw,
        secret_repository,
    )
    .await
}

pub(super) async fn resolve_generate_api_config(
    source: ChatCompletionSource,
    dto: &ChatCompletionGenerateRequestDto,
    secret_repository: &Arc<dyn SecretRepository>,
) -> Result<ChatCompletionApiConfig, ApplicationError> {
    let reverse_proxy = dto.get_string("reverse_proxy").unwrap_or_default().trim();
    let proxy_password = dto.get_string("proxy_password").unwrap_or_default().trim();
    let custom_url_raw = get_payload_string(&dto.payload, "custom_url");
    let custom_url = custom_url_raw.trim();
    let custom_headers_raw = get_payload_string(&dto.payload, "custom_include_headers");

    resolve_api_config(
        source,
        reverse_proxy,
        proxy_password,
        custom_url,
        &custom_headers_raw,
        secret_repository,
    )
    .await
}

async fn resolve_api_config(
    source: ChatCompletionSource,
    reverse_proxy: &str,
    proxy_password: &str,
    custom_url: &str,
    custom_headers_raw: &str,
    secret_repository: &Arc<dyn SecretRepository>,
) -> Result<ChatCompletionApiConfig, ApplicationError> {
    match source {
        ChatCompletionSource::OpenAi
        | ChatCompletionSource::Claude
        | ChatCompletionSource::Makersuite => {
            let base_url = if reverse_proxy.is_empty() {
                default_base_url(source).to_string()
            } else {
                reverse_proxy.to_string()
            };

            let api_key = if reverse_proxy.is_empty() {
                let secret_key = source_secret_key(source).ok_or_else(|| {
                    ApplicationError::InternalError(
                        "Secret key mapping is missing for chat completion source".to_string(),
                    )
                })?;

                read_required_secret(secret_repository, secret_key, source_display_name(source))
                    .await?
            } else {
                proxy_password.to_string()
            };

            Ok(ChatCompletionApiConfig {
                base_url,
                api_key,
                extra_headers: HashMap::new(),
            })
        }
        ChatCompletionSource::Custom => {
            let base_url = resolve_custom_base_url(custom_url, reverse_proxy)?;
            let extra_headers = custom_parameters::parse_string_map(custom_headers_raw)?;

            let api_key = if reverse_proxy.is_empty() {
                read_optional_secret(secret_repository, SecretKeys::CUSTOM)
                    .await?
                    .unwrap_or_default()
            } else {
                proxy_password.to_string()
            };

            Ok(ChatCompletionApiConfig {
                base_url,
                api_key,
                extra_headers,
            })
        }
    }
}

fn resolve_custom_base_url(
    custom_url: &str,
    reverse_proxy: &str,
) -> Result<String, ApplicationError> {
    if !custom_url.is_empty() {
        return Ok(custom_url.to_string());
    }

    if !reverse_proxy.is_empty() {
        return Ok(reverse_proxy.to_string());
    }

    Err(ApplicationError::ValidationError(
        "Custom endpoint is missing. Please configure custom_url.".to_string(),
    ))
}

fn get_payload_string(payload: &serde_json::Map<String, Value>, key: &str) -> String {
    payload
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_default()
}

async fn read_required_secret(
    secret_repository: &Arc<dyn SecretRepository>,
    secret_key: &str,
    source_name: &str,
) -> Result<String, ApplicationError> {
    secret_repository
        .read_secret(secret_key)
        .await?
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            ApplicationError::ValidationError(format!(
                "{} API key is missing. Please configure {}.",
                source_name, secret_key
            ))
        })
}

async fn read_optional_secret(
    secret_repository: &Arc<dyn SecretRepository>,
    secret_key: &str,
) -> Result<Option<String>, ApplicationError> {
    Ok(secret_repository
        .read_secret(secret_key)
        .await?
        .filter(|value| !value.trim().is_empty()))
}

fn default_base_url(source: ChatCompletionSource) -> &'static str {
    match source {
        ChatCompletionSource::OpenAi => OPENAI_API_BASE,
        ChatCompletionSource::Claude => CLAUDE_API_BASE,
        ChatCompletionSource::Makersuite => GEMINI_API_BASE,
        ChatCompletionSource::Custom => OPENAI_API_BASE,
    }
}

fn source_secret_key(source: ChatCompletionSource) -> Option<&'static str> {
    match source {
        ChatCompletionSource::OpenAi => Some(SecretKeys::OPENAI),
        ChatCompletionSource::Claude => Some(SecretKeys::CLAUDE),
        ChatCompletionSource::Makersuite => Some(SecretKeys::MAKERSUITE),
        ChatCompletionSource::Custom => Some(SecretKeys::CUSTOM),
    }
}

fn source_display_name(source: ChatCompletionSource) -> &'static str {
    match source {
        ChatCompletionSource::OpenAi => "OpenAI",
        ChatCompletionSource::Claude => "Claude",
        ChatCompletionSource::Makersuite => "Google Gemini",
        ChatCompletionSource::Custom => "Custom OpenAI",
    }
}
