use serde_json::{Map, Value};

use crate::application::errors::ApplicationError;
use crate::domain::repositories::chat_completion_repository::ChatCompletionSource;

mod aws_bedrock;
mod chutes;
mod claude;
mod claude_messages;
mod cohere;
mod custom;
mod deepseek;
mod gemini_interactions;
mod makersuite;
mod minimax;
mod moonshot;
mod nanogpt;
mod openai;
mod openai_reasoning;
mod openai_responses;
mod openrouter;
mod prompt_post_processing;
mod shared;
mod tool_calls;
mod vertexai;
mod workers_ai;
mod zai;

pub(super) fn build_payload(
    source: ChatCompletionSource,
    payload: Map<String, Value>,
) -> Result<(String, Value), ApplicationError> {
    let mut payload = payload;

    if !matches!(source, ChatCompletionSource::DeepSeek) {
        prompt_post_processing::apply_custom_prompt_post_processing(&mut payload);
    }

    let result = match source {
        ChatCompletionSource::OpenAi
        | ChatCompletionSource::Groq
        | ChatCompletionSource::SiliconFlow => Ok(openai::build(payload)),
        ChatCompletionSource::DeepSeek => deepseek::build(payload),
        ChatCompletionSource::Cohere => Ok(cohere::build(payload)?),
        ChatCompletionSource::Moonshot => Ok(moonshot::build(payload)),
        ChatCompletionSource::NanoGpt => nanogpt::build(payload),
        ChatCompletionSource::Chutes => chutes::build(payload),
        ChatCompletionSource::WorkersAi => workers_ai::build(payload),
        ChatCompletionSource::OpenRouter => Ok(openrouter::build(payload)),
        ChatCompletionSource::Zai => Ok(zai::build(payload)),
        ChatCompletionSource::MiniMax => Ok(minimax::build(payload)),
        ChatCompletionSource::Custom => custom::build(payload),
        ChatCompletionSource::Claude => Ok(claude::build(payload)?),
        ChatCompletionSource::AwsBedrock => Ok(aws_bedrock::build(payload)?),
        ChatCompletionSource::Makersuite => Ok(makersuite::build(payload)?),
        ChatCompletionSource::VertexAi => Ok(vertexai::build(payload)?),
    };

    result
}

pub(super) fn validate_upstream_tool_transcript(
    endpoint_path: &str,
    upstream_payload: &Value,
) -> Result<(), ApplicationError> {
    if endpoint_path != "/chat/completions" {
        return Ok(());
    }

    tool_calls::validate_openai_chat_tool_transcript(upstream_payload.get("messages"), false)
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::build_payload;
    use crate::domain::repositories::chat_completion_repository::ChatCompletionSource;

    #[test]
    fn deepseek_leaves_additional_body_overrides_to_service_layer() {
        let payload = json!({
            "chat_completion_source": "deepseek",
            "model": "deepseek-v4-flash",
            "messages": [{"role": "user", "content": "hello"}],
            "custom_include_body": "{\"x_extra\":true}",
            "custom_exclude_body": "[\"model\"]"
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (_, upstream) =
            build_payload(ChatCompletionSource::DeepSeek, payload).expect("payload should build");
        let body = upstream.as_object().expect("body must be object");

        assert!(body.get("x_extra").is_none());
        assert_eq!(
            body.get("model").and_then(serde_json::Value::as_str),
            Some("deepseek-v4-flash")
        );
    }

    #[test]
    fn claude_leaves_additional_body_overrides_to_service_layer() {
        let payload = json!({
            "chat_completion_source": "claude",
            "model": "claude-sonnet-4-5",
            "messages": [{"role": "user", "content": "hello"}],
            "stream": true,
            "custom_include_body": "{\"metadata\":{\"feature\":\"override\"}}",
            "custom_exclude_body": "[\"stream\"]"
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (_, upstream) =
            build_payload(ChatCompletionSource::Claude, payload).expect("payload should build");
        let body = upstream.as_object().expect("body must be object");

        assert!(body.get("metadata").is_none());
        assert_eq!(
            body.get("stream").and_then(serde_json::Value::as_bool),
            Some(true)
        );
    }

    #[test]
    fn custom_openai_responses_replays_native_function_call_through_payload_boundary() {
        let payload = json!({
            "chat_completion_source": "custom",
            "custom_api_format": "openai_responses",
            "model": "gpt-5",
            "messages": [
                { "role": "user", "content": "hi" },
                {
                    "role": "assistant",
                    "content": "",
                    "native": {
                        "openai_responses": {
                            "responseId": "resp_1",
                            "output": [{
                                "id": "fc_1",
                                "type": "function_call",
                                "call_id": "call_1",
                                "name": "workspace_write_file",
                                "arguments": "{\"path\":\"output/main.md\",\"content\":\"hi\"}"
                            }]
                        }
                    }
                },
                { "role": "tool", "tool_call_id": "call_1", "content": "ok" }
            ]
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (endpoint, upstream) =
            build_payload(ChatCompletionSource::Custom, payload).expect("payload should build");

        assert_eq!(endpoint, "/responses");
        let input = upstream
            .get("input")
            .and_then(Value::as_array)
            .expect("responses input should exist");
        assert_eq!(input[1]["type"], "function_call");
        assert_eq!(input[1]["call_id"], "call_1");
        assert_eq!(input[2]["type"], "function_call_output");
        assert_eq!(input[2]["call_id"], "call_1");
    }

    #[test]
    fn custom_openai_responses_rejects_orphan_tool_output_through_payload_boundary() {
        let payload = json!({
            "chat_completion_source": "custom",
            "custom_api_format": "openai_responses",
            "model": "gpt-5",
            "messages": [
                { "role": "user", "content": "hi" },
                { "role": "tool", "tool_call_id": "call_1", "content": "orphan" }
            ]
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let error = build_payload(ChatCompletionSource::Custom, payload)
            .expect_err("orphan tool output must fail");

        assert!(
            error
                .to_string()
                .contains("without preceding function_call")
        );
    }
}
