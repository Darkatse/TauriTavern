use std::borrow::Cow;

use serde_json::{Value, json};
use tt_contracts::observability::{
    MessageViewDto, ParameterViewDto, PartViewDto, RequestKindDto, ResponseViewDto, ToolViewDto,
    UsageViewDto,
};

use super::*;

fn stream_view(kind: RequestKindDto, events: &[Value]) -> ResponseViewDto {
    let document = merge_stream_events(
        kind,
        events.iter().filter_map(|event| event.as_object().cloned()),
    );
    project_response_view(kind, &document, None)
}

fn response_view(kind: RequestKindDto, response: &Value) -> ResponseViewDto {
    project_response_view(kind, response, None)
}

#[test]
fn format_endpoint_strips_userinfo_but_keeps_path() {
    assert_eq!(
        format_endpoint("https://user:pass@example.com/v1", "/messages"),
        "https://example.com/v1/messages"
    );
}

#[test]
fn openai_request_and_response_project_tools_calls_results_and_general_multiline_args() {
    let payload = json!({
        "model": "gpt-5",
        "temperature": 0.7,
        "tool_choice": "auto",
        "tools": [{
            "type": "function",
            "function": {
                "name": "custom_patch_tool",
                "description": "Write notes",
                "parameters": { "type": "object" }
            }
        }],
        "messages": [
            { "role": "system", "content": "sys" },
            { "role": "assistant", "content": null, "tool_calls": [{
                "id": "call_1",
                "type": "function",
                "function": {
                    "name": "custom_patch_tool",
                    "arguments": "{\"path\":\"output/main.md\",\"patch\":\"first line\\nsecond line\"}"
                }
            }]},
            {
                "role": "tool",
                "tool_call_id": "call_1",
                "name": "custom_patch_tool",
                "content": "{\"ok\":true,\"content\":\"Wrote 2 bytes.\"}"
            }
        ]
    });

    let request = project_request_view(RequestKindDto::Chat, &payload);
    assert_eq!(request.kind, RequestKindDto::Chat);
    assert_eq!(request.tool_choice, Some(json!("auto")));
    assert_eq!(
        request.tools,
        vec![ToolViewDto {
            name: "custom_patch_tool".to_string(),
            description: Some("Write notes".to_string()),
            parameters: Some(json!({ "type": "object" })),
            path: "/tools/0".to_string(),
        }]
    );
    assert_eq!(
        request.parameters,
        vec![
            ParameterViewDto {
                key: "model".to_string(),
                path: "/model".to_string(),
                value: json!("gpt-5"),
            },
            ParameterViewDto {
                key: "temperature".to_string(),
                path: "/temperature".to_string(),
                value: json!(0.7),
            },
        ]
    );
    assert_eq!(
        request.messages[1].parts,
        vec![PartViewDto::ToolCall {
            id: "call_1".to_string(),
            name: "custom_patch_tool".to_string(),
            arguments: json!({
                "path": "output/main.md",
                "patch": "first line\nsecond line"
            }),
            arguments_text: None,
            path: Some("/messages/1/tool_calls/0".to_string()),
        }]
    );
    assert_eq!(
        request.messages[2].parts,
        vec![PartViewDto::ToolResult {
            call_id: "call_1".to_string(),
            name: Some("custom_patch_tool".to_string()),
            is_error: None,
            parts: vec![PartViewDto::Text {
                text: "{\"ok\":true,\"content\":\"Wrote 2 bytes.\"}".to_string(),
                cache_breakpoint: false,
                path: Some("/messages/2/content".to_string()),
            }],
            path: Some("/messages/2".to_string()),
        }]
    );

    assert_eq!(
        render_request_readable(&request),
        "[tools count=1 tool_choice=auto]\n- custom_patch_tool\n\n[system]\nsys\n\n[assistant]\n[tool_call id=call_1 name=custom_patch_tool]\n{\"path\":\"output/main.md\"}\n[patch]\nfirst line\nsecond line\n\n[tool id=call_1 name=custom_patch_tool]\n{\"ok\":true,\"content\":\"Wrote 2 bytes.\"}"
    );

    let response = json!({
        "choices": [{
            "finish_reason": "tool_calls",
            "message": {
                "role": "assistant",
                "content": null,
                "tool_calls": [{
                    "id": "call_2",
                    "type": "function",
                    "function": {
                        "name": "workspace_commit",
                        "arguments": "{not-json"
                    }
                }]
            }
        }],
        "usage": {
            "prompt_tokens": 120,
            "prompt_tokens_details": { "cached_tokens": 80 },
            "completion_tokens": 15,
            "completion_tokens_details": { "reasoning_tokens": 5 }
        }
    });

    let response_view = project_response_view(RequestKindDto::Chat, &response, Some(""));
    assert_eq!(response_view.finish_reason.as_deref(), Some("tool_calls"));
    assert_eq!(
        response_view.usage,
        Some(UsageViewDto {
            input: Some(120),
            cache_read: Some(80),
            output: Some(15),
            reasoning: Some(5),
        })
    );
    assert_eq!(
        response_view.candidates[0].parts,
        vec![PartViewDto::ToolCall {
            id: "call_2".to_string(),
            name: "workspace_commit".to_string(),
            arguments: serde_json::Value::Null,
            arguments_text: Some("{not-json".to_string()),
            path: Some("/choices/0/message/tool_calls/0".to_string()),
        }]
    );
}

