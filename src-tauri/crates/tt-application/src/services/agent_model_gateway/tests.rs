use serde_json::{Value, json};

use super::decode::{decode_chat_completion_exchange, decode_chat_completion_response};
use super::encode::encode_chat_completion_request;
use super::provider_state::{next_provider_state, responses_websocket_session_id};
use super::providers::AgentProviderAdapter;
use super::schema::sanitize_schema_for_provider;
use crate::services::agent_tools::BuiltinAgentToolRegistry;
use crate::services::chat_completion_service::exchange::{
    ChatCompletionExchange, ChatCompletionProviderFormat, NormalizedChatCompletionResponse,
};
use tt_domain::models::agent::{
    AgentModelContentPart, AgentModelMessage, AgentModelRequest, AgentModelRole, AgentModelTool,
    AgentToolResult,
};
use tt_domain::models::tool::{ToolChoice, ToolId, ToolInvocation, ToolProviderId};
use tt_ports::repositories::chat_completion_repository::{
    CHAT_COMPLETION_PROVIDER_STATE_FIELD, ChatCompletionNormalizationReport, ChatCompletionSource,
};

fn model_tools(registry: &BuiltinAgentToolRegistry) -> Vec<AgentModelTool> {
    registry
        .catalog()
        .iter()
        .map(|descriptor| AgentModelTool {
            tool_id: descriptor.id.clone(),
            model_alias: descriptor.id.native_name().replace('.', "_"),
            description: descriptor.description.clone(),
            input_schema: descriptor.input_schema.clone(),
        })
        .collect()
}

fn model_tool(registry: &BuiltinAgentToolRegistry, name: &str) -> AgentModelTool {
    model_tools(registry)
        .into_iter()
        .find(|tool| tool.tool_id.native_name() == name)
        .expect("builtin model tool")
}

#[test]
fn decodes_tool_call_to_canonical_identity() {
    let registry = BuiltinAgentToolRegistry::all();
    let mut write = model_tool(&registry, "workspace.write_file");
    write.tool_id = ToolId::new(
        &ToolProviderId::parse("mcp/registration-1").unwrap(),
        "workspace.write_file",
    )
    .unwrap();
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

    let decoded = decode_chat_completion_response(response, &[write]).unwrap();
    assert_eq!(decoded.tool_calls.len(), 1);
    assert_eq!(
        decoded.tool_calls[0].tool_id,
        ToolId::new(
            &ToolProviderId::parse("mcp/registration-1").unwrap(),
            "workspace.write_file",
        )
        .unwrap()
    );
    assert_eq!(decoded.tool_calls[0].call_id, "call_1");
    assert_eq!(
        decoded.tool_calls[0].provider_metadata["signature"],
        "sig_1"
    );
}

#[test]
fn openai_compatible_replays_opaque_continuation() {
    let registry = BuiltinAgentToolRegistry::all();
    let tools = model_tools(&registry);
    let response = json!({
        "choices": [{
            "message": {
                "content": null,
                "reasoning": "Need tools",
                "reasoning_content": " exact reasoning ",
                "reasoning_details": [{
                    "type": "reasoning.encrypted",
                    "id": "call_1",
                    "data": "opaque-reasoning"
                }],
                "tool_calls": [
                    {
                        "id": "call_1",
                        "type": "function",
                        "function": {
                            "name": "workspace_finish",
                            "arguments": "{}"
                        },
                        "extra_content": {
                            "google": { "thought_signature": "opaque-signature" }
                        }
                    },
                    {
                        "id": "call_2",
                        "type": "function",
                        "function": {
                            "name": "workspace_write_file",
                            "arguments": "{\"path\":\"output/main.md\",\"content\":\"hello\"}"
                        }
                    }
                ]
            }
        }]
    });

    let decoded = decode_chat_completion_response(response, &tools).unwrap();
    let mut request = basic_request("custom", Some("openai_compat"), vec![decoded.message]);
    request.tools = tools;

    let dto = encode_chat_completion_request(&request, false).unwrap();
    let message = &dto.payload["messages"][0];
    assert_eq!(message["reasoning"], "Need tools");
    assert_eq!(message["reasoning_content"], " exact reasoning ");
    assert_eq!(
        message["reasoning_details"],
        json!([{
            "type": "reasoning.encrypted",
            "id": "call_1",
            "data": "opaque-reasoning"
        }])
    );
    let calls = message["tool_calls"].as_array().unwrap();
    assert_eq!(
        calls[0]["extra_content"],
        json!({ "google": { "thought_signature": "opaque-signature" } })
    );
    assert!(calls[1].get("extra_content").is_none());
}

