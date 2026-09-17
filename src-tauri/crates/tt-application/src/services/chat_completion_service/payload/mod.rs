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

    match source {
        ChatCompletionSource::OpenAi
        | ChatCompletionSource::Groq
        | ChatCompletionSource::SiliconFlow
        | ChatCompletionSource::Pollinations => openai::build(payload),
        ChatCompletionSource::OpenCode => {
            let include_reasoning = payload
                .get("include_reasoning")
                .and_then(Value::as_bool);
            let built = match opencode_format.expect("OpenCode format resolved") {
                OpenCodeApiFormat::OpenAiCompat => openai::build_chat(payload),
                OpenCodeApiFormat::OpenAiResponses => openai_responses::build(payload),
                OpenCodeApiFormat::ClaudeMessages => claude_messages::build(payload),
                OpenCodeApiFormat::Gemini => makersuite::build(payload),
            }?;
            apply_deepseek_v4_compat_if_chat(include_reasoning, built)
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
        ChatCompletionSource::Custom => {
            // builder 白名单会丢弃 include_reasoning，先读出思考开关意图。
            let include_reasoning = payload
                .get("include_reasoning")
                .and_then(Value::as_bool);
            let built = custom::build(payload)?;
            apply_deepseek_v4_compat_if_chat(include_reasoning, built)
        }
        ChatCompletionSource::Claude => Ok(claude::build(payload)?),
        ChatCompletionSource::AwsBedrock => Ok(aws_bedrock::build(payload)?),
        ChatCompletionSource::Makersuite => Ok(makersuite::build(payload)?),
        ChatCompletionSource::VertexAi => Ok(vertexai::build(payload)?),
    }
}