#[test]
fn claude_gemini_and_responses_requests_project_to_unified_exchange_view() {
    let claude_payload = json!({
        "model": "claude-sonnet-4-5",
        "system": [{
            "type": "text",
            "text": "sys",
            "cache_control": { "type": "ephemeral", "ttl": "5m" }
        }],
        "tools": [{
            "name": "read_file",
            "description": "Read file",
            "input_schema": { "type": "object" }
        }],
        "messages": [
            { "role": "user", "content": "hi" },
            { "role": "assistant", "content": [
                { "type": "thinking", "thinking": "Need file", "signature": "opaque" },
                { "type": "tool_use", "id": "toolu_1", "name": "read_file", "input": { "path": "a.md" } }
            ]},
            { "role": "user", "content": [
                { "type": "tool_result", "tool_use_id": "toolu_1", "content": "missing", "is_error": true }
            ]}
        ]
    });

    let claude_req = project_request_view(RequestKindDto::Chat, &claude_payload);
    assert_eq!(claude_req.system_field.as_deref(), Some("system"));
    assert_eq!(claude_req.system_path.as_deref(), Some("/system"));
    assert_eq!(
        claude_req.system,
        vec![PartViewDto::Text {
            text: "sys".to_string(),
            cache_breakpoint: true,
            path: Some("/system/0".to_string()),
        }]
    );
    assert_eq!(claude_req.tools[0].name, "read_file");
    assert_eq!(
        claude_req.messages[1].parts,
        vec![
            PartViewDto::Reasoning {
                text: "Need file".to_string(),
                path: Some("/messages/1/content/0".to_string()),
            },
            PartViewDto::NativeState {
                kind: "signature".to_string(),
                length: 6,
                path: Some("/messages/1/content/0/signature".to_string()),
            },
            PartViewDto::ToolCall {
                id: "toolu_1".to_string(),
                name: "read_file".to_string(),
                arguments: json!({ "path": "a.md" }),
                arguments_text: None,
                path: Some("/messages/1/content/1".to_string()),
            },
        ]
    );
    assert_eq!(
        claude_req.messages[2].parts,
        vec![PartViewDto::ToolResult {
            call_id: "toolu_1".to_string(),
            name: None,
            is_error: Some(true),
            parts: vec![PartViewDto::Text {
                text: "missing".to_string(),
                cache_breakpoint: false,
                path: Some("/messages/2/content/0/content".to_string()),
            }],
            path: Some("/messages/2/content/0".to_string()),
        }]
    );
    assert_eq!(
        render_request_readable(&claude_req),
        "[tools count=1]\n- read_file\n\n[system]\nsys\n\n[user]\nhi\n\n[assistant]\n[reasoning]\nNeed file\n[native_state kind=signature]\n[tool_call id=toolu_1 name=read_file]\n{\"path\":\"a.md\"}\n\n[user]\n[tool_result id=toolu_1 is_error=true]\nmissing"
    );

    let gemini_payload = json!({
        "systemInstruction": { "parts": [{ "text": "Be concise" }] },
        "tools": [{
            "function_declarations": [{
                "name": "lookup",
                "parameters": { "type": "object" }
            }]
        }],
        "contents": [
            { "role": "user", "parts": [{ "text": "Hello" }] },
            { "role": "model", "parts": [{
                "functionCall": { "id": "fc_1", "name": "lookup", "args": { "q": "tea" } },
                "thoughtSignature": "sig12"
            }]},
            { "role": "user", "parts": [
                { "functionResponse": { "id": "fc_1", "name": "lookup", "response": { "content": "green" } } },
                { "text": "Thanks" }
            ]}
        ]
    });

    let gemini_req = project_request_view(RequestKindDto::Chat, &gemini_payload);
    assert_eq!(
        gemini_req.system_field.as_deref(),
        Some("systemInstruction")
    );
    assert_eq!(gemini_req.tools[0].path, "/tools/0/function_declarations/0");
    assert_eq!(gemini_req.messages[1].role, "model");
    assert_eq!(gemini_req.messages[1].parts.len(), 2);
    assert_eq!(
        gemini_req.messages[2].parts,
        vec![
            PartViewDto::ToolResult {
                call_id: "fc_1".to_string(),
                name: Some("lookup".to_string()),
                is_error: None,
                parts: vec![PartViewDto::Text {
                    text: "green".to_string(),
                    cache_breakpoint: false,
                    path: Some("/contents/2/parts/0/functionResponse/response/content".to_string()),
                }],
                path: Some("/contents/2/parts/0".to_string()),
            },
            PartViewDto::Text {
                text: "Thanks".to_string(),
                cache_breakpoint: false,
                path: Some("/contents/2/parts/1".to_string()),
            },
        ]
    );
    // Text after a part of another kind starts on its own line.
    assert_eq!(
        render_request_readable(&gemini_req),
        "[tools count=1]\n- lookup\n\n[system]\nBe concise\n\n[user]\nHello\n\n[model]\n[tool_call id=fc_1 name=lookup]\n{\"q\":\"tea\"}\n[native_state kind=thoughtSignature]\n\n[user]\n[tool_result id=fc_1 name=lookup]\ngreen\nThanks"
    );

    let responses_payload = json!({
        "model": "gpt-6-sol",
        "instructions": "Follow rules",
        "input": [
            { "role": "developer", "content": "dev rule" },
            { "type": "function_call", "call_id": "call_9", "name": "ping", "arguments": "{}" },
            { "type": "function_call_output", "call_id": "call_9", "output": "pong" }
        ]
    });

    let responses_req = project_request_view(RequestKindDto::Chat, &responses_payload);
    assert_eq!(responses_req.system_field.as_deref(), Some("instructions"));
    assert_eq!(responses_req.messages[0].role, "developer");
    assert_eq!(responses_req.messages[1].role, "assistant");
    assert_eq!(responses_req.messages[2].role, "tool");
}

