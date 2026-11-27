use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use serde_json::{Value, json};
use tokio::sync::{mpsc, watch};
use tt_adapter_http::HttpClientPool;
use tt_contracts::observability::{
    REDACTED, RequestDescriptionDto, RequestHeaderDto, RequestTransportDto,
};
use tt_ports::repositories::chat_completion_repository::{
    AnthropicBetaHeaderMode, ChatCompletionApiConfig, ChatCompletionRepository,
    ChatCompletionSource, GenerationCall,
};

use super::HttpChatCompletionRepository;
use super::test_upstream::upstream;

const API_KEY: &str = "sk-test-key";
const USER_AGENT: &str = "TauriTavern/test";

fn repository() -> HttpChatCompletionRepository {
    HttpChatCompletionRepository::new(Arc::new(HttpClientPool::new(USER_AGENT)))
}

fn config(base_url: String) -> ChatCompletionApiConfig {
    ChatCompletionApiConfig {
        base_url,
        user_configured_endpoint: false,
        api_key: API_KEY.to_string(),
        authorization_header: None,
        vertexai_service_account_json: None,
        extra_headers: HashMap::new(),
        additional_headers: HashMap::new(),
        anthropic_beta_header_mode: AnthropicBetaHeaderMode::None,
        aws_bedrock_custom_response_path: None,
        aws_bedrock_custom_stream_path: None,
    }
}

fn headers(description: &RequestDescriptionDto) -> BTreeMap<&str, &str> {
    description
        .headers
        .iter()
        .map(|header| (header.name.as_str(), header.value.as_str()))
        .collect()
}

#[tokio::test]
async fn described_request_is_the_sent_one_with_credentials_replaced() {
    let cached = json!({
        "model": "claude-test",
        "messages": [{ "role": "user", "content": [
            { "type": "text", "text": "Hi", "cache_control": { "type": "ephemeral" } }
        ] }]
    });
    let chat = json!({ "model": "gpt-test", "messages": [{ "role": "user", "content": "Hi" }] });
    let gemini = json!({ "model": "gemini-test", "contents": [] });

    type Case<'a> = (
        ChatCompletionSource,
        &'a str,
        &'a Value,
        GenerationCall,
        &'a [(&'a str, &'a str)],
        &'a [&'a str],
    );
    let cases: [Case<'_>; 3] = [
        (
            ChatCompletionSource::OpenAi,
            "/chat/completions",
            &chat,
            GenerationCall::GenerateStream,
            &[("HTTP-Referer", "https://example.test")],
            &["authorization"],
        ),
        (
            ChatCompletionSource::Claude,
            "/messages",
            &cached,
            GenerationCall::Generate,
            &[("anthropic-beta", "custom-beta")],
            &["x-api-key"],
        ),
        (
            ChatCompletionSource::Makersuite,
            "/streamGenerateContent",
            &gemini,
            GenerationCall::GenerateStream,
            &[],
            &["x-goog-api-key"],
        ),
    ];

    for (source, endpoint_path, payload, call, extra_headers, credential_headers) in cases {
        let (base, server) = upstream(vec!["{}".to_string()]).await;
        let mut config = config(format!("{base}/v1"));
        config.extra_headers = extra_headers
            .iter()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect();
        let repository = repository();

        let described = repository
            .describe_request(source, &config, endpoint_path, payload, call)
            .unwrap();
        // Ignore parsing errors from the `{}` response; assert the captured request.
        if call == GenerationCall::Generate {
            let _ = repository
                .generate(source, &config, endpoint_path, payload)
                .await;
        } else {
            let (sender, _receiver) = mpsc::unbounded_channel();
            let (_cancel_sender, cancel) = watch::channel(false);
            let _ = repository
                .generate_stream(source, &config, endpoint_path, payload, sender, cancel)
                .await;
        }

        let request = server.await.unwrap().remove(0);
        let (head, body) = request.split_once("\r\n\r\n").unwrap();
        let mut lines = head.lines();
        let mut request_line = lines.next().unwrap().split_whitespace();
        let (method, target) = (request_line.next().unwrap(), request_line.next().unwrap());
        let mut sent = lines
            .map(|line| line.split_once(": ").unwrap())
            .collect::<BTreeMap<_, _>>();

        assert_eq!(described.http.transport, RequestTransportDto::Http);
        assert_eq!(described.http.method, method);
        assert_eq!(
            described.http.url,
            format!("{base}{target}").replace(API_KEY, REDACTED),
            "{source:?}"
        );
        assert_eq!(
            *described.body,
            serde_json::from_str::<Value>(body).unwrap(),
            "{source:?}"
        );
        assert!(sent.remove("host").is_some() && sent.remove("content-length").is_some());
        for header in credential_headers {
            assert!(
                sent.insert(header, REDACTED).unwrap().contains(API_KEY),
                "{source:?}"
            );
        }
        assert_eq!(headers(&described.http), sent, "{source:?}");
    }
}

#[test]
fn description_replaces_credentials_by_name_and_by_value_and_keeps_other_headers() {
    let mut config = config("https://generativelanguage.googleapis.com".to_string());
    config.extra_headers = HashMap::from([("x-goog-api-key".to_string(), "moved".to_string())]);
    config.additional_headers = HashMap::from([
        ("X-Relay".to_string(), API_KEY.to_string()),
        ("X-Trace".to_string(), "keep-me".to_string()),
    ]);

    let payload = json!({ "model": "gemini-test", "contents": [] });
    let mut description = repository()
        .describe_request(
            ChatCompletionSource::Makersuite,
            &config,
            "/streamGenerateContent",
            &payload,
            GenerationCall::GenerateStream,
        )
        .unwrap()
        .http;
    description
        .headers
        .sort_by(|left, right| left.name.cmp(&right.name));

    let header = |name: &str, value: &str| RequestHeaderDto {
        name: name.to_string(),
        value: value.to_string(),
    };
    assert_eq!(
        description,
        RequestDescriptionDto {
            transport: RequestTransportDto::Http,
            method: "POST".to_string(),
            url: format!(
                "https://generativelanguage.googleapis.com/v1beta/models/gemini-test:streamGenerateContent?key={REDACTED}&alt=sse"
            ),
            headers: vec![
                header("accept", "text/event-stream"),
                header("content-type", "application/json"),
                header("user-agent", USER_AGENT),
                header("x-goog-api-key", REDACTED),
                header("x-relay", REDACTED),
                header("x-trace", "keep-me"),
            ],
        }
    );
}
