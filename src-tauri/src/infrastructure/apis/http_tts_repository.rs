use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use reqwest::header::{ACCEPT, CONTENT_TYPE};
use reqwest::{RequestBuilder, Response, StatusCode};
use serde_json::{Value, json};
use tokio::time::sleep;

use crate::domain::errors::DomainError;
use crate::domain::repositories::tts_repository::{
    GrokOutputFormat, TtsRepository, TtsRequest, TtsRouteResponse,
};
use crate::infrastructure::http_client_pool::{HttpClientPool, HttpClientProfile};

const GROK_VOICES_URL: &str = "https://api.x.ai/v1/tts/voices";
const GROK_TTS_URL: &str = "https://api.x.ai/v1/tts";
const MIMO_CHAT_COMPLETIONS_URL: &str = "https://api.xiaomimimo.com/v1/chat/completions";
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
            TtsRequest::GrokVoices { api_key } => grok_voices(client, api_key).await,
            TtsRequest::GrokGenerate {
                api_key,
                text,
                voice_id,
                language,
                output_format,
            } => grok_generate(client, api_key, text, voice_id, language, output_format).await,
            TtsRequest::MimoGenerate {
                api_key,
                text,
                voice_id,
                model,
                format,
                instructions,
            } => mimo_generate(client, api_key, text, voice_id, model, format, instructions).await,
        }
    }
}

async fn grok_voices(
    client: reqwest::Client,
    api_key: String,
) -> Result<TtsRouteResponse, DomainError> {
    let response = send_with_retry("Grok voice list request", || {
        client
            .get(GROK_VOICES_URL)
            .bearer_auth(&api_key)
            .header(ACCEPT, "application/json")
    })
    .await?;

    if !response.status().is_success() {
        return upstream_error_response(response, "Grok voice list request failed").await;
    }

    let content_type = response_content_type(&response, "application/json");
    let bytes = response.bytes().await.map_err(|error| {
        DomainError::InternalError(format!("Grok voice list response read failed: {error}"))
    })?;

    if let Err(error) = serde_json::from_slice::<Value>(&bytes) {
        return Ok(TtsRouteResponse::text(
            502,
            format!("Grok voice list response is not valid JSON: {error}"),
        ));
    }

    Ok(TtsRouteResponse::bytes(200, content_type, bytes.to_vec()))
}

async fn grok_generate(
    client: reqwest::Client,
    api_key: String,
    text: String,
    voice_id: String,
    language: String,
    output_format: GrokOutputFormat,
) -> Result<TtsRouteResponse, DomainError> {
    let payload = json!({
        "text": text,
        "voice_id": voice_id,
        "language": language,
        "output_format": {
            "codec": output_format.codec,
            "sample_rate": output_format.sample_rate,
            "bit_rate": output_format.bit_rate,
        },
    });

    let response = send_with_retry("Grok TTS request", || {
        client
            .post(GROK_TTS_URL)
            .bearer_auth(&api_key)
            .header(ACCEPT, "*/*")
            .header(CONTENT_TYPE, "application/json")
            .json(&payload)
    })
    .await?;

    if !response.status().is_success() {
        return upstream_error_response(response, "Grok TTS request failed").await;
    }

    let content_type = response_content_type(&response, "audio/mpeg");
    let bytes = response.bytes().await.map_err(|error| {
        DomainError::InternalError(format!("Grok TTS response read failed: {error}"))
    })?;

    Ok(TtsRouteResponse::bytes(200, content_type, bytes.to_vec()))
}

async fn mimo_generate(
    client: reqwest::Client,
    api_key: String,
    text: String,
    voice_id: String,
    model: String,
    format: String,
    instructions: Option<String>,
) -> Result<TtsRouteResponse, DomainError> {
    let mut messages = Vec::new();
    if let Some(instructions) = instructions {
        messages.push(json!({
            "role": "user",
            "content": instructions,
        }));
    }
    messages.push(json!({
        "role": "assistant",
        "content": text,
    }));

    let payload = json!({
        "model": model,
        "messages": messages,
        "audio": {
            "format": format,
            "voice": voice_id,
        },
    });

    let response = send_with_retry("MiMo TTS request", || {
        client
            .post(MIMO_CHAT_COMPLETIONS_URL)
            .header("api-key", api_key.as_str())
            .header(ACCEPT, "application/json")
            .header(CONTENT_TYPE, "application/json")
            .json(&payload)
    })
    .await?;

    if !response.status().is_success() {
        return upstream_error_response(response, "MiMo TTS request failed").await;
    }

    let bytes = response.bytes().await.map_err(|error| {
        DomainError::InternalError(format!("MiMo TTS response read failed: {error}"))
    })?;

    let payload: Value = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(error) => {
            return Ok(TtsRouteResponse::text(
                502,
                format!("MiMo TTS response is not valid JSON: {error}"),
            ));
        }
    };

    let Some(audio_base64) = payload
        .get("choices")
        .and_then(|value| value.get(0))
        .and_then(|value| value.get("message"))
        .and_then(|value| value.get("audio"))
        .and_then(|value| value.get("data"))
        .and_then(|value| value.as_str())
        .filter(|value| !value.is_empty())
    else {
        return Ok(TtsRouteResponse::text(
            502,
            "MiMo TTS response did not include audio data",
        ));
    };

    let audio = match BASE64_STANDARD.decode(audio_base64.as_bytes()) {
        Ok(audio) => audio,
        Err(error) => {
            return Ok(TtsRouteResponse::text(
                502,
                format!("MiMo TTS audio data is not valid base64: {error}"),
            ));
        }
    };

    Ok(TtsRouteResponse::bytes(
        200,
        mimo_content_type(&format),
        audio,
    ))
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
    let message = parse_upstream_error_message(&bytes, fallback);
    Ok(TtsRouteResponse::text(status, message))
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
    if let Ok(payload) = serde_json::from_slice::<Value>(body) {
        if let Some(message) = parse_json_error_message(&payload) {
            return message;
        }
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

    for key in ["error", "message"] {
        if let Some(message) = payload
            .get(key)
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return Some(message.to_string());
        }
    }

    payload
        .get("error")
        .and_then(|value| value.get("message"))
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn mimo_content_type(format: &str) -> &'static str {
    match format {
        "mp3" => "audio/mpeg",
        _ => "audio/wav",
    }
}

#[cfg(test)]
mod tests {
    use super::parse_upstream_error_message;

    #[test]
    fn parses_nested_json_error_message() {
        let message = parse_upstream_error_message(
            br#"{"error":{"message":"Rate limited"}}"#,
            "Request failed",
        );

        assert_eq!(message, "Rate limited");
    }

    #[test]
    fn preserves_plain_text_error_body() {
        let message = parse_upstream_error_message(b"upstream gateway timeout", "Request failed");

        assert_eq!(message, "upstream gateway timeout");
    }

    #[test]
    fn falls_back_for_empty_error_body() {
        let message = parse_upstream_error_message(b"  ", "Request failed");

        assert_eq!(message, "Request failed");
    }
}