#[test]
fn wire_log_payload_redacts_inline_media_including_input_audio() {
    let payload = json!({
        "model": "gpt-5",
        "messages": [
            {
                "role": "user",
                "content": [
                    {
                        "type": "image_url",
                        "image_url": { "url": "data:image/png;base64,AAAA" }
                    },
                    {
                        "type": "input_audio",
                        "input_audio": { "format": "wav", "data": "EEEEEEEE" }
                    }
                ]
            },
            {
                "role": "assistant",
                "content": "ok",
                "reasoning_details": [{
                    "type": "reasoning.encrypted",
                    "data": "text-sig",
                    "format": "google-gemini-v1"
                }]
            }
        ],
        "interactions": { "type": "image", "mime_type": "image/webp", "data": "DDDDDDD" },
        "claude": {
            "source": {
                "type": "base64",
                "media_type": "image/jpeg",
                "data": "BBBBB"
            }
        },
        "gemini": {
            "inlineData": {
                "mimeType": "video/mp4",
                "data": "CCCCCC"
            }
        }
    });

    let sanitized = wire_log_payload(Cow::Borrowed(&payload));
    assert_eq!(
        sanitized.pointer("/messages/0/content/0/image_url/url"),
        Some(&json!(
            "<inline media omitted; mime=image/png; base64_chars=4>"
        ))
    );
    assert_eq!(
        sanitized.pointer("/messages/0/content/1/input_audio/data"),
        Some(&json!(
            "<inline media omitted; mime=audio/wav; base64_chars=8>"
        ))
    );
    // OpenRouter encrypted reasoning is not misidentified as inline audio.
    assert_eq!(
        sanitized.pointer("/messages/1/reasoning_details/0/data"),
        Some(&json!("text-sig"))
    );
    assert_eq!(
        sanitized.pointer("/claude/source/data"),
        Some(&json!(
            "<inline media omitted; mime=image/jpeg; base64_chars=5>"
        ))
    );
    assert_eq!(
        sanitized.pointer("/gemini/inlineData/data"),
        Some(&json!(
            "<inline media omitted; mime=video/mp4; base64_chars=6>"
        ))
    );
    assert_eq!(
        sanitized.pointer("/interactions/data"),
        Some(&json!(
            "<inline media omitted; mime=image/webp; base64_chars=7>"
        ))
    );

    let request = project_request_view(RequestKindDto::Chat, &sanitized);
    assert_eq!(
        request.messages[0].parts,
        vec![
            PartViewDto::Media {
                mime: "image/png".to_string(),
                encoded_length: Some(4),
                url: None,
                path: Some("/messages/0/content/0".to_string()),
            },
            PartViewDto::Media {
                mime: "audio/wav".to_string(),
                encoded_length: Some(8),
                url: None,
                path: Some("/messages/0/content/1".to_string()),
            },
        ]
    );
    assert_eq!(
        request.messages[1].parts[0],
        PartViewDto::NativeState {
            kind: "reasoning.encrypted".to_string(),
            length: 8,
            path: Some("/messages/1/reasoning_details/0".to_string()),
        }
    );
}