#[test]
fn rejects_tool_names_outside_the_current_turn_aliases() {
    let registry = BuiltinAgentToolRegistry::all();
    let write = model_tool(&registry, "workspace.write_file");

    for (raw_name, tools) in [
        ("workspace_write_file", Vec::new()),
        ("workspace.write_file", vec![write.clone()]),
    ] {
        let response = json!({
            "choices": [{
                "message": {
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": { "name": raw_name, "arguments": "{}" }
                    }]
                }
            }]
        });

        let error = decode_chat_completion_response(response, &tools).unwrap_err();
        assert!(error.to_string().contains("model.unknown_tool_call"));
    }
}

#[test]
fn rejects_tool_call_without_id() {
    let registry = BuiltinAgentToolRegistry::all();
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

    let error = decode_chat_completion_response(response, &model_tools(&registry)).unwrap_err();
    assert!(error.to_string().contains("tool_call_id is required"));
}

#[test]
fn rejects_normalizer_synthetic_tool_call_id() {
    let registry = BuiltinAgentToolRegistry::all();
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

    let error = decode_chat_completion_exchange(exchange, &model_tools(&registry)).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("provider response is missing tool_call_id")
    );
}

#[test]
fn encodes_typed_tool_choice_against_advertised_tools() {
    let registry = BuiltinAgentToolRegistry::all();
    let finish = model_tool(&registry, "workspace.finish");
    let cases = [
        (ToolChoice::None, json!("none")),
        (ToolChoice::Auto, json!("auto")),
        (ToolChoice::Required, json!("required")),
        (
            ToolChoice::Specific(ToolId::builtin("workspace.finish").unwrap()),
            json!({
                "type": "function",
                "function": { "name": finish.model_alias }
            }),
        ),
    ];

    for (tool_choice, expected) in cases {
        let mut request = basic_request("openai", None, Vec::new());
        request.tools = vec![finish.clone()];
        request.tool_choice = tool_choice;

        let dto = encode_chat_completion_request(&request, false).expect("choice should encode");
        assert_eq!(dto.payload.get("tool_choice"), Some(&expected));
    }
}

#[test]
fn rejects_tool_choice_outside_the_advertised_set() {
    let registry = BuiltinAgentToolRegistry::all();
    let mut request = basic_request("openai", None, Vec::new());
    request.tools = vec![model_tool(&registry, "workspace.finish")];

    request.tool_choice = ToolChoice::Specific(ToolId::builtin("workspace.write_file").unwrap());
    let error = encode_chat_completion_request(&request, false)
        .expect_err("unadvertised tool choice must fail");
    assert!(
        error
            .to_string()
            .contains("agent.tool_choice_tool_not_advertised")
    );

    let external = ToolProviderId::parse("mcp/registration-1").unwrap();
    request.tool_choice = ToolChoice::Specific(ToolId::new(&external, "search").unwrap());
    let error = encode_chat_completion_request(&request, false)
        .expect_err("unadvertised external tool choice must fail");
    assert!(
        error
            .to_string()
            .contains("agent.tool_choice_tool_not_advertised")
    );
}

#[test]
fn encodes_tool_results_as_model_facing_text() {
    let builtin_id = ToolId::builtin("search").unwrap();
    let mcp_id = ToolId::new(
        &ToolProviderId::parse("mcp/registration-1").unwrap(),
        "search",
    )
    .unwrap();
    let mut request = basic_request("openai", None, Vec::new());
    request.tools = vec![
        AgentModelTool {
            tool_id: builtin_id,
            model_alias: "builtin_search".to_string(),
            description: None,
            input_schema: json!({ "type": "object" }),
        },
        AgentModelTool {
            tool_id: mcp_id.clone(),
            model_alias: "mcp_search".to_string(),
            description: None,
            input_schema: json!({ "type": "object" }),
        },
    ];
    request.messages = vec![AgentModelMessage {
        role: AgentModelRole::Tool,
        parts: vec![AgentModelContentPart::ToolResult {
            result: AgentToolResult {
                call_id: "call_mcp".to_string(),
                tool_id: mcp_id,
                content: "done".to_string(),
                structured: json!({ "auditOnly": true }),
                is_error: false,
                error_code: None,
                resource_refs: vec!["tool-results/call_mcp.json".to_string()],
            },
        }],
        provider_metadata: Value::Null,
    }];

    let dto = encode_chat_completion_request(&request, false).unwrap();
    assert_eq!(dto.payload["messages"][0]["name"], "mcp_search");
    assert_eq!(
        dto.payload["messages"][0]["content"],
        Value::String("done".to_string())
    );

    let AgentModelContentPart::ToolResult { result } = &mut request.messages[0].parts[0] else {
        panic!("expected tool result")
    };
    result.content = "The requested file is not available.".to_string();
    result.is_error = true;
    result.error_code = Some("workspace.file_not_found".to_string());
    let dto = encode_chat_completion_request(&request, false).unwrap();
    assert_eq!(
        dto.payload["messages"][0]["content"],
        Value::String("## Tool error\n\nThe requested file is not available.".to_string())
    );
}

