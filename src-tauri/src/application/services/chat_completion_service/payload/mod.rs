use serde_json::{Map, Value};

use crate::application::errors::ApplicationError;
use crate::domain::repositories::chat_completion_repository::ChatCompletionSource;

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
        ChatCompletionSource::Makersuite => Ok(makersuite::build(payload)?),
        ChatCompletionSource::VertexAi => Ok(vertexai::build(payload)?),
    };

    result
}

#[cfg(test)]
mod tests {
    use serde_json::json;

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
}