#[test]
fn openai_stream_merges_candidates_reasoning_tool_calls_and_usage_like_the_response() {
    let events = vec![
        json!({"choices":[{"index":0,"delta":{"role":"assistant","reasoning":"Plan","reasoning_details":[{"type":"reasoning.text","text":"Plan"}]}}]}),
        json!({"choices":[{"index":0,"delta":{"reasoning":" more","reasoning_details":[{"type":"reasoning.text","text":" more"}],"content":"Checking"}}]}),
        json!({"choices":[{"index":0,"delta":{"tool_calls":[{"index":1,"id":"call_1","type":"function","function":{"name":"read_file","arguments":"{\"path\":"}}]}}]}),
        json!({"choices":[{"index":0,"delta":{"tool_calls":[{"index":1,"function":{"arguments":"\"a.md\"}"}}]},"finish_reason":"tool_calls"}]}),
        json!({"choices":[{"index":1,"delta":{"role":"assistant","content":"Second"},"finish_reason":"stop"}]}),
        json!({"choices":[],"usage":{"prompt_tokens":100,"prompt_tokens_details":{"cached_tokens":60},"completion_tokens":20,"completion_tokens_details":{"reasoning_tokens":8}}}),
    ];
    let response = json!({
        "choices": [
            {
                "index": 0,
                "finish_reason": "tool_calls",
                "message": {
                    "role": "assistant",
                    "content": "Checking",
                    "reasoning": "Plan more",
                    "reasoning_content": "Plan more",
                    "reasoning_details": [{ "type": "reasoning.text", "text": "Plan more" }],
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": { "name": "read_file", "arguments": "{\"path\":\"a.md\"}" }
                    }]
                }
            },
            {
                "index": 1,
                "finish_reason": "stop",
                "message": { "role": "assistant", "content": "Second" }
            }
        ],
        "usage": {
            "prompt_tokens": 100,
            "prompt_tokens_details": { "cached_tokens": 60 },
            "completion_tokens": 20,
            "completion_tokens_details": { "reasoning_tokens": 8 }
        }
    });

    let stream = stream_view(RequestKindDto::Chat, &events);
    let document = response_view(RequestKindDto::Chat, &response);
    assert_eq!(stream.candidates, document.candidates);
    assert_eq!(
        stream.candidates,
        vec![
            MessageViewDto {
                role: "assistant".to_string(),
                name: None,
                path: None,
                parts: vec![
                    PartViewDto::Reasoning {
                        text: "Plan more".to_string(),
                        path: None,
                    },
                    PartViewDto::Text {
                        text: "Checking".to_string(),
                        cache_breakpoint: false,
                        path: None,
                    },
                    PartViewDto::ToolCall {
                        id: "call_1".to_string(),
                        name: "read_file".to_string(),
                        arguments: json!({ "path": "a.md" }),
                        arguments_text: None,
                        path: None,
                    },
                ],
            },
            MessageViewDto {
                role: "assistant".to_string(),
                name: None,
                path: None,
                parts: vec![PartViewDto::Text {
                    text: "Second".to_string(),
                    cache_breakpoint: false,
                    path: None,
                }],
            },
        ]
    );
    assert_eq!(stream.usage, document.usage);
    assert_eq!(
        stream.usage,
        Some(UsageViewDto {
            input: Some(100),
            cache_read: Some(60),
            output: Some(20),
            reasoning: Some(8),
        })
    );
    assert_eq!(stream.finish_reason.as_deref(), Some("tool_calls"));
}