#[test]
fn agent_encoder_owns_tool_selection_stream_and_choice_count() {
    let mut request = basic_request("openai", None, Vec::new());
    request.tool_choice = ToolChoice::Required;
    let error = encode_chat_completion_request(&request, false).expect_err("required needs tools");
    assert!(
        error
            .to_string()
            .contains("agent.tool_choice_requires_tools")
    );

    request.tool_choice = ToolChoice::Auto;
    request
        .payload
        .insert("tools".to_string(), json!([{"raw": true}]));
    request
        .payload
        .insert("tool_choice".to_string(), json!("required"));
    request.payload.insert("stream".to_string(), json!(true));
    request.payload.insert("n".to_string(), json!(4));
    let dto = encode_chat_completion_request(&request, false).expect("auto without tools is valid");
    assert!(dto.payload.get("tools").is_none());
    assert!(dto.payload.get("tool_choice").is_none());
    assert_eq!(dto.payload["stream"], false);
    assert_eq!(dto.payload["n"], 1);

    let dto = encode_chat_completion_request(&request, true).unwrap();
    assert_eq!(dto.payload["stream"], true);
}

#[test]
fn rejects_provider_refusal_before_decoding_the_agent_turn() {
    let response = json!({
        "stop_reason": "refusal",
        "stop_details": {
            "explanation": "This request was declined by the provider."
        },
        "choices": [{
            "finish_reason": "refusal",
            "message": {
                "role": "assistant",
                "content": "partial output"
            }
        }]
    });

    let error = decode_chat_completion_response(response, &[]).unwrap_err();
    assert!(error.to_string().contains("model.provider_refusal"));
    assert!(
        error
            .to_string()
            .contains("This request was declined by the provider.")
    );
}

#[test]
fn rejects_truncated_agent_turns() {
    for response in [
        json!({
            "stop_reason": "model_context_window_exceeded",
            "choices": [{
                "message": { "role": "assistant", "content": "partial output" }
            }]
        }),
        json!({
            "choices": [{
                "finish_reason": "length",
                "message": { "role": "assistant", "content": "partial output" }
            }]
        }),
    ] {
        let error = decode_chat_completion_response(response, &[]).unwrap_err();
        assert!(error.to_string().contains("model.output_truncated"));
    }
}

#[test]
fn gemini_schema_sanitizer_projects_nested_objects_to_agent_friendly_schema() {
    let schema = json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "agentId": { "type": "string" },
            "task": {
                "type": "object",
                "additionalProperties": true,
                "properties": {
                    "title": { "type": "string" },
                    "objective": { "type": "string" },
                    "context": {
                        "type": "object",
                        "additionalProperties": true,
                        "description": "Free-form task context."
                    }
                },
                "required": ["objective"]
            }
        },
        "required": ["agentId", "task", "missing"]
    });

    let sanitized = sanitize_schema_for_provider(&schema, AgentProviderAdapter::Gemini);

    assert_eq!(sanitized["required"], json!(["agentId", "task"]));
    assert!(sanitized["properties"]["task"].get("required").is_none());
    assert_eq!(sanitized["properties"]["task"]["type"], "object");
    assert_eq!(
        sanitized["properties"]["task"]["properties"]["context"]["type"],
        "string"
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
    let registry = BuiltinAgentToolRegistry::all();
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
                    call: ToolInvocation {
                        call_id: "call_1".to_string(),
                        tool_id: ToolId::builtin("workspace.write_file").unwrap(),
                        arguments: json!({"path":"output/main.md","content":"hi"}),
                        provider_metadata: Value::Null,
                    },
                }],
                provider_metadata: Value::Null,
            },
            tool_result_message("call_1", "workspace.write_file", "ok"),
        ],
        tools: model_tools(&registry),
        tool_choice: ToolChoice::Auto,
        provider_state: json!({
            "sessionId": "run_1",
            "providerFormat": "openai_responses",
            "transport": "responses_websocket",
            "previousResponseId": "resp_1",
            "messageCursor": 2
        }),
    };

    let dto = encode_chat_completion_request(&request, false).unwrap();
    let messages = dto.payload["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["role"], "tool");
    assert_eq!(messages[0]["name"], "workspace_write_file");
    assert_eq!(dto.payload["previous_response_id"], "resp_1");
    assert!(
        dto.payload
            .get(CHAT_COMPLETION_PROVIDER_STATE_FIELD)
            .is_some()
    );
}

