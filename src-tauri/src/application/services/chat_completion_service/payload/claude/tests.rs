use serde_json::{Value, json};

use super::build;

fn claude_payload(model: &str) -> serde_json::Map<String, Value> {
    json!({
        "model": model,
        "messages": [{"role": "user", "content": "hello"}],
    })
    .as_object()
    .cloned()
    .expect("payload must be object")
}

#[test]
fn claude_manual_reasoning_uses_legacy_thinking_and_clears_sampling() {
    let mut payload = claude_payload("claude-sonnet-4-5");
    payload.insert("max_tokens".to_string(), json!(1000));
    payload.insert("reasoning_effort".to_string(), json!("medium"));
    payload.insert("temperature".to_string(), json!(0.7));
    payload.insert("top_k".to_string(), json!(40));
    payload.insert("stream".to_string(), json!(false));

    let (_, upstream) = build(payload).expect("build should succeed");
    let body = upstream.as_object().expect("body must be object");

    assert_eq!(
        body.get("max_tokens")
            .and_then(Value::as_i64)
            .unwrap_or_default(),
        2024
    );
    assert_eq!(
        body.get("thinking")
            .and_then(Value::as_object)
            .and_then(|thinking| thinking.get("budget_tokens"))
            .and_then(Value::as_i64)
            .unwrap_or_default(),
        1024
    );
    assert_eq!(
        body.get("thinking")
            .and_then(Value::as_object)
            .and_then(|thinking| thinking.get("type"))
            .and_then(Value::as_str)
            .unwrap_or_default(),
        "enabled"
    );
    assert!(body.get("temperature").is_none());
    assert!(body.get("top_p").is_none());
    assert!(body.get("top_k").is_none());
}

#[test]
fn claude_rejects_assistant_prefill_for_models_that_removed_it() {
    for model in ["claude-opus-4-7", "claude-opus-4-6", "claude-sonnet-4-6"] {
        let mut payload = claude_payload(model);
        payload.insert("assistant_prefill".to_string(), json!("prefill"));

        let error = build(payload).expect_err("build should fail");
        let message = error.to_string();

        assert!(
            message.contains("does not support assistant_prefill"),
            "{model} should reject assistant_prefill, got: {message}"
        );
    }
}

#[test]
fn claude_rejects_assistant_prefill_with_reasoning_effort() {
    let mut payload = claude_payload("claude-sonnet-4-5");
    payload.insert("assistant_prefill".to_string(), json!("prefill"));
    payload.insert("reasoning_effort".to_string(), json!("medium"));

    let error = build(payload).expect_err("build should fail");
    assert!(
        error
            .to_string()
            .contains("does not support assistant_prefill with reasoning_effort")
    );
}

