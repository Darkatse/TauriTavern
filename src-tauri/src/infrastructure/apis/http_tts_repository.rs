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
    GrokOutputFormat, MinimaxGenerateRequest, TtsRepository, TtsRequest, TtsRouteResponse,
};
use crate::infrastructure::http_client_pool::{HttpClientPool, HttpClientProfile};

const GROK_VOICES_URL: &str = "https://api.x.ai/v1/tts/voices";
const GROK_TTS_URL: &str = "https://api.x.ai/v1/tts";
const MIMO_CHAT_COMPLETIONS_URL: &str = "https://api.xiaomimimo.com/v1/chat/completions";
const MINIMAX_TTS_SOURCE: &str = "SillyTavern-TTS";
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
            TtsRequest::MinimaxGenerate { request } => minimax_generate(client, request).await,
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

async fn minimax_generate(
    client: reqwest::Client,
    request: MinimaxGenerateRequest,
) -> Result<TtsRouteResponse, DomainError> {
    let MinimaxGenerateRequest {
        api_key,
        group_id,
        text,
        voice_id,
        api_host,
        model,
        speed,
        volume,
        pitch,
        audio_sample_rate,
        bitrate,
        format,
        language,
    } = request;
    let audio_content_type = minimax_content_type(&format);
    let mut payload = json!({
        "model": model,
        "text": text,
        "stream": false,
        "voice_setting": {
            "voice_id": voice_id,
            "speed": speed,
            "vol": volume,
            "pitch": pitch,
        },
        "audio_setting": {
            "sample_rate": audio_sample_rate,
            "bitrate": bitrate,
            "format": format,
            "channel": 1,
        },
    });

    if let Some(language) = language {
        payload["lang"] = Value::String(language);
    }

    let url = format!("{}/v1/t2a_v2", api_host.trim().trim_end_matches('/'));
    let response = send_with_retry("MiniMax TTS request", || {
        client
            .post(&url)
            .query(&[("GroupId", group_id.as_str())])
            .bearer_auth(&api_key)
            .header(ACCEPT, "application/json")
            .header(CONTENT_TYPE, "application/json")
            .header("MM-API-Source", MINIMAX_TTS_SOURCE)
            .json(&payload)
    })
    .await?;

    if !response.status().is_success() {
        return minimax_upstream_error_response(response, "MiniMax TTS request failed").await;
    }

    let bytes = response.bytes().await.map_err(|error| {
        DomainError::InternalError(format!("MiniMax TTS response read failed: {error}"))
    })?;

    let payload: Value = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(error) => {
            return Ok(minimax_error_response(
                502,
                format!("MiniMax TTS response is not valid JSON: {error}"),
            ));
        }
    };

    if let Some(message) = parse_minimax_base_resp_error(&payload) {
        return Ok(minimax_error_response(502, message));
    }

    if let Some(hex_audio) = payload
        .get("data")
        .and_then(|value| value.get("audio"))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
    {
        let audio = match decode_hex_audio(hex_audio) {
            Ok(audio) => audio,
            Err(message) => return Ok(minimax_error_response(502, message)),
        };
        return Ok(TtsRouteResponse::bytes(200, audio_content_type, audio));
    }

    if let Some(audio_url) = payload
        .get("data")
        .and_then(|value| value.get("url"))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
    {
        let audio_response = send_with_retry("MiniMax TTS audio URL request", || {
            client.get(audio_url).header(ACCEPT, "*/*")
        })
        .await?;

        if !audio_response.status().is_success() {
            return minimax_upstream_error_response(
                audio_response,
                "MiniMax TTS audio URL request failed",
            )
            .await;
        }

        let audio = audio_response.bytes().await.map_err(|error| {
            DomainError::InternalError(format!(
                "MiniMax TTS audio URL response read failed: {error}"
            ))
        })?;
        return Ok(TtsRouteResponse::bytes(
            200,
            audio_content_type,
            audio.to_vec(),
        ));
    }

    Ok(minimax_error_response(
        502,
        parse_minimax_json_error_message(&payload)
            .unwrap_or_else(|| "MiniMax TTS response did not include audio data".to_string()),
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

async fn minimax_upstream_error_response(
    response: Response,
    fallback: &str,
) -> Result<TtsRouteResponse, DomainError> {
    let status = response.status().as_u16();
    let bytes = response.bytes().await.map_err(|error| {
        DomainError::InternalError(format!(
            "MiniMax upstream error response read failed: {error}"
        ))
    })?;
    let message = parse_minimax_upstream_error_message(&bytes, fallback);
    Ok(minimax_error_response(status, message))
}

fn minimax_error_response(status: u16, message: impl Into<String>) -> TtsRouteResponse {
    TtsRouteResponse::json_error(status, message)
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

fn parse_minimax_upstream_error_message(body: &[u8], fallback: &str) -> String {
    if let Ok(payload) = serde_json::from_slice::<Value>(body) {
        if let Some(message) = parse_minimax_json_error_message(&payload) {
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

    for key in ["error", "message", "detail"] {
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

fn minimax_content_type(format: &str) -> &'static str {
    match format.trim().to_ascii_lowercase().as_str() {
        "wav" => "audio/wav",
        "pcm" => "audio/pcm",
        "flac" => "audio/flac",
        "aac" => "audio/aac",
        _ => "audio/mpeg",
    }
}

fn parse_minimax_base_resp_error(payload: &Value) -> Option<String> {
    let base_resp = payload.get("base_resp")?;
    let status_code = base_resp.get("status_code").and_then(Value::as_i64)?;
    if status_code == 0 {
        return None;
    }

    let status_msg = base_resp
        .get("status_msg")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("Unknown error");

    if status_code == 1004 {
        Some("Authentication failed - Please check your API key and API host".to_string())
    } else {
        Some(format!("API Error: {status_msg}"))
    }
}

fn parse_minimax_json_error_message(payload: &Value) -> Option<String> {
    parse_minimax_base_resp_error(payload).or_else(|| parse_json_error_message(payload))
}

fn decode_hex_audio(value: &str) -> Result<Vec<u8>, String> {
    let mut clean = value.trim();
    if let Some(stripped) = clean
        .strip_prefix("0x")
        .or_else(|| clean.strip_prefix("0X"))
    {
        clean = stripped;
    }

    let compact = clean
        .bytes()
        .filter(|byte| !byte.is_ascii_whitespace())
        .collect::<Vec<_>>();

    if compact.is_empty() {
        return Err("MiniMax TTS audio data is empty".to_string());
    }

    let mut audio = Vec::with_capacity(compact.len().div_ceil(2));
    let mut index = 0;
    if compact.len() % 2 != 0 {
        audio.push(hex_nibble(compact[0])?);
        index = 1;
    }

    while index < compact.len() {
        let high = hex_nibble(compact[index])?;
        let low = hex_nibble(compact[index + 1])?;
        audio.push((high << 4) | low);
        index += 2;
    }

    Ok(audio)
}

fn hex_nibble(byte: u8) -> Result<u8, String> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err("MiniMax TTS audio data is not valid hex".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    use super::{
        decode_hex_audio, minimax_generate, parse_minimax_base_resp_error,
        parse_minimax_upstream_error_message, parse_upstream_error_message,
    };
    use crate::domain::repositories::tts_repository::MinimaxGenerateRequest;

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

    #[test]
    fn parses_minimax_base_resp_error() {
        let message = parse_minimax_base_resp_error(&json!({
            "base_resp": {
                "status_code": 2001,
                "status_msg": "invalid voice"
            }
        }));

        assert_eq!(message.as_deref(), Some("API Error: invalid voice"));
    }

    #[test]
    fn parses_minimax_upstream_error_without_generic_provider_leakage() {
        let message = parse_minimax_upstream_error_message(
            br#"{"base_resp":{"status_code":2001,"status_msg":"\u97f3\u8272\u4e0d\u5b58\u5728"}}"#,
            "MiniMax TTS request failed",
        );

        assert_eq!(message, "API Error: 音色不存在");
    }

    #[test]
    fn decodes_minimax_hex_audio() {
        let audio = decode_hex_audio("0x0001ff").unwrap();

        assert_eq!(audio, vec![0, 1, 255]);
    }

    #[test]
    fn rejects_invalid_minimax_hex_audio_as_upstream_payload_error() {
        assert_eq!(
            decode_hex_audio("not-hex").unwrap_err(),
            "MiniMax TTS audio data is not valid hex"
        );
    }

    #[tokio::test]
    async fn minimax_generate_sends_expected_payload_and_decodes_hex_audio() {
        let body = br#"{"data":{"audio":"0x0001ff"},"base_resp":{"status_code":0,"status_msg":"success"}}"#.to_vec();
        let (api_host, server) = spawn_one_response(200, "application/json", body).await;

        let response = minimax_generate(
            reqwest::Client::new(),
            minimax_request(api_host, Some("English".to_string())),
        )
        .await
        .unwrap();
        let request = server.await.unwrap();
        let request_body = request_body_json(&request);

        assert_eq!(response.status, 200);
        assert_eq!(response.content_type, "audio/mpeg");
        assert_eq!(response.body, vec![0, 1, 255]);
        assert!(request.starts_with("POST /v1/t2a_v2?GroupId=group-id HTTP/1.1"));
        assert!(
            request
                .to_ascii_lowercase()
                .contains("authorization: bearer api-key")
        );
        assert!(
            request
                .to_ascii_lowercase()
                .contains("mm-api-source: sillytavern-tts")
        );
        assert_eq!(request_body["model"], "speech-02-hd");
        assert_eq!(request_body["text"], "Hello MiniMax");
        assert_eq!(request_body["stream"], false);
        assert_eq!(request_body["voice_setting"]["voice_id"], "voice-id");
        assert_eq!(request_body["voice_setting"]["speed"], 1.2);
        assert_eq!(request_body["voice_setting"]["vol"], 0.8);
        assert_eq!(request_body["voice_setting"]["pitch"], -2.0);
        assert_eq!(request_body["audio_setting"]["sample_rate"], 32_000);
        assert_eq!(request_body["audio_setting"]["bitrate"], 128_000);
        assert_eq!(request_body["audio_setting"]["format"], "mp3");
        assert_eq!(request_body["audio_setting"]["channel"], 1);
        assert_eq!(request_body["lang"], "English");
    }

    #[tokio::test]
    async fn minimax_generate_fetches_audio_url_response() {
        let (audio_url, audio_server) =
            spawn_one_response(200, "audio/mpeg", vec![10, 11, 12]).await;
        let body = format!(
            r#"{{"data":{{"url":"{audio_url}/audio.mp3"}},"base_resp":{{"status_code":0}}}}"#
        )
        .into_bytes();
        let (api_host, api_server) = spawn_one_response(200, "application/json", body).await;

        let response = minimax_generate(reqwest::Client::new(), minimax_request(api_host, None))
            .await
            .unwrap();
        let api_request = api_server.await.unwrap();
        let audio_request = audio_server.await.unwrap();

        assert_eq!(response.status, 200);
        assert_eq!(response.content_type, "audio/mpeg");
        assert_eq!(response.body, vec![10, 11, 12]);
        assert!(api_request.starts_with("POST /v1/t2a_v2?GroupId=group-id HTTP/1.1"));
        assert!(audio_request.starts_with("GET /audio.mp3 HTTP/1.1"));
    }

    #[tokio::test]
    async fn minimax_generate_returns_json_error_for_base_resp_failure() {
        let body =
            br#"{"base_resp":{"status_code":2001,"status_msg":"\u97f3\u8272\u4e0d\u5b58\u5728"}}"#
                .to_vec();
        let (api_host, server) = spawn_one_response(200, "application/json", body).await;

        let response = minimax_generate(reqwest::Client::new(), minimax_request(api_host, None))
            .await
            .unwrap();
        let _request = server.await.unwrap();

        assert_eq!(response.status, 502);
        assert_eq!(response.content_type, "application/json; charset=utf-8");
        assert_eq!(response.status_text, None);
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&response.body).unwrap(),
            json!({ "error": "API Error: 音色不存在" })
        );
    }

    fn minimax_request(api_host: String, language: Option<String>) -> MinimaxGenerateRequest {
        MinimaxGenerateRequest {
            api_key: "api-key".to_string(),
            group_id: "group-id".to_string(),
            text: "Hello MiniMax".to_string(),
            voice_id: "voice-id".to_string(),
            api_host,
            model: "speech-02-hd".to_string(),
            speed: 1.2,
            volume: 0.8,
            pitch: -2.0,
            audio_sample_rate: 32_000,
            bitrate: 128_000,
            format: "mp3".to_string(),
            language,
        }
    }

    async fn spawn_one_response(
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

    fn request_body_json(request: &str) -> serde_json::Value {
        let body = request
            .split_once("\r\n\r\n")
            .map(|(_headers, body)| body)
            .expect("request body separator should be present");
        serde_json::from_str(body).unwrap()
    }
}
