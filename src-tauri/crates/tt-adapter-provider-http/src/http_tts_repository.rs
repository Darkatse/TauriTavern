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
    if let Some(message) = unexpected_content_type(&response, fallback_content_type) {
        return Ok(TtsRouteResponse::text(502, format!("{label}: {message}")));
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

fn unexpected_content_type(response: &Response, expected: &str) -> Option<String> {
    let actual = response_content_type(response, expected);
    let media_type = actual
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    let is_json = media_type == "application/json" || media_type.ends_with("+json");
    if matches!(media_type.as_str(), "text/html" | "application/xhtml+xml")
        || (expected.starts_with("audio/") && is_json)
    {
        Some(format!("Expected {expected}, got {actual}"))
    } else {
        None
    }
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
    use std::sync::Arc;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    use super::{HttpTtsRepository, parse_upstream_error_message};
    use tt_adapter_http::HttpClientPool;
    use tt_ports::repositories::tts_repository::{OpenAiTtsRequest, TtsRepository, TtsRequest};

    #[tokio::test]
    async fn openai_compatible_rejects_error_documents_and_preserves_audio_bytes() {
        let repository = HttpTtsRepository::new(Arc::new(HttpClientPool::new("TauriTavern/test")));
        for (content_type, body, status) in [
            ("text/html; charset=utf-8", b"<!DOCTYPE html>".to_vec(), 502),
            (
                "application/json",
                br#"{"error":"invalid voice"}"#.to_vec(),
                502,
            ),
            ("application/octet-stream", vec![0, 1, 255], 200),
        ] {
            let (url, server) = spawn_one_response(200, content_type, body.clone()).await;
            let response = repository
                .handle(TtsRequest::OpenAi(OpenAiTtsRequest::CompatibleGenerate {
                    api_key: None,
                    endpoint: url.parse().unwrap(),
                    input: "hello".into(),
                    voice: "test".into(),
                    model: "test".into(),
                    response_format: "mp3".into(),
                    speed: 1.0,
                }))
                .await
                .unwrap();
            server.await.unwrap();

            assert_eq!(response.status, status, "{content_type}");
            if status == 200 {
                assert_eq!(response.body, body);
            } else {
                assert!(
                    String::from_utf8(response.body)
                        .unwrap()
                        .contains(content_type)
                );
            }
        }
    }

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

    pub(super) async fn spawn_one_response(
        status: u16,
        content_type: &'static str,
        body: Vec<u8>,
    ) -> (String, tokio::task::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let handle = tokio::spawn(async move {
            let (mut stream, _addr) = listener.accept().await.unwrap();
            let request = read_http_request(&mut stream).await;
            let response_head = format!(
                "HTTP/1.1 {status} OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(response_head.as_bytes()).await.unwrap();
            stream.write_all(&body).await.unwrap();
            request
        });
        (url, handle)
    }

    async fn read_http_request(stream: &mut TcpStream) -> String {
        let mut bytes = Vec::new();
        let header_end = loop {
            let mut buffer = [0_u8; 1024];
            let read = stream.read(&mut buffer).await.unwrap();
            assert!(read > 0, "client closed connection before sending headers");
            bytes.extend_from_slice(&buffer[..read]);
            if let Some(index) = find_header_end(&bytes) {
                break index;
            }
        };
        let headers = String::from_utf8_lossy(&bytes[..header_end]).to_string();
        let content_length = headers
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())
                    .flatten()
            })
            .unwrap_or(0);
        let expected_len = header_end + 4 + content_length;
        while bytes.len() < expected_len {
            let mut buffer = [0_u8; 1024];
            let read = stream.read(&mut buffer).await.unwrap();
            assert!(
                read > 0,
                "client closed connection before sending full body"
            );
            bytes.extend_from_slice(&buffer[..read]);
        }
        String::from_utf8_lossy(&bytes[..expected_len]).to_string()
    }

    fn find_header_end(bytes: &[u8]) -> Option<usize> {
        bytes.windows(4).position(|window| window == b"\r\n\r\n")
    }
}