#[test]
fn claude_use_sysprompt_collects_only_leading_system_messages() {
    let payload = json!({
        "model": "claude-3-5-sonnet-latest",
        "use_sysprompt": true,
        "messages": [
            {"role": "system", "content": "s1"},
            {"role": "system", "content": "s2"},
            {"role": "user", "content": "u1"},
            {"role": "system", "content": "late system"},
            {"role": "user", "content": "u2"}
        ]
    })
    .as_object()
    .cloned()
    .expect("payload must be object");

    let (_, upstream) = build(payload).expect("build should succeed");
    let body = upstream.as_object().expect("body must be object");

    let system = body
        .get("system")
        .and_then(Value::as_array)
        .expect("system must be array");
    assert_eq!(system.len(), 2);
    assert_eq!(system[0]["text"].as_str().unwrap_or_default(), "s1");
    assert_eq!(system[1]["text"].as_str().unwrap_or_default(), "s2");

    let messages = body
        .get("messages")
        .and_then(Value::as_array)
        .expect("messages must be array");
    let joined = messages
        .iter()
        .filter_map(|message| message.get("content").and_then(Value::as_array))
        .flat_map(|parts| parts.iter())
        .filter_map(|part| part.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(joined.contains("late system"));
}

#[test]
fn claude_system_messages_become_user_when_use_sysprompt_false() {
    let payload = json!({
        "model": "claude-3-5-sonnet-latest",
        "use_sysprompt": false,
        "messages": [
            {"role": "system", "content": "s1"},
            {"role": "user", "content": "u1"}
        ]
    })
    .as_object()
    .cloned()
    .expect("payload must be object");

    let (_, upstream) = build(payload).expect("build should succeed");
    let body = upstream.as_object().expect("body must be object");

    assert!(body.get("system").is_none());

    let messages = body
        .get("messages")
        .and_then(Value::as_array)
        .expect("messages must be array");
    let joined = messages
        .iter()
        .filter_map(|message| message.get("content").and_then(Value::as_array))
        .flat_map(|parts| parts.iter())
        .filter_map(|part| part.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(joined.contains("s1"));
    assert!(joined.contains("u1"));
}

#[test]
fn claude_tool_calls_and_results_are_structured() {
    let payload = json!({
        "model": "claude-3-5-sonnet-latest",
        "messages": [
            {
                "role": "assistant",
                "tool_calls": [{
                    "id": "call_weather",
                    "type": "function",
                    "function": {
                        "name": "weather",
                        "arguments": "{\"city\":\"Paris\"}"
                    }
                }]
            },
            {
                "role": "tool",
                "tool_call_id": "call_weather",
                "content": "{\"temperature\":20}"
            }
        ],
        "tools": [{
            "type": "function",
            "function": {
                "name": "weather",
                "description": "get weather",
                "parameters": { "type": "object", "properties": {} }
            }
        }]
    })
    .as_object()
    .cloned()
    .expect("payload must be object");

    let (_, upstream) = build(payload).expect("build should succeed");
    let body = upstream.as_object().expect("body must be object");
    let messages = body
        .get("messages")
        .and_then(Value::as_array)
        .expect("messages must be array");

    let assistant_blocks = messages
        .first()
        .and_then(Value::as_object)
        .and_then(|message| message.get("content"))
        .and_then(Value::as_array)
        .expect("assistant content must be array");
    assert_eq!(
        assistant_blocks[0]["type"].as_str().unwrap_or_default(),
        "tool_use"
    );
    assert_eq!(
        assistant_blocks[0]["id"].as_str().unwrap_or_default(),
        "call_weather"
    );
    assert_eq!(
        assistant_blocks[0]["name"].as_str().unwrap_or_default(),
        "weather"
    );

    let tool_result_block = messages
        .get(1)
        .and_then(Value::as_object)
        .and_then(|message| message.get("content"))
        .and_then(Value::as_array)
        .and_then(|parts| parts.first())
        .and_then(Value::as_object)
        .expect("tool result block must be object");
    assert_eq!(
        tool_result_block
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        "tool_result"
    );
    assert_eq!(
        tool_result_block
            .get("tool_use_id")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        "call_weather"
    );
}

#[test]
fn claude_tool_calls_are_text_when_tools_disabled() {
    let payload = json!({
        "model": "claude-3-5-sonnet-latest",
        "messages": [
            {
                "role": "assistant",
                "tool_calls": [{
                    "id": "call_weather",
                    "type": "function",
                    "function": {
                        "name": "weather",
                        "arguments": "{\"city\":\"Paris\"}"
                    }
                }]
            },
            {
                "role": "tool",
                "tool_call_id": "call_weather",
                "content": "{\"temperature\":20}"
            }
        ]
    })
    .as_object()
    .cloned()
    .expect("payload must be object");

    let (_, upstream) = build(payload).expect("build should succeed");
    let body = upstream.as_object().expect("body must be object");
    let messages = body
        .get("messages")
        .and_then(Value::as_array)
        .expect("messages must be array");

    let assistant_blocks = messages
        .first()
        .and_then(Value::as_object)
        .and_then(|message| message.get("content"))
        .and_then(Value::as_array)
        .expect("assistant content must be array");
    assert_eq!(
        assistant_blocks[0]["type"].as_str().unwrap_or_default(),
        "text"
    );

    let tool_blocks = messages
        .get(1)
        .and_then(Value::as_object)
        .and_then(|message| message.get("content"))
        .and_then(Value::as_array)
        .expect("tool content must be array");
    assert_eq!(tool_blocks[0]["type"].as_str().unwrap_or_default(), "text");
}

#[test]
fn claude_converts_openai_image_url_blocks() {
    let payload = json!({
        "model": "claude-3-5-sonnet-latest",
        "messages": [{
            "role": "user",
            "content": [
                { "type": "text", "text": "describe" },
                { "type": "image_url", "image_url": { "url": "data:image/png;base64,AAAA" } }
            ]
        }]
    })
    .as_object()
    .cloned()
    .expect("payload must be object");

    let (_, upstream) = build(payload).expect("build should succeed");
    let body = upstream.as_object().expect("body must be object");
    let messages = body
        .get("messages")
        .and_then(Value::as_array)
        .expect("messages must be array");

    let content = messages[0]
        .get("content")
        .and_then(Value::as_array)
        .expect("message content must be array");

    assert_eq!(content[0]["type"].as_str().unwrap_or_default(), "text");
    assert_eq!(content[0]["text"].as_str().unwrap_or_default(), "describe");

    assert_eq!(content[1]["type"].as_str().unwrap_or_default(), "image");
    assert_eq!(
        content[1]["source"]["type"].as_str().unwrap_or_default(),
        "base64"
    );
    assert_eq!(
        content[1]["source"]["media_type"]
            .as_str()
            .unwrap_or_default(),
        "image/png"
    );
    assert_eq!(
        content[1]["source"]["data"].as_str().unwrap_or_default(),
        "AAAA"
    );
}

#[test]
fn claude_moves_images_out_of_assistant_messages() {
    let payload = json!({
        "model": "claude-3-5-sonnet-latest",
        "messages": [
            {
                "role": "assistant",
                "content": [
                    { "type": "text", "text": "here" },
                    { "type": "image_url", "image_url": { "url": "data:image/png;base64,AAAA" } }
                ]
            },
            { "role": "user", "content": "ok" }
        ]
    })
    .as_object()
    .cloned()
    .expect("payload must be object");

    let (_, upstream) = build(payload).expect("build should succeed");
    let body = upstream.as_object().expect("body must be object");
    let messages = body
        .get("messages")
        .and_then(Value::as_array)
        .expect("messages must be array");

    let assistant_content = messages[0]
        .get("content")
        .and_then(Value::as_array)
        .expect("assistant content must be array");
    assert!(
        !assistant_content
            .iter()
            .any(|block| block.get("type").and_then(Value::as_str) == Some("image"))
    );

    let user_content = messages[1]
        .get("content")
        .and_then(Value::as_array)
        .expect("user content must be array");
    assert!(
        user_content
            .iter()
            .any(|block| block.get("type").and_then(Value::as_str) == Some("image"))
    );
}

#[test]
fn claude_limited_sampling_models_reject_temperature_and_top_p_together() {
    let mut payload = claude_payload("claude-sonnet-4-5");
    payload.insert("temperature".to_string(), json!(0.7));
    payload.insert("top_p".to_string(), json!(0.9));

    let error = build(payload).expect_err("build should fail");
    assert!(
        error
            .to_string()
            .contains("accepts either temperature or top_p, not both")
    );
}

#[test]
fn claude_full_sampling_models_keep_temperature_top_p_and_top_k() {
    let mut payload = claude_payload("claude-3-5-sonnet-latest");
    payload.insert("temperature".to_string(), json!(0.7));
    payload.insert("top_p".to_string(), json!(0.9));
    payload.insert("top_k".to_string(), json!(40));

    let (_, upstream) = build(payload).expect("build should succeed");
    let body = upstream.as_object().expect("body must be object");

    assert!(body.get("temperature").is_some());
    assert!(body.get("top_p").is_some());
    assert!(body.get("top_k").is_some());
}

#[test]
fn claude_sampling_free_models_reject_non_default_sampling_params() {
    let mut payload = claude_payload("claude-opus-4-7");
    payload.insert("temperature".to_string(), json!(0.7));

    let error = build(payload).expect_err("build should fail");
    assert!(
        error
            .to_string()
            .contains("does not support non-default sampling parameters: temperature")
    );
}

#[test]
fn claude_sampling_free_models_ignore_default_sampling_params() {
    let mut payload = claude_payload("claude-opus-4-7");
    payload.insert("temperature".to_string(), json!(1.0));
    payload.insert("top_p".to_string(), json!(1.0));
    payload.insert("top_k".to_string(), json!(0));

    let (_, upstream) = build(payload).expect("build should succeed");
    let body = upstream.as_object().expect("body must be object");

    assert!(body.get("temperature").is_none());
    assert!(body.get("top_p").is_none());
    assert!(body.get("top_k").is_none());
}

#[test]
fn claude_future_models_do_not_inherit_sampling_or_prefill_support() {
    let mut sampling_payload = claude_payload("claude-opus-4-8");
    sampling_payload.insert("temperature".to_string(), json!(0.7));

    let sampling_error = build(sampling_payload).expect_err("build should fail");
    assert!(
        sampling_error
            .to_string()
            .contains("does not support non-default sampling parameters: temperature")
    );

    let mut prefill_payload = claude_payload("claude-opus-4-8");
    prefill_payload.insert("assistant_prefill".to_string(), json!("prefill"));

    let prefill_error = build(prefill_payload).expect_err("build should fail");
    assert!(
        prefill_error
            .to_string()
            .contains("does not support assistant_prefill")
    );
}

#[test]
fn claude_future_models_do_not_inherit_legacy_or_adaptive_reasoning_support() {
    let mut payload = claude_payload("claude-opus-4-8");
    payload.insert("reasoning_effort".to_string(), json!("medium"));

    let error = build(payload).expect_err("build should fail");
    assert!(
        error
            .to_string()
            .contains("does not support reasoning_effort")
    );
}

#[test]
fn claude_adaptive_reasoning_uses_adaptive_thinking_and_effort() {
    let mut payload = claude_payload("claude-opus-4-7");
    payload.insert("reasoning_effort".to_string(), json!("high"));
    payload.insert("include_reasoning".to_string(), json!(true));

    let (_, upstream) = build(payload).expect("build should succeed");
    let body = upstream.as_object().expect("body must be object");

    assert_eq!(
        body.get("thinking")
            .and_then(Value::as_object)
            .and_then(|thinking| thinking.get("type"))
            .and_then(Value::as_str)
            .unwrap_or_default(),
        "adaptive"
    );
    assert_eq!(
        body.get("thinking")
            .and_then(Value::as_object)
            .and_then(|thinking| thinking.get("display"))
            .and_then(Value::as_str)
            .unwrap_or_default(),
        "summarized"
    );
    assert_eq!(
        body.get("output_config")
            .and_then(Value::as_object)
            .and_then(|config| config.get("effort"))
            .and_then(Value::as_str)
            .unwrap_or_default(),
        "high"
    );
    assert!(
        body.get("thinking")
            .and_then(Value::as_object)
            .and_then(|thinking| thinking.get("budget_tokens"))
            .is_none()
    );
}

#[test]
fn claude_manual_or_adaptive_models_accept_legacy_thinking_overrides() {
    let request = json!({
        "model": "claude-opus-4-6",
        "messages": [{"role": "user", "content": [{"type": "text", "text": "hello"}]}],
        "thinking": {
            "type": "enabled",
            "budget_tokens": 2048
        },
        "max_tokens": 4096
    });

    super::validate_request(&request).expect("request should be valid");
}