#[test]
fn anthropic_stream_merges_blocks_usage_and_native_content_like_the_response() {
    let events = vec![
        json!({"type":"message_start","message":{"role":"assistant","content":[],"usage":{"input_tokens":10,"cache_creation_input_tokens":20,"cache_read_input_tokens":70}}}),
        json!({"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"Plan."}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"sig123"}}),
        json!({"type":"content_block_stop","index":0}),
        json!({"type":"content_block_start","index":1,"content_block":{"type":"server_tool_use","id":"srvtoolu_1","name":"web_search","input":{}}}),
        json!({"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"query\":\"tea\"}"}}),
        json!({"type":"content_block_start","index":2,"content_block":{"type":"tool_use","id":"toolu_1","name":"write_file","input":{}}}),
        json!({"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":""}}),
        json!({"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"{\"path\":"}}),
        json!({"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"\"b.md\"}"}}),
        json!({"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":14}}),
        json!({"type":"message_stop"}),
    ];
    let response = json!({
        "choices": [{
            "index": 0,
            "finish_reason": "tool_calls",
            "message": {
                "role": "assistant",
                "content": "",
                "reasoning_content": "Plan.",
                "tool_calls": [{
                    "id": "toolu_1",
                    "type": "function",
                    "function": { "name": "write_file", "arguments": "{\"path\":\"b.md\"}" }
                }],
                "native": {
                    "claude": {
                        "content": [
                            { "type": "thinking", "thinking": "Plan.", "signature": "sig123" },
                            {
                                "type": "server_tool_use",
                                "id": "srvtoolu_1",
                                "name": "web_search",
                                "input": { "query": "tea" }
                            },
                            {
                                "type": "tool_use",
                                "id": "toolu_1",
                                "name": "write_file",
                                "input": { "path": "b.md" }
                            }
                        ],
                        "stop_reason": "tool_use"
                    }
                }
            }
        }],
        "usage": {
            "prompt_tokens": 100,
            "completion_tokens": 14,
            "total_tokens": 114,
            "prompt_tokens_details": { "cached_tokens": 70 }
        }
    });

    let stream = stream_view(RequestKindDto::Chat, &events);
    let document = response_view(RequestKindDto::Chat, &response);
    assert_eq!(stream.candidates, document.candidates);
    assert_eq!(
        stream.candidates,
        vec![MessageViewDto {
            role: "assistant".to_string(),
            name: None,
            path: None,
            parts: vec![
                PartViewDto::Reasoning {
                    text: "Plan.".to_string(),
                    path: None,
                },
                PartViewDto::NativeState {
                    kind: "signature".to_string(),
                    length: 6,
                    path: None,
                },
                PartViewDto::Other {
                    label: "server_tool_use".to_string(),
                    value: json!({
                        "type": "server_tool_use",
                        "id": "srvtoolu_1",
                        "name": "web_search",
                        "input": { "query": "tea" }
                    }),
                    path: None,
                },
                PartViewDto::ToolCall {
                    id: "toolu_1".to_string(),
                    name: "write_file".to_string(),
                    arguments: json!({ "path": "b.md" }),
                    arguments_text: None,
                    path: None,
                },
            ],
        }]
    );
    assert_eq!(stream.usage, document.usage);
    assert_eq!(
        stream.usage,
        Some(UsageViewDto {
            input: Some(100),
            cache_read: Some(70),
            output: Some(14),
            reasoning: None,
        })
    );
    // Streams show the provider's own finish reason; non-streamed responses the adapter's.
    assert_eq!(stream.finish_reason.as_deref(), Some("tool_use"));
}