#[test]
fn openai_responses_portable_mode_replays_full_transcript() {
    let mut request = basic_request(
        "custom",
        Some("openai_responses"),
        vec![
            text_message(AgentModelRole::System, "sys"),
            text_message(AgentModelRole::User, "hi"),
        ],
    );
    request.provider_state = json!({
        "sessionId": "run_1",
        "previousResponseId": "resp_ignored",
        "messageCursor": 1
    });

    let dto = encode_chat_completion_request(&request, false).unwrap();
    assert_eq!(dto.payload["messages"].as_array().unwrap().len(), 2);
    assert!(dto.payload.get("previous_response_id").is_none());
}

#[test]
fn openai_responses_next_state_only_enables_continuation_for_websocket_mode() {
    let raw = json!({
        "id": "resp_2",
        "model": "test",
        "choices": [{ "message": { "role": "assistant", "content": "done" } }]
    });
    let response = decode_chat_completion_response(raw, &[]).unwrap();
    let mut request = basic_request("custom", Some("openai_responses"), Vec::new());

    let portable = next_provider_state(
        &request,
        ChatCompletionSource::Custom,
        AgentProviderAdapter::OpenAiResponses,
        &response,
    )
    .unwrap();
    assert!(portable.get("transport").is_none());
    assert!(portable.get("previousResponseId").is_none());

    request.provider_state["transport"] = json!("responses_websocket");
    let enhanced = next_provider_state(
        &request,
        ChatCompletionSource::Custom,
        AgentProviderAdapter::OpenAiResponses,
        &response,
    )
    .unwrap();
    assert_eq!(enhanced["transport"], "responses_websocket");
    assert_eq!(enhanced["previousResponseId"], "resp_2");
    assert_eq!(responses_websocket_session_id(&request), Some("run_1"));
}

#[test]
fn openai_responses_continuation_requires_valid_cursor() {
    let registry = BuiltinAgentToolRegistry::all();
    let mut request = basic_request(
        "custom",
        Some("openai_responses"),
        vec![text_message(AgentModelRole::User, "hi")],
    );
    request.tools = model_tools(&registry);
    request.provider_state = json!({
        "sessionId": "run_1",
        "transport": "responses_websocket",
        "previousResponseId": "resp_1"
    });

    let error = encode_chat_completion_request(&request, false).unwrap_err();
    assert!(error.to_string().contains("missing messageCursor"));

    request.provider_state = json!({
        "sessionId": "run_1",
        "transport": "responses_websocket",
        "previousResponseId": "resp_1",
        "messageCursor": 2
    });
    let error = encode_chat_completion_request(&request, false).unwrap_err();
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

    let registry = BuiltinAgentToolRegistry::all();
    let raw = response_with_tool_call_without_native();
    let response = decode_chat_completion_response(raw, &model_tools(&registry)).unwrap();

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
    let registry = BuiltinAgentToolRegistry::all();
    let raw = json!({
        "id": "msg_1",
        "model": "test",
        "choices": [{ "message": { "role": "assistant", "content": "hello" } }]
    });
    let response = decode_chat_completion_response(raw, &model_tools(&registry)).unwrap();
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
    let registry = BuiltinAgentToolRegistry::all();
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

    let response = decode_chat_completion_exchange(exchange, &model_tools(&registry)).unwrap();
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
    let registry = BuiltinAgentToolRegistry::all();
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

    let response = decode_chat_completion_exchange(exchange, &model_tools(&registry)).unwrap();
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

    let dto = encode_chat_completion_request(&request, false).unwrap();
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

    let dto = encode_chat_completion_request(&request, false).unwrap();
    let native = dto.payload["messages"][0]["native"].as_object().unwrap();
    assert!(native.get("claude").is_some());
}

fn provider_state_test_request(session_id: &str) -> AgentModelRequest {
    let mut request = basic_request("claude", None, Vec::new());
    request.tools = model_tools(&BuiltinAgentToolRegistry::all());
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
        tool_choice: ToolChoice::Auto,
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
                tool_id: ToolId::builtin(name).unwrap(),
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
