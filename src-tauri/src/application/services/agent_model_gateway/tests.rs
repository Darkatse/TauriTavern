use serde_json::{Value, json};

use super::decode::{decode_chat_completion_exchange, decode_chat_completion_response};
use super::encode::encode_chat_completion_request;
use super::provider_state::next_provider_state;
use super::providers::AgentProviderAdapter;
use super::schema::sanitize_schema_for_provider;
use crate::application::services::agent_tools::BuiltinAgentToolRegistry;
use crate::application::services::chat_completion_service::exchange::{
    ChatCompletionExchange, ChatCompletionProviderFormat, NormalizedChatCompletionResponse,
};
use crate::domain::models::agent::{
    AgentModelContentPart, AgentModelMessage, AgentModelRequest, AgentModelRole, AgentToolCall,
    AgentToolResult,
};
use crate::domain::repositories::chat_completion_repository::{
    CHAT_COMPLETION_PROVIDER_STATE_FIELD, ChatCompletionNormalizationReport, ChatCompletionSource,
};

#[test]
fn decodes_tool_call_to_canonical_name() {
    let registry = BuiltinAgentToolRegistry::phase2c();
    let response = json!({
        "choices": [{
            "message": {
                "content": null,
                "tool_calls": [{
                    "id": "call_1",
                    "type": "function",
                    "function": {
                        "name": "workspace_write_file",
                        "arguments": "{\"path\":\"output/main.md\",\"content\":\"hello\"}"
                    },
                    "signature": "sig_1"
                }]
            }
        }]
    });

    let decoded = decode_chat_completion_response(response, registry.specs()).unwrap();
    assert_eq!(decoded.tool_calls.len(), 1);
    assert_eq!(decoded.tool_calls[0].name, "workspace.write_file");
    assert_eq!(decoded.tool_calls[0].id, "call_1");
    assert_eq!(
        decoded.tool_calls[0].provider_metadata["signature"],
        "sig_1"
    );
}

#[test]
fn rejects_tool_call_without_id() {
    let registry = BuiltinAgentToolRegistry::phase2c();
    let response = json!({
        "choices": [{
            "message": {
                "tool_calls": [{
                    "type": "function",
                    "function": { "name": "workspace_finish", "arguments": "{}" }
                }]
            }
        }]
    });

    let error = decode_chat_completion_response(response, registry.specs()).unwrap_err();
    assert!(error.to_string().contains("tool_call_id is required"));
}

#[test]
fn rejects_normalizer_synthetic_tool_call_id() {
    let registry = BuiltinAgentToolRegistry::phase2c();
    let response = json!({
        "choices": [{
            "message": {
                "tool_calls": [{
                    "id": "tool_call_0",
                    "type": "function",
                    "function": { "name": "workspace_finish", "arguments": "{}" }
                }]
            }
        }]
    });
    let mut report = ChatCompletionNormalizationReport::default();
    report.record_synthetic_tool_call_id("tool_call_0");
    let exchange = ChatCompletionExchange {
        source: ChatCompletionSource::Claude,
        provider_format: ChatCompletionProviderFormat::ClaudeMessages,
        normalized_response: NormalizedChatCompletionResponse::from_value(response).unwrap(),
        normalization_report: report,
    };

    let error = decode_chat_completion_exchange(exchange, registry.specs()).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("provider response is missing tool_call_id")
    );
}

#[test]
fn gemini_schema_sanitizer_removes_unsupported_keys_deeply() {
    let schema = json!({
        "type": "object",
        "title": "Root",
        "additionalProperties": false,
        "$defs": { "x": { "type": "string" } },
        "properties": {
            "mode": {
                "type": "string",
                "const": "draft",
                "default": "draft"
            },
            "nested": {
                "type": "array",
                "items": {
                    "oneOf": [{ "type": "string" }],
                    "examples": ["x"]
                }
            }
        }
    });

    let sanitized = sanitize_schema_for_provider(&schema, AgentProviderAdapter::Gemini);
    assert!(sanitized.get("additionalProperties").is_none());
    assert!(sanitized.get("$defs").is_none());
    assert!(sanitized.get("title").is_none());
    assert!(sanitized["properties"]["mode"].get("const").is_none());
    assert!(sanitized["properties"]["mode"].get("default").is_none());
    assert!(
        sanitized["properties"]["nested"]["items"]
            .get("oneOf")
            .is_none()
    );
    assert!(
        sanitized["properties"]["nested"]["items"]
            .get("examples")
            .is_none()
    );
}