#[test]
fn gemini_stream_merges_thoughts_and_native_content_like_the_response() {
    let events = [
        json!({"candidates":[{"content":{"role":"model","parts":[{"thought":true,"text":"Think "}]}}]}),
        json!({"candidates":[{"content":{"role":"model","parts":[{"thought":true,"text":"more.","thoughtSignature":"gsig"}]}}]}),
        json!({"candidates":[{"content":{"role":"model","parts":[{"functionCall":{"id":"fc_1","name":"search","args":{"q":"rust"}}}]},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":50,"cachedContentTokenCount":30,"candidatesTokenCount":12,"thoughtsTokenCount":6}}),
        json!({"choices":[{"index":0,"delta":{"native":{"gemini":{"content":{"role":"model","parts":[{"thought":true,"text":"Think more.","thoughtSignature":"gsig"},{"functionCall":{"id":"fc_1","name":"search","args":{"q":"rust"}}}]}}}}}]}),
    ];
    let response = json!({
        "choices": [{
            "index": 0,
            "finish_reason": "tool_calls",
            "message": {
                "role": "assistant",
                "content": "",
                "reasoning_content": "Think more.",
                "tool_calls": [{
                    "id": "fc_1",
                    "type": "function",
                    "function": { "name": "search", "arguments": "{\"q\":\"rust\"}" }
                }],
                "native": {
                    "gemini": {
                        "content": {
                            "role": "model",
                            "parts": [
                                { "thought": true, "text": "Think more.", "thoughtSignature": "gsig" },
                                { "functionCall": { "id": "fc_1", "name": "search", "args": { "q": "rust" } } }
                            ]
                        }
                    }
                }
            }
        }],
        "usage": {
            "prompt_tokens": 50,
            "completion_tokens": 12,
            "total_tokens": 62,
            "prompt_tokens_details": { "cached_tokens": 30 }
        }
    });

    // A stream the user stopped has no terminal `native` chunk and merges the same.
    for events in [&events[..], &events[..events.len() - 1]] {
        let stream = stream_view(RequestKindDto::Chat, events);
        let document = response_view(RequestKindDto::Chat, &response);
        assert_eq!(stream.candidates, document.candidates);
        assert_eq!(stream.candidates[0].role, "model");
        assert_eq!(
            stream.candidates[0].parts,
            vec![
                PartViewDto::Reasoning {
                    text: "Think more.".to_string(),
                    path: None,
                },
                PartViewDto::NativeState {
                    kind: "thoughtSignature".to_string(),
                    length: 4,
                    path: None,
                },
                PartViewDto::ToolCall {
                    id: "fc_1".to_string(),
                    name: "search".to_string(),
                    arguments: json!({ "q": "rust" }),
                    arguments_text: None,
                    path: None,
                },
            ]
        );
        // The adapter's non-streamed usage carries no thought tokens, so usage is not compared.
        assert_eq!(
            stream.usage,
            Some(UsageViewDto {
                input: Some(50),
                cache_read: Some(30),
                output: Some(12),
                reasoning: Some(6),
            })
        );
        assert_eq!(stream.finish_reason.as_deref(), Some("STOP"));
    }
}

#[test]
fn bedrock_text_frames_without_block_start_open_text_blocks() {
    let events = vec![
        json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello, "}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"world"}}),
    ];
    let response = json!({
        "choices": [{
            "index": 0,
            "finish_reason": "stop",
            "message": {
                "role": "assistant",
                "content": "Hello, world",
                "native": {
                    "claude": {
                        "content": [{ "type": "text", "text": "Hello, world" }]
                    }
                }
            }
        }]
    });

    let stream = stream_view(RequestKindDto::Chat, &events);
    let document = response_view(RequestKindDto::Chat, &response);
    assert_eq!(stream.candidates, document.candidates);
    assert_eq!(
        stream.candidates,
        vec![MessageViewDto {
            role: "assistant".to_string(),
            name: None,
            path: None,
            parts: vec![PartViewDto::Text {
                text: "Hello, world".to_string(),
                cache_breakpoint: false,
                path: None,
            }],
        }]
    );
}

#[test]
fn cohere_stream_merges_into_the_same_message_shape_as_the_response() {
    let events = vec![
        json!({"type":"message-start","id":"m1","delta":{"message":{"role":"assistant","content":[],"tool_plan":"","tool_calls":[],"citations":[]}}}),
        json!({"type":"content-start","index":0,"delta":{"message":{"content":{"type":"text","text":""}}}}),
        json!({"type":"content-delta","index":0,"delta":{"message":{"content":{"text":"Hel"}}}}),
        json!({"type":"content-delta","index":0,"delta":{"message":{"content":{"text":"lo"}}}}),
        json!({"type":"content-end","index":0}),
        json!({"type":"tool-plan-delta","delta":{"message":{"tool_plan":"I will "}}}),
        json!({"type":"tool-plan-delta","delta":{"message":{"tool_plan":"search."}}}),
        json!({"type":"tool-call-start","index":0,"delta":{"message":{"tool_calls":{"id":"call_1","type":"function","function":{"name":"search","arguments":""}}}}}),
        json!({"type":"tool-call-delta","index":0,"delta":{"message":{"tool_calls":{"function":{"arguments":"{\"q\":\"tea\"}"}}}}}),
        json!({"type":"tool-call-end","index":0}),
        json!({"type":"message-end","delta":{"finish_reason":"TOOL_CALL","usage":{"billed_units":{"input_tokens":7,"output_tokens":3},"tokens":{"input_tokens":9,"output_tokens":4}}}}),
    ];
    let response = json!({
        "id": "m1",
        "finish_reason": "TOOL_CALL",
        "message": {
            "role": "assistant",
            "tool_plan": "I will search.",
            "content": [{ "type": "text", "text": "Hello" }],
            "tool_calls": [{
                "id": "call_1",
                "type": "function",
                "function": { "name": "search", "arguments": "{\"q\":\"tea\"}" }
            }]
        },
        "usage": {
            "billed_units": { "input_tokens": 7, "output_tokens": 3 },
            "tokens": { "input_tokens": 9, "output_tokens": 4 }
        }
    });

    let stream = stream_view(RequestKindDto::Chat, &events);
    let document = response_view(RequestKindDto::Chat, &response);
    assert_eq!(stream.candidates, document.candidates);
    assert_eq!(
        stream.candidates[0].parts,
        vec![
            PartViewDto::Reasoning {
                text: "I will search.".to_string(),
                path: None,
            },
            PartViewDto::Text {
                text: "Hello".to_string(),
                cache_breakpoint: false,
                path: None,
            },
            PartViewDto::ToolCall {
                id: "call_1".to_string(),
                name: "search".to_string(),
                arguments: json!({ "q": "tea" }),
                arguments_text: None,
                path: None,
            },
        ]
    );
    assert_eq!(stream.usage, document.usage);
    assert_eq!(
        stream.usage,
        Some(UsageViewDto {
            input: Some(9),
            cache_read: None,
            output: Some(4),
            reasoning: None,
        })
    );
    assert_eq!(stream.finish_reason, document.finish_reason);
    assert_eq!(stream.finish_reason.as_deref(), Some("TOOL_CALL"));
}

#[test]
fn text_completion_streams_merge_candidates_by_index_like_the_response() {
    let events = vec![
        json!({"choices":[{"index":0,"text":"A","reasoning":"T"}]}),
        json!({"choices":[{"index":1,"text":"X"}]}),
        json!({"choices":[{"index":0,"text":"B","finish_reason":"stop"}],"usage":{"prompt_tokens":5,"completion_tokens":2}}),
    ];
    let response = json!({
        "choices": [
            { "index": 0, "text": "AB", "reasoning": "T", "finish_reason": "stop" },
            { "index": 1, "text": "X" }
        ],
        "usage": { "prompt_tokens": 5, "completion_tokens": 2 }
    });

    let stream = stream_view(RequestKindDto::Text, &events);
    let document = response_view(RequestKindDto::Text, &response);
    assert_eq!(stream.candidates, document.candidates);
    assert_eq!(stream.usage, document.usage);
    assert_eq!(
        render_response_readable(RequestKindDto::Text, &stream),
        "[reasoning]\nT\n\n[assistant]\nAB\n\nX"
    );
}

#[test]
fn llamacpp_bare_stream_outputs_merge_by_index_like_the_response() {
    let events = vec![
        json!({"index":0,"content":"He","stop":false}),
        json!({"index":1,"content":"Yo","stop":false}),
        json!({"index":0,"content":"y","stop":true,"stop_type":"eos"}),
    ];
    let response = json!([
        { "index": 0, "content": "Hey", "stop": true, "stop_type": "eos" },
        { "index": 1, "content": "Yo", "stop": false }
    ]);

    let stream = stream_view(RequestKindDto::Text, &events);
    let document = response_view(RequestKindDto::Text, &response);
    assert_eq!(stream.candidates, document.candidates);
    assert_eq!(stream.finish_reason.as_deref(), Some("eos"));
}

#[test]
fn text_completion_projects_and_renders_json_requests_and_outputs() {
    let request_payload = json!({
        "prompt": "Once upon a time",
        "max_tokens": 64
    });
    let request = project_request_view(RequestKindDto::Text, &request_payload);
    assert_eq!(request.kind, RequestKindDto::Text);
    assert_eq!(request.prompt.as_deref(), Some("Once upon a time"));
    assert_eq!(render_request_readable(&request), "Once upon a time");

    let llamacpp_multi = json!([
        { "content": "First", "thinking": "Step 1" },
        { "content": "Second" }
    ]);
    let multi_view = project_response_view(RequestKindDto::Text, &llamacpp_multi, Some(""));
    assert_eq!(
        render_response_readable(RequestKindDto::Text, &multi_view),
        "[reasoning]\nStep 1\n\n[assistant]\nFirst\n\nSecond"
    );

    let ollama_single = json!({ "response": "Hello", "thinking": "Plan" });
    let single_view = project_response_view(RequestKindDto::Text, &ollama_single, Some(""));
    assert_eq!(
        render_response_readable(RequestKindDto::Text, &single_view),
        "[reasoning]\nPlan\n\n[assistant]\nHello"
    );
}
