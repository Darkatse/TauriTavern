use serde_json::{Map, Value};

use crate::errors::ApplicationError;
use tt_ports::repositories::chat_completion_repository::ChatCompletionSource;

use super::OPENCODE_STABLE_CHAT_ID_FIELD;
use super::exchange::ChatCompletionProviderFormat;
use super::opencode::{self, OpenCodeApiFormat};

mod aws_bedrock;
mod chutes;
mod claude;
mod claude_messages;
mod cohere;
mod content_parts;
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
mod tool_choice;
mod vertexai;
mod workers_ai;
mod xai;
mod zai;

pub(super) fn build_payload(
    source: ChatCompletionSource,
    payload: Map<String, Value>,
) -> Result<(String, Value), ApplicationError> {
    let mut payload = payload;
    // Capture provider controls before wire builders discard internal fields.
    let deepseek_options = deepseek::RequestOptions::from_payload(source, &payload)?;
    let opencode_format = (source == ChatCompletionSource::OpenCode)
        .then(|| opencode::format_from_payload(&payload))
        .transpose()?;
    payload.remove(OPENCODE_STABLE_CHAT_ID_FIELD);
    if opencode_format.is_some() {
        payload.remove("opencode_endpoint");
        payload.remove("opencode_api_format");
    }

    if !matches!(source, ChatCompletionSource::DeepSeek) {
        prompt_post_processing::apply_custom_prompt_post_processing(&mut payload);
    }

    if source == ChatCompletionSource::OpenAi
        && ChatCompletionProviderFormat::from_payload(source, &payload)?
            == ChatCompletionProviderFormat::OpenAiResponses
    {
        return openai_responses::build(payload);
    }

    let (endpoint, mut body) = match source {
        ChatCompletionSource::OpenAi
        | ChatCompletionSource::Groq
        | ChatCompletionSource::SiliconFlow
        | ChatCompletionSource::Pollinations => openai::build(payload),
        ChatCompletionSource::OpenCode => {
            match opencode_format.expect("OpenCode format resolved") {
                OpenCodeApiFormat::OpenAiCompat => openai::build_chat(payload),
                OpenCodeApiFormat::OpenAiResponses => openai_responses::build(payload),
                OpenCodeApiFormat::ClaudeMessages => claude_messages::build(payload),
                OpenCodeApiFormat::Gemini => makersuite::build(payload),
            }
        }
        ChatCompletionSource::DeepSeek => deepseek::build(payload),
        ChatCompletionSource::Cohere => Ok(cohere::build(payload)?),
        ChatCompletionSource::Moonshot => moonshot::build(payload),
        ChatCompletionSource::NanoGpt => nanogpt::build(payload),
        ChatCompletionSource::Chutes => chutes::build(payload),
        ChatCompletionSource::Xai => xai::build(payload),
        ChatCompletionSource::WorkersAi => workers_ai::build(payload),
        ChatCompletionSource::OpenRouter => openrouter::build(payload),
        ChatCompletionSource::Zai => zai::build(payload),
        ChatCompletionSource::MiniMax => Ok(minimax::build(payload)),
        ChatCompletionSource::Custom => custom::build(payload),
        ChatCompletionSource::Claude => Ok(claude::build(payload)?),
        ChatCompletionSource::AwsBedrock => Ok(aws_bedrock::build(payload)?),
        ChatCompletionSource::Makersuite => Ok(makersuite::build(payload)?),
        ChatCompletionSource::VertexAi => Ok(vertexai::build(payload)?),
    }?;

    if endpoint == "/chat/completions"
        && let Some(options) = deepseek_options
    {
        options.apply(body.as_object_mut().expect("wire builders return objects"));
    }
    Ok((endpoint, body))
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
    use tt_ports::repositories::chat_completion_repository::ChatCompletionSource;

    #[test]
    fn converted_tool_call_history_replays_unusable_arguments_as_an_empty_object() {
        let error = "## Tool error\n\nRequest rejected";
        for arguments in ["null", "[1,2]", r#"{"path":"#] {
            for (format, arguments_path, error_path) in [
                ("openai_responses", "/input/1/arguments", "/input/2/output"),
                (
                    "claude_messages",
                    "/messages/1/content/0/input",
                    "/messages/2/content/0/content",
                ),
                (
                    "gemini_generate_content",
                    "/contents/1/parts/0/functionCall/args",
                    "/contents/2/parts/0/functionResponse/response/content",
                ),
                (
                    "gemini_interactions",
                    "/input/1/arguments",
                    "/input/2/result/0/text",
                ),
            ] {
                let payload = json!({
                    "chat_completion_source": "custom",
                    "custom_api_format": format,
                    "model": "test-model",
                    "tools": [{ "type": "function", "function": {
                        "name": "read_file", "parameters": { "type": "object" }
                    } }],
                    "messages": [
                        { "role": "user", "content": "Read a file" },
                        { "role": "assistant", "tool_calls": [{
                            "id": "call_1", "type": "function",
                            "function": { "name": "read_file", "arguments": arguments }
                        }] },
                        { "role": "tool", "tool_call_id": "call_1", "content": error }
                    ]
                });
                let (_, upstream) = build_payload(
                    ChatCompletionSource::Custom,
                    payload.as_object().unwrap().clone(),
                )
                .unwrap();
                let replay = upstream
                    .pointer(arguments_path)
                    .unwrap_or_else(|| panic!("{format} missing {arguments_path}: {upstream}"));
                let replay = match replay.as_str() {
                    Some(encoded) => serde_json::from_str::<Value>(encoded).unwrap(),
                    None => replay.clone(),
                };
                assert_eq!(replay, json!({}), "{format}: {arguments}");
                assert_eq!(
                    upstream.pointer(error_path),
                    Some(&json!(error)),
                    "{format}"
                );
            }
        }
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
    fn openai_gpt_6_astra_uses_responses_api() {
        let payload = json!({
            "model": "gpt-6-astra",
            "messages": [{"role": "user", "content": "hello"}],
            "temperature": 0.7
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (endpoint, upstream) =
            build_payload(ChatCompletionSource::OpenAi, payload).expect("payload should build");

        assert_eq!(endpoint, "/responses");
        assert_eq!(upstream["model"], "gpt-6-astra");
        assert_eq!(upstream["temperature"], 0.7);
    }

    #[test]
    fn opencode_selects_existing_wire_adapter_explicitly() {
        for (format, endpoint, model) in [
            (
                "openai_compat",
                "/chat/completions",
                "gpt-3.5-turbo-instruct",
            ),
            ("openai_responses", "/responses", "gateway/deepseek-v4-pro"),
            ("claude_messages", "/messages", "gateway/deepseek-v4-pro"),
            ("gemini", "/generateContent", "gateway/deepseek-v4-pro"),
        ] {
            let payload = json!({
                "chat_completion_source": "opencode",
                "opencode_endpoint": "zen",
                "opencode_api_format": format,
                "model": model,
                "messages": [{"role": "user", "content": "hello"}],
                "stream": false
            })
            .as_object()
            .cloned()
            .unwrap();

            let (actual_endpoint, upstream) =
                build_payload(ChatCompletionSource::OpenCode, payload).unwrap();
            assert_eq!(actual_endpoint, endpoint);
            assert!(upstream.get("thinking").is_none(), "{format}");
        }
    }

    #[test]
    fn custom_openai_responses_replays_native_function_call_through_payload_boundary() {
        let payload = json!({
            "chat_completion_source": "custom",
            "custom_api_format": "openai_responses",
            "model": "gateway/deepseek-v4-pro",
            "reasoning_effort": "medium",
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
        assert_eq!(upstream["reasoning"]["effort"], "medium");
        assert!(upstream.get("thinking").is_none());
        assert!(upstream.get("reasoning_effort").is_none());
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

    #[test]
    fn compat_deepseek_preserves_explicit_parameters() {
        for (source, model, effort) in [
            (
                ChatCompletionSource::Custom,
                " GO/deepseek-flash ",
                "provider-specific",
            ),
            (
                ChatCompletionSource::OpenCode,
                "OR/DeepSeek-v4.1-flash",
                "medium",
            ),
        ] {
            let payload = json!({
                "chat_completion_source": source.key(),
                "model": model,
                "messages": [{"role": "user", "content": "hello"}],
                "reasoning_effort": effort,
                "temperature": 1.2,
                "top_p": 0.7,
                "presence_penalty": 0.1,
                "frequency_penalty": 0.2
            });
            let (_, body) = build_payload(source, payload.as_object().unwrap().clone()).unwrap();

            assert_eq!(body["thinking"]["type"], "enabled", "{source:?}");
            assert_eq!(body["reasoning_effort"], effort, "{source:?}");
            assert_eq!(body["temperature"], 1.2);
            assert_eq!(body["top_p"], 0.7);
            assert_eq!(body["presence_penalty"], 0.1);
            assert_eq!(body["frequency_penalty"], 0.2);
        }
    }

    #[test]
    fn deepseek_repairs_tool_continuations_without_overwriting_reasoning() {
        for (source, reasoning) in [
            (ChatCompletionSource::DeepSeek, Value::Null),
            (
                ChatCompletionSource::Custom,
                json!("  original\nreasoning  "),
            ),
            (
                ChatCompletionSource::OpenCode,
                json!("  original\nreasoning  "),
            ),
        ] {
            let payload = json!({
                "chat_completion_source": source.key(),
                "model": "deepseek-v4-flash",
                "messages": [
                    {"role": "user", "content": "weather"},
                    {"role": "assistant", "content": "I'll check.", "reasoning_content": reasoning},
                    {"role": "user", "content": "please do"},
                    {"role": "assistant", "content": "", "tool_calls": [{
                        "id": "call_1", "type": "function",
                        "function": {"name": "weather", "arguments": "{}"}
                    }]},
                    {"role": "tool", "tool_call_id": "call_1", "content": "cloudy"}
                ],
                "tools": [{"type": "function", "function": {"name": "weather", "parameters": {
                    "type": "object", "properties": {}, "required": []
                }}}]
            });
            let (_, body) = build_payload(source, payload.as_object().unwrap().clone()).unwrap();

            assert_eq!(
                body["messages"][1].get("reasoning_content"),
                Some(&reasoning),
                "{source:?}"
            );
            assert_eq!(
                body["messages"][3].get("reasoning_content"),
                Some(&json!(""))
            );
            assert_eq!(
                body["tools"][0]["function"]["parameters"],
                json!({"type": "object", "properties": {}})
            );
            assert!(body.get("reasoning_effort").is_none());
        }
    }

    #[test]
    fn compat_deepseek_respects_disabled_thinking() {
        let payload = json!({
            "chat_completion_source": "opencode",
            "model": "deepseek-v4-pro",
            "include_reasoning": false,
            "reasoning_effort": "medium",
            "messages": [
                {"role": "user", "content": "weather"},
                {"role": "assistant", "content": "I'll check."}
            ],
            "tools": [{"type": "function", "function": {"name": "weather", "parameters": {"type": "object"}}}]
        });
        let (_, body) = build_payload(
            ChatCompletionSource::OpenCode,
            payload.as_object().unwrap().clone(),
        )
        .unwrap();

        assert_eq!(body["thinking"]["type"], "disabled");
        assert_eq!(body["reasoning_effort"], "medium");
        assert!(body["messages"][1].get("reasoning_content").is_none());
    }

    #[test]
    fn deepseek_compat_does_not_affect_other_sources_or_model_families() {
        for (source, model) in [
            (ChatCompletionSource::Custom, "deepseek-chat"),
            (ChatCompletionSource::OpenRouter, "deepseek/deepseek-v4-pro"),
        ] {
            let payload = json!({
                "chat_completion_source": source.key(),
                "model": model,
                "messages": [{"role": "user", "content": "hello"}],
                "include_reasoning": true
            });
            let (_, body) = build_payload(source, payload.as_object().unwrap().clone()).unwrap();
            assert!(body.get("thinking").is_none(), "{source:?}: {model}");
        }
    }
}
