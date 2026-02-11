use std::collections::HashMap;
use std::time::Duration;

use async_trait::async_trait;
use reqwest::header::AUTHORIZATION;
use reqwest::{Client, RequestBuilder, StatusCode};
use serde_json::Value;

use crate::domain::errors::DomainError;
use crate::domain::repositories::chat_completion_repository::{
    ChatCompletionApiConfig, ChatCompletionRepository, ChatCompletionSource,
};

mod claude;
mod makersuite;
mod normalizers;
mod openai;

pub struct HttpChatCompletionRepository {
    client: Client,
}

impl HttpChatCompletionRepository {
    pub fn new() -> Result<Self, DomainError> {
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(120))
            .build()
            .map_err(|error| {
                DomainError::InternalError(format!("Failed to build HTTP client: {error}"))
            })?;

        Ok(Self { client })
    }

    fn build_url(base_url: &str, path: &str) -> String {
        format!("{}{}", base_url.trim_end_matches('/'), path)
    }

    fn apply_bearer_auth(request: RequestBuilder, api_key: &str) -> RequestBuilder {
        if api_key.trim().is_empty() {
            request
        } else {
            request.header(AUTHORIZATION, format!("Bearer {api_key}"))
        }
    }

    fn apply_header_if_present(
        request: RequestBuilder,
        header_name: &str,
        header_value: &str,
    ) -> RequestBuilder {
        if header_value.trim().is_empty() {
            request
        } else {
            request.header(header_name, header_value)
        }
    }

    fn apply_extra_headers(
        mut request: RequestBuilder,
        headers: &HashMap<String, String>,
    ) -> RequestBuilder {
        for (key, value) in headers {
            if key.trim().is_empty() || value.trim().is_empty() {
                continue;
            }
            request = request.header(key.as_str(), value.as_str());
        }

        request
    }

    async fn map_error_response(
        provider_name: &str,
        response: reqwest::Response,
        default_message: &str,
    ) -> DomainError {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        let message = extract_error_message(&body, default_message);

        match status {
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
                DomainError::AuthenticationError(message)
            }
            StatusCode::BAD_REQUEST => DomainError::InvalidData(message),
            _ => DomainError::InternalError(format!(
                "{provider_name} endpoint failed with status {}: {message}",
                status.as_u16()
            )),
        }
    }
}

#[async_trait]
impl ChatCompletionRepository for HttpChatCompletionRepository {
    async fn list_models(
        &self,
        source: ChatCompletionSource,
        config: &ChatCompletionApiConfig,
    ) -> Result<Value, DomainError> {
        match source {
            ChatCompletionSource::OpenAi => openai::list_models(self, config, "OpenAI").await,
            ChatCompletionSource::Custom => {
                openai::list_models(self, config, "Custom OpenAI").await
            }
            ChatCompletionSource::DeepSeek => openai::list_models(self, config, "DeepSeek").await,
            ChatCompletionSource::Moonshot => {
                openai::list_models(self, config, "Moonshot AI").await
            }
            ChatCompletionSource::SiliconFlow => {
                openai::list_models(self, config, "SiliconFlow").await
            }
            ChatCompletionSource::Zai => openai::list_models(self, config, "Z.AI (GLM)").await,
            ChatCompletionSource::Claude => claude::list_models(self, config).await,
            ChatCompletionSource::Makersuite => makersuite::list_models(self, config).await,
        }
    }

    async fn generate(
        &self,
        source: ChatCompletionSource,
        config: &ChatCompletionApiConfig,
        endpoint_path: &str,
        payload: &Value,
    ) -> Result<Value, DomainError> {
        match source {
            ChatCompletionSource::OpenAi => {
                openai::generate(self, config, endpoint_path, payload, "OpenAI").await
            }
            ChatCompletionSource::Custom => {
                openai::generate(self, config, endpoint_path, payload, "Custom OpenAI").await
            }
            ChatCompletionSource::DeepSeek => {
                openai::generate(self, config, endpoint_path, payload, "DeepSeek").await
            }
            ChatCompletionSource::Moonshot => {
                openai::generate(self, config, endpoint_path, payload, "Moonshot AI").await
            }
            ChatCompletionSource::SiliconFlow => {
                openai::generate(self, config, endpoint_path, payload, "SiliconFlow").await
            }
            ChatCompletionSource::Zai => {
                openai::generate(self, config, endpoint_path, payload, "Z.AI (GLM)").await
            }
            ChatCompletionSource::Claude => {
                claude::generate(self, config, endpoint_path, payload).await
            }
            ChatCompletionSource::Makersuite => makersuite::generate(self, config, payload).await,
        }
    }
}

fn extract_error_message(body: &str, default_message: &str) -> String {
    let body = body.trim();
    if body.is_empty() {
        return default_message.to_string();
    }

    if let Ok(value) = serde_json::from_str::<Value>(body) {
        if let Some(message) = value
            .get("error")
            .and_then(Value::as_object)
            .and_then(|error| error.get("message"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return message.to_string();
        }

        if let Some(message) = value
            .get("message")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return message.to_string();
        }
    }

    body.to_string()
}
