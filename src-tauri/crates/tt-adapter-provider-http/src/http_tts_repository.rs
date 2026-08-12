use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use reqwest::header::CONTENT_TYPE;
use reqwest::{RequestBuilder, Response, StatusCode};
use serde_json::Value;
use tokio::time::sleep;

use tt_adapter_http::{HttpClientPool, HttpClientProfile};
use tt_domain::errors::DomainError;
use tt_ports::repositories::tts_repository::{TtsRepository, TtsRequest, TtsRouteResponse};

mod azure;
mod elevenlabs;
mod google;
mod grok;
mod mimo;
mod minimax;
mod novelai;
mod openai;
mod pollinations;
mod volcengine;

const RETRIES: usize = 2;
const RETRY_DELAY_MS: u64 = 350;

pub struct HttpTtsRepository {
    http_clients: Arc<HttpClientPool>,
}

impl HttpTtsRepository {
    pub fn new(http_clients: Arc<HttpClientPool>) -> Self {
        Self { http_clients }
    }

    fn http_client(&self) -> Result<reqwest::Client, DomainError> {
        self.http_clients.client(HttpClientProfile::Tts)
    }
}

#[async_trait]
impl TtsRepository for HttpTtsRepository {
    async fn handle(&self, request: TtsRequest) -> Result<TtsRouteResponse, DomainError> {
        let client = self.http_client()?;

        match request {
            TtsRequest::Azure(request) => azure::handle(client, request).await,
            TtsRequest::GoogleTranslate(request) => google::handle_translate(client, request).await,
            TtsRequest::GoogleGemini(request) => google::handle_gemini(client, request).await,
            TtsRequest::NovelAiGenerate {
                api_key,
                text,
                voice,
            } => novelai::generate(client, api_key, text, voice).await,
            TtsRequest::OpenAi(request) => openai::handle_openai(client, request).await,
            TtsRequest::ElectronHub(request) => openai::handle_electronhub(client, request).await,
            TtsRequest::ChutesGenerate {
                api_key,
                input,
                voice,
                speed,
            } => openai::generate_chutes(client, api_key, input, voice, speed).await,
            TtsRequest::ElevenLabs(request) => elevenlabs::handle(client, request).await,
            TtsRequest::Pollinations(request) => pollinations::handle(client, request).await,
            TtsRequest::Volcengine(request) => volcengine::generate(client, request).await,
            TtsRequest::GrokVoices { api_key } => grok::voices(client, api_key).await,
            TtsRequest::GrokGenerate {
                api_key,
                text,
                voice_id,
                language,
                output_format,
            } => grok::generate(client, api_key, text, voice_id, language, output_format).await,
            TtsRequest::MimoGenerate {
                api_key,
                text,
                voice_id,
                model,
                format,
                instructions,
            } => mimo::generate(client, api_key, text, voice_id, model, format, instructions).await,
            TtsRequest::MinimaxGenerate { request } => minimax::generate(client, request).await,
        }
    }
}

async fn send_with_retry<F>(label: &str, build: F) -> Result<Response, DomainError>
where
    F: Fn() -> RequestBuilder,
{
    let mut last_error = None;

    for attempt in 0..=RETRIES {
        match build().send().await {
            Ok(response) => {
                if !is_retryable_status(response.status()) || attempt == RETRIES {
                    return Ok(response);
                }
            }
            Err(error) => {
                if attempt == RETRIES {
                    return Err(DomainError::InternalError(format!(
                        "{label} failed: {error}"
                    )));
                }
                last_error = Some(error);
            }
        }

        sleep(Duration::from_millis(RETRY_DELAY_MS * (attempt as u64 + 1))).await;
    }

    Err(DomainError::InternalError(format!(
        "{label} failed: {}",
        last_error
            .map(|error| error.to_string())
            .unwrap_or_else(|| "request failed".to_string())
    )))
}

fn is_retryable_status(status: StatusCode) -> bool {
    matches!(status.as_u16(), 408 | 425 | 429 | 500 | 502 | 503 | 504)
}

async fn upstream_error_response(
    response: Response,
    fallback: &str,
) -> Result<TtsRouteResponse, DomainError> {
    let status = response.status().as_u16();
    let bytes = response.bytes().await.map_err(|error| {
        DomainError::InternalError(format!("Upstream error response read failed: {error}"))
    })?;
    Ok(TtsRouteResponse::text(
        status,
        parse_upstream_error_message(&bytes, fallback),
    ))
}

async fn bytes_response(
    response: Response,
    label: &str,
    fallback_content_type: &str,
    preserve_content_type: bool,
) -> Result<TtsRouteResponse, DomainError> {
    if !response.status().is_success() {
        return upstream_error_response(response, &format!("{label} failed")).await;
    }
    let content_type = if preserve_content_type {
        response_content_type(&response, fallback_content_type)
    } else {
        fallback_content_type.to_string()
    };
    let body = response.bytes().await.map_err(|error| {
        DomainError::InternalError(format!("{label} response read failed: {error}"))
    })?;
    Ok(TtsRouteResponse::bytes(200, content_type, body.to_vec()))
}

fn response_content_type(response: &Response, fallback: &str) -> String {
    response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback)
        .to_string()
}

fn parse_upstream_error_message(body: &[u8], fallback: &str) -> String {
    if let Ok(payload) = serde_json::from_slice::<Value>(body)
        && let Some(message) = parse_json_error_message(&payload)
    {
        return message;
    }

    let text = String::from_utf8_lossy(body).trim().to_string();
    if text.is_empty() {
        fallback.to_string()
    } else {
        text
    }
}

fn parse_json_error_message(payload: &Value) -> Option<String> {
    if let Some(message) = payload
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Some(message.to_string());
    }

    for key in ["error", "message", "detail"] {
        if let Some(message) = payload
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return Some(message.to_string());
        }
    }

    payload
        .get("error")
        .and_then(|value| value.get("message"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::parse_upstream_error_message;

    #[test]
    fn parses_nested_json_error_message() {
        assert_eq!(
            parse_upstream_error_message(
                br#"{"error":{"message":"Rate limited"}}"#,
                "Request failed",
            ),
            "Rate limited"
        );
    }

    #[test]
    fn preserves_plain_text_error_body() {
        assert_eq!(
            parse_upstream_error_message(b"upstream gateway timeout", "Request failed"),
            "upstream gateway timeout"
        );
    }

    #[test]
    fn falls_back_for_empty_error_body() {
        assert_eq!(
            parse_upstream_error_message(b"  ", "Request failed"),
            "Request failed"
        );
    }
}