#[test]
fn claude_schema_sanitizer_only_removes_transport_metadata() {
    let schema = json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": "tool.schema.json",
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "mode": {
                "$id": "mode",
                "type": "string",
                "const": "draft"
            }
        }
    });

    let sanitized = sanitize_schema_for_provider(&schema, AgentProviderAdapter::ClaudeMessages);
    assert!(sanitized.get("$schema").is_none());
    assert!(sanitized.get("$id").is_none());
    assert!(sanitized["properties"]["mode"].get("$id").is_none());
    assert_eq!(sanitized["additionalProperties"], false);
    assert_eq!(sanitized["properties"]["mode"]["const"], "draft");
}

#[test]
fn openai_responses_continuation_sends_only_new_tool_results() {
    let registry = BuiltinAgentToolRegistry::phase2c();
    let request = AgentModelRequest {
        payload: json!({
            "chat_completion_source": "custom",
            "custom_api_format": "openai_responses",
            "model": "gpt-5"
        })
        .as_object()
        .cloned()
        .unwrap(),
        messages: vec![
            text_message(AgentModelRole::System, "sys"),
            text_message(AgentModelRole::User, "hi"),
            AgentModelMessage {
                role: AgentModelRole::Assistant,
                parts: vec![AgentModelContentPart::ToolCall {
                    call: AgentToolCall {
                        id: "call_1".to_string(),
                        name: "workspace.write_file".to_string(),
                        arguments: json!({"path":"output/main.md","content":"hi"}),
                        provider_metadata: Value::Null,
                    },
                }],
                provider_metadata: Value::Null,
            },
            tool_result_message("call_1", "workspace.write_file", "ok"),
        ],
        tools: registry.specs().to_vec(),
        tool_choice: Value::String("auto".to_string()),
        provider_state: json!({
            "sessionId": "run_1",
            "providerFormat": "openai_responses",
            "previousResponseId": "resp_1",
            "messageCursor": 2
        }),
    };

    let dto = encode_chat_completion_request(&request).unwrap();
    let messages = dto.payload["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["role"], "tool");
    assert_eq!(dto.payload["previous_response_id"], "resp_1");
    assert!(
        dto.payload
            .get(CHAT_COMPLETION_PROVIDER_STATE_FIELD)
            .is_some()
    );
}

#[test]
fn openai_responses_continuation_requires_valid_cursor() {
    let registry = BuiltinAgentToolRegistry::phase2c();
    let mut request = basic_request(
        "custom",
        Some("openai_responses"),
        vec![text_message(AgentModelRole::User, "hi")],
    );
    request.tools = registry.specs().to_vec();
    request.provider_state = json!({
        "sessionId": "run_1",
        "previousResponseId": "resp_1"
    });

    let error = encode_chat_completion_request(&request).unwrap_err();
    assert!(error.to_string().contains("missing messageCursor"));

    request.provider_state = json!({
        "sessionId": "run_1",
        "previousResponseId": "resp_1",
        "messageCursor": 2
    });
    let error = encode_chat_completion_request(&request).unwrap_err();
    assert!(error.to_string().contains("exceeds message count"));
}

#[test]
fn same_provider_native_metadata_loss_fails_for_native_formats() {
    let cases = [
        (
            ChatCompletionSource::Custom,
            AgentProviderAdapter::OpenAiResponses,
            "openai_responses",
        ),
        (
            ChatCompletionSource::Claude,
            AgentProviderAdapter::ClaudeMessages,
            "claude",
        ),
        (
            ChatCompletionSource::Makersuite,
            AgentProviderAdapter::Gemini,
            "gemini",
        ),
        (
            ChatCompletionSource::Custom,
            AgentProviderAdapter::GeminiInteractions,
            "gemini_interactions",
        ),
    ];

    let registry = BuiltinAgentToolRegistry::phase2c();
    let raw = response_with_tool_call_without_native();
    let response = decode_chat_completion_response(raw, registry.specs()).unwrap();

    for (source, adapter, provider) in cases {
        let error = next_provider_state(
            &provider_state_test_request("run_missing_native"),
            source,
            adapter,
            &response,
        )
        .unwrap_err();
        assert!(
            error.to_string().contains("model.native_metadata_lost"),
            "expected native loss error for {provider}"
        );
        assert!(error.to_string().contains(provider));
    }
}

#[test]
fn provider_state_requires_session_id() {
    let registry = BuiltinAgentToolRegistry::phase2c();
    let raw = json!({
        "id": "msg_1",
        "model": "test",
        "choices": [{ "message": { "role": "assistant", "content": "hello" } }]
    });
    let response = decode_chat_completion_response(raw, registry.specs()).unwrap();
    let mut request = provider_state_test_request("run_1");
    request.provider_state = Value::Null;

    let error = next_provider_state(
        &request,
        ChatCompletionSource::OpenAi,
        AgentProviderAdapter::OpenAiCompatible,
        &response,
    )
    .unwrap_err();

    assert!(error.to_string().contains("sessionId is required"));
}

#[test]
fn claude_provider_state_records_native_continuation() {
    let registry = BuiltinAgentToolRegistry::phase2c();
    let request = provider_state_test_request("run_claude");
    let raw = json!({
        "id": "msg_1",
        "model": "claude-test",
        "choices": [{
            "message": {
                "role": "assistant",
                "content": "",
                "tool_calls": [{
                    "id": "call_1",
                    "type": "function",
                    "function": {
                        "name": "workspace_write_file",
                        "arguments": "{\"path\":\"output/main.md\",\"content\":\"hi\"}"
                    },
                    "signature": "sig_1"
                }],
                "native": {
                    "claude": {
                        "content": [{
                            "type": "tool_use",
                            "id": "call_1",
                            "name": "workspace_write_file",
                            "input": { "path": "output/main.md", "content": "hi" },
                            "signature": "sig_1"
                        }]
                    }
                }
            }
        }]
    });
    let exchange = ChatCompletionExchange {
        source: ChatCompletionSource::Claude,
        provider_format: ChatCompletionProviderFormat::ClaudeMessages,
        normalized_response: NormalizedChatCompletionResponse::from_value(raw).unwrap(),
        normalization_report: ChatCompletionNormalizationReport::default(),
    };

    let response = decode_chat_completion_exchange(exchange, registry.specs()).unwrap();
    let state = next_provider_state(
        &request,
        ChatCompletionSource::Claude,
        AgentProviderAdapter::ClaudeMessages,
        &response,
    )
    .unwrap();

    assert_eq!(state["nativeContinuation"]["provider"], "claude");
    assert_eq!(state["nativeContinuation"]["partCount"], 1);
}

#[test]
fn gemini_provider_state_records_native_continuation() {
    let registry = BuiltinAgentToolRegistry::phase2c();
    let request = provider_state_test_request("run_gemini");
    let raw = json!({
        "id": "gemini-chat-completion",
        "model": "gemini-test",
        "choices": [{
            "message": {
                "role": "assistant",
                "content": "",
                "tool_calls": [{
                    "id": "call_1",
                    "type": "function",
                    "function": {
                        "name": "workspace_write_file",
                        "arguments": "{\"path\":\"output/main.md\",\"content\":\"hi\"}"
                    },
                    "signature": "sig_1"
                }],
                "native": {
                    "gemini": {
                        "content": {
                            "role": "model",
                            "parts": [{
                                "functionCall": {
                                    "id": "call_1",
                                    "name": "workspace_write_file",
                                    "args": { "path": "output/main.md", "content": "hi" }
                                },
                                "thoughtSignature": "sig_1"
                            }]
                        }
                    }
                }
            }
        }]
    });
    let exchange = ChatCompletionExchange {
        source: ChatCompletionSource::Makersuite,
        provider_format: ChatCompletionProviderFormat::Gemini,
        normalized_response: NormalizedChatCompletionResponse::from_value(raw).unwrap(),
        normalization_report: ChatCompletionNormalizationReport::default(),
    };

    let response = decode_chat_completion_exchange(exchange, registry.specs()).unwrap();
    let state = next_provider_state(
        &request,
        ChatCompletionSource::Makersuite,
        AgentProviderAdapter::Gemini,
        &response,
    )
    .unwrap();

    assert_eq!(state["nativeContinuation"]["provider"], "gemini");
    assert_eq!(state["nativeContinuation"]["partCount"], 1);
}

#[test]
fn cross_provider_switch_does_not_migrate_private_native_metadata() {
    let request = basic_request(
        "openai",
        None,
        vec![AgentModelMessage {
            role: AgentModelRole::Assistant,
            parts: vec![
                AgentModelContentPart::Text {
                    text: "portable text".to_string(),
                },
                AgentModelContentPart::Native {
                    provider: "claude".to_string(),
                    value: json!({ "content": [{ "type": "thinking", "signature": "sig_1" }] }),
                },
            ],
            provider_metadata: Value::Null,
        }],
    );

    let dto = encode_chat_completion_request(&request).unwrap();
    let message = dto.payload["messages"][0].as_object().unwrap();
    assert_eq!(message["content"], "portable text");
    assert!(message.get("native").is_none());
}

#[test]
fn same_provider_keeps_matching_private_native_metadata() {
    let request = basic_request(
        "claude",
        None,
        vec![AgentModelMessage {
            role: AgentModelRole::Assistant,
            parts: vec![AgentModelContentPart::Native {
                provider: "claude".to_string(),
                value: json!({ "content": [{ "type": "thinking", "signature": "sig_1" }] }),
            }],
            provider_metadata: Value::Null,
        }],
    );

    let dto = encode_chat_completion_request(&request).unwrap();
    let native = dto.payload["messages"][0]["native"].as_object().unwrap();
    assert!(native.get("claude").is_some());
}

fn provider_state_test_request(session_id: &str) -> AgentModelRequest {
    let mut request = basic_request("claude", None, Vec::new());
    request.tools = BuiltinAgentToolRegistry::phase2c().specs().to_vec();
    request.provider_state = json!({ "sessionId": session_id });
    request
}

fn basic_request(
    source: &str,
    custom_api_format: Option<&str>,
    messages: Vec<AgentModelMessage>,
) -> AgentModelRequest {
    let mut payload = json!({
        "chat_completion_source": source,
        "model": "test-model"
    })
    .as_object()
    .cloned()
    .unwrap();
    if let Some(format) = custom_api_format {
        payload.insert(
            "custom_api_format".to_string(),
            Value::String(format.to_string()),
        );
    }

    AgentModelRequest {
        payload,
        messages,
        tools: Vec::new(),
        tool_choice: Value::String("auto".to_string()),
        provider_state: json!({ "sessionId": "run_1" }),
    }
}

fn response_with_tool_call_without_native() -> Value {
    json!({
        "id": "msg_1",
        "model": "test",
        "choices": [{
            "message": {
                "role": "assistant",
                "content": "",
                "tool_calls": [{
                    "id": "call_1",
                    "type": "function",
                    "function": {
                        "name": "workspace_write_file",
                        "arguments": "{\"path\":\"output/main.md\",\"content\":\"hi\"}"
                    }
                }]
            }
        }]
    })
}

fn text_message(role: AgentModelRole, text: &str) -> AgentModelMessage {
    AgentModelMessage {
        role,
        parts: vec![AgentModelContentPart::Text {
            text: text.to_string(),
        }],
        provider_metadata: Value::Null,
    }
}

fn tool_result_message(call_id: &str, name: &str, content: &str) -> AgentModelMessage {
    AgentModelMessage {
        role: AgentModelRole::Tool,
        parts: vec![AgentModelContentPart::ToolResult {
            result: AgentToolResult {
                call_id: call_id.to_string(),
                name: name.to_string(),
                content: content.to_string(),
                structured: Value::Null,
                is_error: false,
                error_code: None,
                resource_refs: Vec::new(),
            },
        }],
        provider_metadata: Value::Null,
    }
}
