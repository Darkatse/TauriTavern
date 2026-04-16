use serde_json::{Map, Value};

use crate::application::errors::ApplicationError;

use super::super::custom_api_format::CustomApiFormat;
use super::claude_messages;
use super::gemini_interactions;
use super::openai;
use super::openai_responses;
use super::shared::apply_custom_body_overrides;

pub(super) fn build(payload: Map<String, Value>) -> Result<(String, Value), ApplicationError> {
    let format = CustomApiFormat::parse(
        payload
            .get("custom_api_format")
            .and_then(Value::as_str)
            .unwrap_or_default(),
    )?;

    match format {
        CustomApiFormat::OpenAiResponses => return openai_responses::build(payload),
        CustomApiFormat::GeminiInteractions => return gemini_interactions::build(payload),
        CustomApiFormat::OpenAiCompat => {}
        CustomApiFormat::ClaudeMessages => return claude_messages::build(payload),
    }

    let include_raw = payload
        .get("custom_include_body")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let exclude_raw = payload
        .get("custom_exclude_body")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    let (endpoint, mut upstream_payload) = openai::build(payload);
    apply_custom_body_overrides(&mut upstream_payload, &include_raw, &exclude_raw)?;

    Ok((endpoint, upstream_payload))
}

#[cfg(test)]
mod tests {
    use serde_json::Value;
    use serde_json::json;

    use super::build;

    #[test]
    fn custom_payload_applies_overrides_and_strips_internal_fields() {
        let payload = json!({
            "chat_completion_source": "custom",
            "model": "gpt-4.1-mini",
            "messages": [{"role": "user", "content": "hello"}],
            "temperature": 0.1,
            "custom_include_body": "{\"temperature\":0.7,\"presence_penalty\":0.2}",
            "custom_exclude_body": "[\"messages\"]",
            "custom_include_headers": "{\"x-test\":\"1\"}",
            "custom_url": "http://localhost:1234/v1"
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (endpoint, upstream) = build(payload).expect("build should succeed");

        assert_eq!(endpoint, "/chat/completions");

        let body = upstream
            .as_object()
            .expect("upstream body should be object");
        assert_eq!(
            body.get("temperature")
                .and_then(serde_json::Value::as_f64)
                .unwrap_or_default(),
            0.7
        );
        assert_eq!(
            body.get("presence_penalty")
                .and_then(serde_json::Value::as_f64)
                .unwrap_or_default(),
            0.2
        );
        assert!(body.get("messages").is_none());
        assert!(body.get("custom_include_body").is_none());
        assert!(body.get("custom_exclude_body").is_none());
        assert!(body.get("custom_include_headers").is_none());
        assert!(body.get("custom_url").is_none());
    }

    #[test]
    fn custom_payload_supports_nested_yaml_overrides() {
        let payload = json!({
            "chat_completion_source": "custom",
            "model": "gpt-4.1-mini",
            "messages": [{"role": "user", "content": "hello"}],
            "custom_include_body": "thinking: { type: 'enabled' }\nenable_thinking: true\nchat_template_kwargs: { thinking: true }",
            "custom_url": "http://localhost:1234/v1"
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (_endpoint, upstream) = build(payload).expect("build should succeed");
        let body = upstream
            .as_object()
            .expect("upstream body should be object");

        assert_eq!(
            body.get("enable_thinking")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            true
        );
        assert_eq!(
            body.get("thinking")
                .and_then(Value::as_object)
                .and_then(|object| object.get("type"))
                .and_then(Value::as_str)
                .unwrap_or_default(),
            "enabled"
        );
        assert_eq!(
            body.get("chat_template_kwargs")
                .and_then(Value::as_object)
                .and_then(|object| object.get("thinking"))
                .and_then(Value::as_bool)
                .unwrap_or(false),
            true
        );
    }

    #[test]
    fn custom_body_overrides_can_explicitly_set_reasoning_effort() {
        let payload = json!({
            "chat_completion_source": "custom",
            "model": "gpt-5-2025-08-07",
            "messages": [{"role": "user", "content": "hello"}],
            "custom_include_body": "reasoning_effort: auto",
            "custom_url": "http://localhost:1234/v1"
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (_endpoint, upstream) = build(payload).expect("build should succeed");
        let body = upstream
            .as_object()
            .expect("upstream body should be object");

        assert_eq!(
            body.get("reasoning_effort")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            "auto"
        );
    }

    #[test]
    fn custom_payload_supports_claude_messages_format_with_overrides() {
        let payload = json!({
            "chat_completion_source": "custom",
            "custom_api_format": "claude_messages",
            "model": "claude-3-5-sonnet-latest",
            "messages": [{"role": "user", "content": "hello"}],
            "temperature": 0.1,
            "custom_include_body": "{\"max_tokens\":77}",
            "custom_exclude_body": "[\"temperature\"]",
            "custom_url": "https://api.anthropic.com/v1"
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (endpoint, upstream) = build(payload).expect("build should succeed");
        assert_eq!(endpoint, "/messages");

        let body = upstream
            .as_object()
            .expect("upstream body should be object");
        assert_eq!(
            body.get("max_tokens")
                .and_then(Value::as_i64)
                .unwrap_or_default(),
            77
        );
        assert!(body.get("temperature").is_none());
    }
}