/// OpenAI 兼容源（Custom/OpenCode）经 OpenAI 兼容上游访问 DeepSeek V4 时，
/// 对齐官方 DeepSeek 渠道语义（thinking 控制、effort 归一化等）。
/// 仅处理 /chat/completions 端点；模型不命中时零改动。
fn apply_deepseek_v4_compat_if_chat(
    include_reasoning: Option<bool>,
    built: (String, Value),
) -> Result<(String, Value), ApplicationError> {
    let (endpoint, mut upstream_payload) = built;
    if endpoint == "/chat/completions" {
        deepseek::apply_deepseek_v4_compat(&mut upstream_payload, include_reasoning)?;
    }
    Ok((endpoint, upstream_payload))
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
    use serde_json::{Map, Value, json};

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
            ("openai_responses", "/responses", "test-model"),
            ("claude_messages", "/messages", "test-model"),
            ("gemini", "/generateContent", "test-model"),
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

            assert_eq!(
                build_payload(ChatCompletionSource::OpenCode, payload)
                    .unwrap()
                    .0,
                endpoint
            );
        }
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

    /// 兼容源（Custom/OpenCode OpenAI 兼容格式）命中 DeepSeek V4 模型时，
    /// 对齐官方 DeepSeek 渠道语义：显式 thinking 控制、effort 归一化、
    /// 关闭思考时保留采样参数、开启思考时清理采样参数、
    /// 工具上下文补 reasoning_content、剥离空 required。
    fn deepseek_v4_compat_payload(model: &str) -> Map<String, Value> {
        json!({
            "chat_completion_source": "custom",
            "custom_api_format": "openai_compat",
            "opencode_endpoint": "zen",
            "opencode_api_format": "openai_compat",
            "model": model,
            "messages": [
                {"role":"user","content":"weather"},
                {"role":"assistant","content":"I'll check."},
                {
                    "role":"assistant",
                    "content":"",
                    "tool_calls":[{
                        "id":"call_1",
                        "type":"function",
                        "function":{"name":"weather","arguments":"{}"}
                    }]
                },
                {"role":"tool","tool_call_id":"call_1","content":"cloudy"}
            ],
            "tools": [{"type":"function","function":{"name":"weather","parameters":{
                "type":"object","properties":{"city":{"type":"string"}},"required":[]
            }}}],
            "temperature": 1.2,
            "top_p": 0.7,
            "presence_penalty": 0.1,
            "frequency_penalty": 0.2,
            "reasoning_effort": "medium"
        })
        .as_object()
        .cloned()
        .expect("payload must be object")
    }

    #[test]
    fn compat_sources_align_deepseek_v4_thinking_when_show_thoughts_on() {
        for source in [
            ChatCompletionSource::Custom,
            ChatCompletionSource::OpenCode,
        ] {
            for model in [
                "GO/deepseek-flash",
                "OR/deepseek-v4.1-flash",
                "deepseek-v4-pro",
            ] {
                let (endpoint, upstream) = build_payload(
                    source,
                    deepseek_v4_compat_payload(model),
                )
                .unwrap_or_else(|error| panic!("{source:?} {model}: {error}"));
                assert_eq!(endpoint, "/chat/completions");

                let body = upstream.as_object().expect("body must be object");
                assert_eq!(
                    body.get("thinking")
                        .and_then(Value::as_object)
                        .and_then(|thinking| thinking.get("type"))
                        .and_then(Value::as_str),
                    Some("enabled"),
                    "{source:?} {model}: thinking"
                );
                assert_eq!(
                    body.get("reasoning_effort").and_then(Value::as_str),
                    Some("high"),
                    "{source:?} {model}: effort medium 归一化为 high"
                );
                assert!(body.get("temperature").is_none(), "{source:?} {model}");
                assert!(body.get("top_p").is_none(), "{source:?} {model}");
                assert!(body.get("presence_penalty").is_none(), "{source:?} {model}");
                assert!(body.get("frequency_penalty").is_none(), "{source:?} {model}");

                let messages = body
                    .get("messages")
                    .and_then(Value::as_array)
                    .expect("messages must be array");
                for index in [1_usize, 2] {
                    let assistant = messages
                        .get(index)
                        .and_then(Value::as_object)
                        .unwrap_or_else(|| panic!("{source:?} {model}: message {index}"));
                    assert_eq!(
                        assistant.get("reasoning_content").and_then(Value::as_str),
                        Some(""),
                        "{source:?} {model}: assistant {index} 补空串"
                    );
                }

                let required = &body["tools"][0]["function"]["parameters"]["required"];
                assert!(
                    required.is_null(),
                    "{source:?} {model}: 空 required 必须剥离, got {required}"
                );
            }
        }
    }

    #[test]
    fn compat_sources_disable_deepseek_v4_thinking_when_show_thoughts_off() {
        let mut payload = deepseek_v4_compat_payload("GO/deepseek-flash");
        payload.insert("include_reasoning".to_string(), Value::Bool(false));

        let (endpoint, upstream) =
            build_payload(ChatCompletionSource::Custom, payload).expect("payload should build");
        assert_eq!(endpoint, "/chat/completions");

        let body = upstream.as_object().expect("body must be object");
        assert_eq!(
            body.get("thinking")
                .and_then(Value::as_object)
                .and_then(|thinking| thinking.get("type"))
                .and_then(Value::as_str),
            Some("disabled")
        );
        // 关闭思考：不转发 effort，采样参数保留，不补 reasoning_content
        assert!(body.get("reasoning_effort").is_none());
        assert_eq!(
            body.get("temperature").and_then(Value::as_f64),
            Some(1.2),
            "关闭思考时采样参数必须保留"
        );

        let messages = body
            .get("messages")
            .and_then(Value::as_array)
            .expect("messages must be array");
        for index in [1_usize, 2] {
            let assistant = messages
                .get(index)
                .and_then(Value::as_object)
                .unwrap_or_else(|| panic!("message {index}"));
            assert!(
                assistant.get("reasoning_content").is_none(),
                "关闭思考时不补 reasoning_content at {index}"
            );
        }
    }

    #[test]
    fn compat_sources_keep_non_deepseek_models_untouched() {
        for model in ["deepseek-3.2", "qwen3-max", "gpt-5.5"] {
            let (endpoint, upstream) =
                build_payload(ChatCompletionSource::Custom, deepseek_v4_compat_payload(model))
                    .expect("payload should build");
            assert_eq!(endpoint, "/chat/completions");

            let body = upstream.as_object().expect("body must be object");
            assert!(
                body.get("thinking").is_none(),
                "{model}: 不注入 thinking"
            );
            // CUSTOM 源现状：reasoning_effort 无条件转发，保持不动
            assert_eq!(
                body.get("reasoning_effort").and_then(Value::as_str),
                Some("medium"),
                "{model}: effort 转发行为保持现状"
            );
            assert!(body.get("temperature").is_some(), "{model}: 采样参数不动");
            assert!(
                !body["tools"][0]["function"]["parameters"]["required"].is_null(),
                "{model}: required 不动"
            );

            let messages = body
                .get("messages")
                .and_then(Value::as_array)
                .expect("messages must be array");
            for index in [1_usize, 2] {
                let assistant = messages
                    .get(index)
                    .and_then(Value::as_object)
                    .unwrap_or_else(|| panic!("message {index}"));
                assert!(
                    assistant.get("reasoning_content").is_none(),
                    "{model}: 不补 reasoning_content at {index}"
                );
            }
        }
    }

    #[test]
    fn compat_sources_reject_invalid_deepseek_v4_reasoning_effort() {
        let mut payload = deepseek_v4_compat_payload("GO/deepseek-flash");
        payload.insert("reasoning_effort".to_string(), Value::String("auto-ish".to_string()));

        let error = build_payload(ChatCompletionSource::Custom, payload)
            .expect_err("非法 effort 必须报错");
        assert!(
            error
                .to_string()
                .contains("Unsupported DeepSeek reasoning_effort")
        );
    }

    #[test]
    fn openrouter_source_keeps_deepseek_v4_without_thinking_injection() {
        // OR 源有自己的 reasoning 方言（include_reasoning/reasoning.effort），
        // fix-up 不得往 OR 请求里注入 DeepSeek 的 thinking 字段。
        let mut payload = deepseek_v4_compat_payload("deepseek/deepseek-v4.1-flash");
        payload.insert("include_reasoning".to_string(), Value::Bool(false));

        let (endpoint, upstream) = build_payload(ChatCompletionSource::OpenRouter, payload)
            .expect("payload should build");
        assert_eq!(endpoint, "/chat/completions");

        let body = upstream.as_object().expect("body must be object");
        assert!(
            body.get("thinking").is_none(),
            "OR 源不得注入 thinking 字段"
        );
        assert_eq!(
            body.get("include_reasoning").and_then(Value::as_bool),
            Some(false),
            "OR 自己的 include_reasoning 方言保持"
        );
    }
}
