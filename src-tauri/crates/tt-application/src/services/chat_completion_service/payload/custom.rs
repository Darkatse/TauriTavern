use serde_json::{Map, Value};

use crate::errors::ApplicationError;

use super::super::custom_api_format::CustomApiFormat;
use super::claude_messages;
use super::gemini_interactions;
use super::makersuite;
use super::openai;
use super::openai_responses;

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
        CustomApiFormat::GeminiGenerateContent => return makersuite::build(payload),
        CustomApiFormat::OpenAiCompat => {}
        CustomApiFormat::ClaudeMessages => return claude_messages::build(payload),
    }

    openai::build(payload)
}

#[cfg(test)]
mod tests {
    use serde_json::Value;
    use serde_json::json;

    use super::build;

    #[test]
    fn custom_gemini_replays_native_parts_and_applies_overrides_after_translation() {
        use crate::services::chat_completion_service::{
            AdditionalParameters, exchange::ChatCompletionProviderFormat, payload::build_payload,
        };
        use tt_ports::repositories::chat_completion_repository::ChatCompletionSource;

        let native_parts = json!([
            { "text": "Plan", "thought": true, "thoughtSignature": "thought-sig" },
            { "text": "Calling tool", "thoughtSignature": "text-sig" },
            { "functionCall": { "id": "call_1", "name": "weather", "args": {} },
              "thoughtSignature": "tool-sig", "futureField": true }
        ]);
        for stream in [false, true] {
            let payload = json!({
                "chat_completion_source": "custom",
                "custom_api_format": "gemini_generate_content",
                "custom_url": "https://example.com/v1beta",
                "model": "gemini-3-pro-preview", "stream": stream, "use_sysprompt": true,
                "messages": [
                    { "role": "system", "content": "Be helpful" },
                    { "role": "user", "content": "Weather?" },
                    { "role": "assistant", "content": "Calling tool", "signature": "canonical-sig",
                      "native": { "gemini": { "content": { "role": "model", "parts": native_parts } } },
                      "tool_calls": [{ "id": "call_1", "type": "function",
                        "function": { "name": "weather", "arguments": "{}" } }] },
                    { "role": "tool", "tool_call_id": "call_1", "content": "Sunny" }
                ],
                "custom_include_body": { "generationConfig": { "temperature": 0.4 } },
                "custom_exclude_body": ["safetySettings"]
            }).as_object().unwrap().clone();
            assert_eq!(
                ChatCompletionProviderFormat::from_payload(ChatCompletionSource::Custom, &payload)
                    .unwrap(),
                ChatCompletionProviderFormat::Gemini
            );
            let parameters = AdditionalParameters::from_payload(&payload).unwrap();
            let (endpoint, mut upstream) =
                build_payload(ChatCompletionSource::Custom, payload).unwrap();
            parameters.apply_body_overrides(&mut upstream).unwrap();
            assert_eq!(
                endpoint,
                if stream {
                    "/streamGenerateContent"
                } else {
                    "/generateContent"
                }
            );
            assert_eq!(upstream["contents"][1]["parts"], native_parts);
            assert_eq!(
                upstream["contents"][2]["parts"][0]["functionResponse"]["name"],
                "weather"
            );
            assert_eq!(
                upstream["systemInstruction"]["parts"][0]["text"],
                "Be helpful"
            );
            assert_eq!(upstream["generationConfig"]["temperature"], 0.4);
            assert!(upstream.get("safetySettings").is_none());
            assert!(upstream.get("custom_api_format").is_none());
            assert!(upstream.get("custom_url").is_none());
        }
    }

    #[test]
    fn custom_native_formats_only_relocate_reasoning_effort() {
        for (format, pointer) in [
            ("openai_responses", "/reasoning/effort"),
            ("claude_messages", "/output_config/effort"),
            ("gemini_interactions", "/generation_config/thinking_level"),
        ] {
            let payload = json!({
                "chat_completion_source": "custom",
                "custom_api_format": format,
                "model": "custom-model",
                "messages": [{"role": "user", "content": "hello"}],
                "reasoning_effort": "max"
            })
            .as_object()
            .cloned()
            .expect("payload must be object");

            let (_endpoint, upstream) = build(payload).expect("build should succeed");
            assert_eq!(upstream.pointer(pointer), Some(&json!("max")), "{format}");
        }
    }

    #[test]
    fn custom_payload_strips_internal_fields_without_applying_overrides() {
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
            0.1
        );
        assert!(body.get("presence_penalty").is_none());
        assert!(body.get("messages").is_some());
        assert!(body.get("custom_include_body").is_none());
        assert!(body.get("custom_exclude_body").is_none());
        assert!(body.get("custom_include_headers").is_none());
        assert!(body.get("custom_url").is_none());
    }

    #[test]
    fn custom_payload_leaves_nested_yaml_overrides_to_service_layer() {
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

        assert!(body.get("enable_thinking").is_none());
        assert!(body.get("thinking").is_none());
        assert!(body.get("chat_template_kwargs").is_none());
    }

    #[test]
    fn custom_body_overrides_do_not_bypass_payload_builder() {
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

        assert!(body.get("reasoning_effort").is_none());
    }

    #[test]
    fn custom_payload_supports_claude_messages_format_without_inline_overrides() {
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
        assert!(body.get("max_tokens").is_some());
        assert_eq!(body.get("temperature").and_then(Value::as_f64), Some(0.1));
    }
}
