use serde_json::json;

use super::*;
use tt_ports::repositories::chat_completion_repository::{
    CHAT_COMPLETION_PROVIDER_STATE_FIELD, ChatCompletionSource,
};

#[test]
fn format_endpoint_strips_userinfo_but_keeps_path() {
    assert_eq!(
        format_endpoint("https://user:pass@example.com/v1", "/messages"),
        "https://example.com/v1/messages"
    );
}

#[test]
fn format_request_readable_shows_tools_tool_calls_and_results() {
    let payload = json!({
        "model": "gpt-5",
        "tool_choice": "auto",
        "tools": [{
            "type": "function",
            "function": {
                "name": "workspace_write_file",
                "parameters": { "type": "object" }
            }
        }],
        "messages": [
            { "role": "system", "content": "sys" },
            { "role": "assistant", "content": null, "tool_calls": [{
                "id": "call_1",
                "type": "function",
                "function": {
                    "name": "workspace_write_file",
                    "arguments": "{\"path\":\"output/main.md\",\"content\":\"first line\\nsecond line\"}"
                }
            }]},
            {
                "role": "tool",
                "tool_call_id": "call_1",
                "name": "workspace_write_file",
                "content": "{\"ok\":true,\"content\":\"Wrote 2 bytes.\"}"
            }
        ]
    });

    let readable = format_request_readable(ChatCompletionSource::Custom, &payload);

    assert_eq!(
        readable,
        "[tools count=1 tool_choice=auto]\n- workspace_write_file\n\n[system]\nsys\n\n[assistant]\n[tool_call id=call_1 name=workspace_write_file]\n{\"path\":\"output/main.md\"}\n[content]\nfirst line\nsecond line\n\n[tool id=call_1 name=workspace_write_file]\n{\"ok\":true,\"content\":\"Wrote 2 bytes.\"}"
    );
}

#[test]
fn format_response_readable_shows_openai_tool_calls() {
    let response = json!({
        "choices": [{
            "message": {
                "role": "assistant",
                "content": null,
                "tool_calls": [{
                    "id": "call_1",
                    "type": "function",
                    "function": {
                        "name": "workspace_commit",
                        "arguments": "{\"path\":\"output/main.md\"}"
                    }
                }]
            }
        }]
    });

    let readable = format_response_readable(&response);

    assert_eq!(
        readable,
        "[assistant]\n[tool_call id=call_1 name=workspace_commit]\n{\"path\":\"output/main.md\"}"
    );
}

#[test]
fn wire_log_payload_strips_internal_provider_state() {
    let mut payload = json!({
        "model": "gpt-5",
        "input": [{ "role": "user", "content": "hi" }]
    });
    payload.as_object_mut().unwrap().insert(
        CHAT_COMPLETION_PROVIDER_STATE_FIELD.to_string(),
        json!({
            "sessionId": "run_123",
            "previousResponseId": "resp_123"
        }),
    );

    let payload = wire_log_payload(&payload);

    assert!(payload.get(CHAT_COMPLETION_PROVIDER_STATE_FIELD).is_none());
    assert!(!pretty_json(&payload).contains(CHAT_COMPLETION_PROVIDER_STATE_FIELD));
    assert_eq!(
        format_request_readable(ChatCompletionSource::Custom, &payload),
        "[user]\nhi"
    );
}

#[test]
fn wire_log_payload_redacts_inline_media_without_mutating_source() {
    let payload = json!({
        "messages": [{
            "role": "user",
            "content": [{
                "type": "image_url",
                "image_url": { "url": "data:image/png;base64,AAAA" }
            }]
        }],
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

    let sanitized = wire_log_payload(&payload);

    assert_eq!(
        sanitized.pointer("/messages/0/content/0/image_url/url"),
        Some(&json!(
            "<inline media omitted; mime=image/png; base64_chars=4>"
        ))
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
        payload.pointer("/messages/0/content/0/image_url/url"),
        Some(&json!("data:image/png;base64,AAAA"))
    );
    assert!(!pretty_json(&sanitized).contains("AAAA"));
    assert!(!pretty_json(&sanitized).contains("BBBBB"));
    assert!(!pretty_json(&sanitized).contains("CCCCCC"));
}

#[test]
fn format_request_readable_supports_claude_tool_blocks() {
    let payload = json!({
        "system": "sys",
        "messages": [
            { "role": "user", "content": "hi" },
            { "role": "assistant", "content": [
                { "type": "text", "text": "checking" },
                { "type": "tool_use", "id": "toolu_1", "name": "workspace_read_file", "input": { "path": "output/main.md" } }
            ]},
            { "role": "user", "content": [
                { "type": "tool_result", "tool_use_id": "toolu_1", "content": "missing", "is_error": true }
            ]}
        ]
    });

    let readable = format_request_readable(ChatCompletionSource::Claude, &payload);

    assert_eq!(
        readable,
        "[system]\nsys\n\n[user]\nhi\n\n[assistant]\nchecking\n[tool_use id=toolu_1 name=workspace_read_file]\n{\"path\":\"output/main.md\"}\n\n[user]\n[tool_result tool_use_id=toolu_1 is_error=true]\nmissing"
    );
}

#[test]
fn format_response_readable_supports_claude_tool_use() {
    let response = json!({
        "id": "msg_1",
        "content": [
            { "type": "thinking", "text": "Need file", "signature": "opaque" },
            { "type": "tool_use", "id": "toolu_1", "name": "workspace_list_files", "input": { "path": "output" } }
        ]
    });

    let readable = format_response_readable(&response);

    assert_eq!(
        readable,
        "[assistant]\n[thinking native_state=present]\nNeed file\n[tool_use id=toolu_1 name=workspace_list_files]\n{\"path\":\"output\"}"
    );
}

#[test]
fn stream_readable_collector_separates_openai_reasoning_delta() {
    let mut collector = StreamReadableCollector::new(ChatCompletionSource::OpenAi);

    collector.push(r#"{"choices":[{"delta":{"reasoning_content":"Need "}}]}"#);
    collector.push(r#"{"choices":[{"delta":{"reasoning_content":"file."}}]}"#);
    collector.push(r#"{"choices":[{"delta":{"content":"Done."}}]}"#);

    assert_eq!(
        collector.into_string(),
        "[reasoning]\nNeed file.\n\n[assistant]\nDone."
    );
}

#[test]
fn stream_readable_collector_separates_claude_thinking_delta() {
    let readable_source = stream_readable_source(ChatCompletionSource::Custom, "/messages");
    let mut collector = StreamReadableCollector::new(readable_source);

    collector.push(
        r#"{"type":"content_block_delta","delta":{"type":"thinking_delta","thinking":"Need file."}}"#,
    );
    collector
        .push(r#"{"type":"content_block_delta","delta":{"type":"text_delta","text":"Done."}}"#);

    assert_eq!(
        collector.into_string(),
        "[reasoning]\nNeed file.\n\n[assistant]\nDone."
    );
}

#[test]
fn stream_readable_collector_separates_gemini_thought_parts() {
    let mut collector = StreamReadableCollector::new(ChatCompletionSource::Makersuite);

    collector
        .push(r#"{"candidates":[{"content":{"parts":[{"thought":true,"text":"Need file."}]}}]}"#);
    collector.push(r#"{"candidates":[{"content":{"parts":[{"text":"Done."}]}}]}"#);

    assert_eq!(
        collector.into_string(),
        "[reasoning]\nNeed file.\n\n[assistant]\nDone."
    );
}
