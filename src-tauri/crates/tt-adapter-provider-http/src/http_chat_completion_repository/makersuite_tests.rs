use std::sync::Arc;

use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::{mpsc, watch};
use tt_adapter_http::HttpClientPool;
use tt_ports::repositories::chat_completion_repository::{
    AnthropicBetaHeaderMode, ChatCompletionApiConfig, ChatCompletionRepository, ChatCompletionSource,
};
use tt_ports::user_endpoint_access::UserEndpointGrantRuntime;

use super::HttpChatCompletionRepository;

async fn upstream(body: String) -> (String, tokio::task::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        let mut chunk = [0; 4096];
        loop {
            let count = socket.read(&mut chunk).await.unwrap();
            assert_ne!(count, 0);
            request.extend_from_slice(&chunk[..count]);
            if let Some(end) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&request[..end]).to_lowercase();
                let length = headers
                    .lines()
                    .find_map(|line| {
                        line.strip_prefix("content-length: ")
                            .map(|value| value.parse::<usize>().unwrap())
                    })
                    .unwrap_or(0);
                if request.len() >= end + 4 + length {
                    break;
                }
            }
        }
        socket.write_all(format!(
            "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            if body.starts_with("data:") { "text/event-stream" } else { "application/json" },
            body.len(),
        ).as_bytes()).await.unwrap();
        String::from_utf8(request).unwrap()
    });
    (base, task)
}

fn repository(base_url: String) -> (HttpChatCompletionRepository, ChatCompletionApiConfig) {
    let base_url = reqwest::Url::parse(&base_url).unwrap().to_string();
    let pool = Arc::new(HttpClientPool::new("TauriTavern/test"));
    let grant = tt_domain::models::endpoint_url::parse_user_http_endpoint(&base_url)
        .unwrap()
        .to_string();
    pool.replace_user_endpoint_grants(&[grant]);
    let config = ChatCompletionApiConfig {
        base_url,
        user_configured_endpoint: true,
        api_key: "custom-key".into(),
        authorization_header: None,
        vertexai_service_account_json: None,
        extra_headers: Default::default(),
        additional_headers: [("x-goog-api-key".into(), "header-override".into())].into(),
        anthropic_beta_header_mode: AnthropicBetaHeaderMode::None,
        aws_bedrock_custom_response_path: None,
        aws_bedrock_custom_stream_path: None,
    };
    (HttpChatCompletionRepository::new(pool), config)
}

#[tokio::test]
async fn custom_gemini_generate_content_http_contract() {
    let parts = json!([
        { "text": "Plan", "thought": true, "thoughtSignature": "reasoning-sig" },
        { "text": "Hello", "thoughtSignature": "text-sig" },
        { "functionCall": { "id": "call_1", "name": "weather", "args": { "city": "Paris" } },
          "thoughtSignature": "tool-sig" }
    ]);
    let response = json!({ "candidates": [{ "content": { "role": "model", "parts": parts }, "finishReason": "STOP" }] });
    let events = [
        json!({ "candidates": [{ "content": { "role": "model", "parts": [parts[0]] } }] }),
        json!({ "candidates": [{ "content": { "parts": [{ "text": "Hel" }] } }] }),
        json!({ "candidates": [{ "content": { "parts": [{ "text": "lo", "thoughtSignature": "text-sig" }, parts[2]] } }] }),
        json!({ "candidates": [{ "finishReason": "STOP" }] }),
    ];
    for stream in [false, true] {
        let body = if !stream {
            response.to_string()
        } else {
            events
                .iter()
                .map(|event| format!("data: {event}\n\n"))
                .collect()
        };
        let (base, server) = upstream(body).await;
        let (repository, config) = repository(format!("{base}/proxy/v1/"));
        let payload = json!({ "model": "models/gemini-test", "contents": [{ "role": "user", "parts": [{ "text": "Hi" }] }] });
        let endpoint = if !stream {
            "/generateContent"
        } else {
            "/streamGenerateContent"
        };
        let source = ChatCompletionSource::Custom;
        let native = if stream {
            let (sender, mut receiver) = mpsc::unbounded_channel();
            let (_cancel, cancel) = watch::channel(false);
            repository
                .generate_stream(source, &config, endpoint, &payload, sender, cancel)
                .await
                .unwrap();
            for event in &events {
                let actual: Value = serde_json::from_str(&receiver.recv().await.unwrap()).unwrap();
                assert_eq!(&actual, event);
            }
            let terminal: Value = serde_json::from_str(&receiver.recv().await.unwrap()).unwrap();
            assert!(receiver.recv().await.is_none());
            terminal["choices"][0]["delta"]["native"].clone()
        } else {
            let result = repository
                .generate(source, &config, endpoint, &payload)
                .await
                .unwrap();
            let message = &result.body["choices"][0]["message"];
            assert_eq!(message["content"], "Hello");
            assert_eq!(message["reasoning_content"], "Plan");
            assert_eq!(message["tool_calls"][0]["function"]["name"], "weather");
            message["native"].clone()
        };
        assert_eq!(native["gemini"]["content"]["parts"], parts);
        let request = server.await.unwrap();
        let (headers, body) = request.split_once("\r\n\r\n").unwrap();
        let target = headers
            .lines()
            .next()
            .unwrap()
            .split_whitespace()
            .nth(1)
            .unwrap();
        let url = reqwest::Url::parse(&format!("{base}{target}")).unwrap();
        assert_eq!(
            url.path(),
            format!(
                "/proxy/v1/models/gemini-test:{}",
                endpoint.trim_start_matches('/')
            )
        );
        let query: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(query.get("key").map(String::as_str), Some("custom-key"));
        assert_eq!(query.get("alt").map(String::as_str), stream.then_some("sse"));
        assert!(headers.contains("x-goog-api-key: header-override"));
        assert!(!headers.to_lowercase().contains("authorization:"));
        let body: Value = serde_json::from_str(body).unwrap();
        assert!(body.get("model").is_none());
        assert_eq!(body["contents"], payload["contents"]);
    }
}

#[tokio::test]
async fn custom_gemini_stream_rejects_incomplete_or_error_events_without_native_commit() {
    for (event, expected) in [
        (
            json!({ "candidates": [{ "content": { "parts": [{ "text": "partial" }] } }] }),
            "without a finish reason",
        ),
        (
            json!({ "error": { "message": "stream rejected" } }),
            "stream rejected",
        ),
    ] {
        let (base, server) = upstream(format!("data: {event}\n\n")).await;
        let (repository, config) = repository(base);
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let (_cancel, cancel) = watch::channel(false);
        let error = repository
            .generate_stream(
                ChatCompletionSource::Custom,
                &config,
                "/streamGenerateContent",
                &json!({ "model": "gemini-test", "contents": [] }),
                sender,
                cancel,
            )
            .await
            .unwrap_err();
        assert!(error.to_string().contains(expected), "{error}");
        while let Some(chunk) = receiver.recv().await {
            let chunk: Value = serde_json::from_str(&chunk).unwrap();
            assert!(chunk.pointer("/choices/0/delta/native").is_none());
        }
        server.await.unwrap();
    }
}
