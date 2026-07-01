//! DeepSeek non-stream / stream chunk → Claude-shape projection.
//!
//! DeepSeek on Bedrock exposes two response shapes, switched by model id:
//!
//! - **Text-completion (R1)**: `{ "choices": [{ "text": "...", "stop_reason": "stop"|"length" }] }`.
//! - **Chat-completion (V3.1+ / V3.2)**: AWS model cards only show the
//!   request side and pretty-print the response via `json.loads(...)`. The
//!   most plausible shape (matching DeepSeek's own OpenAI-compatible API) is
//!   `{ "choices": [{ "message": { "role":"assistant","content":"..." }, "finish_reason":"stop" }] }`,
//!   but Bedrock could also keep the legacy `text` field. We probe both
//!   `choices[0].message.content` and `choices[0].text` defensively.
//!
//! Stream chunks mirror the same two flavours and are unified into Anthropic
//! `content_block_delta` text frames.

use serde_json::{Map, Value, json};

pub(super) fn response_to_claude_shape(body: Value) -> Value {
    let text = body
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            body.pointer("/choices/0/text")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_default();

    let stop_reason = body
        .pointer("/choices/0/finish_reason")
        .or_else(|| body.pointer("/choices/0/stop_reason"))
        .and_then(Value::as_str)
        .map(|value| match value {
            "length" => "max_tokens".to_string(),
            other => other.to_string(),
        })
        .unwrap_or_else(|| "end_turn".to_string());

    let mut usage = Map::new();
    if let Some(usage_value) = body.get("usage").and_then(Value::as_object) {
        if let Some(input_tokens) = usage_value
            .get("prompt_tokens")
            .or_else(|| usage_value.get("input_tokens"))
            .and_then(Value::as_u64)
        {
            usage.insert("input_tokens".to_string(), json!(input_tokens));
        }
        if let Some(output_tokens) = usage_value
            .get("completion_tokens")
            .or_else(|| usage_value.get("output_tokens"))
            .and_then(Value::as_u64)
        {
            usage.insert("output_tokens".to_string(), json!(output_tokens));
        }
    }

    let mut claude_body = Map::new();
    claude_body.insert(
        "content".to_string(),
        Value::Array(vec![json!({ "type": "text", "text": text })]),
    );
    claude_body.insert("stop_reason".to_string(), Value::String(stop_reason));
    if !usage.is_empty() {
        claude_body.insert("usage".to_string(), Value::Object(usage));
    }
    Value::Object(claude_body)
}

/// DeepSeek stream chunks (per the published R1 schema and the Bedrock model
/// card examples) come in two flavours:
///
/// - Text-completion (R1): `{ "choices": [{ "text": "...", "stop_reason": null|"stop" }] }`
/// - Chat-completion (V3.1+): `{ "choices": [{ "delta": { "content": "..." } }] }`
///   (OpenAI-compatible — same as DeepSeek's own /v1/chat/completions stream).
///
/// We probe each one and rewrap the extracted text as an Anthropic
/// `content_block_delta`. Terminal frames without user-visible text (empty
/// `text` / empty `delta.content`) are silently dropped so the frontend never
/// re-renders a blank tail.
pub(super) fn transform_chunk_to_anthropic(decoded: &str) -> Option<String> {
    let value: Value = serde_json::from_str(decoded).ok()?;
    let text = value
        .pointer("/choices/0/delta/content")
        .and_then(Value::as_str)
        .or_else(|| value.pointer("/choices/0/text").and_then(Value::as_str))?;

    if text.is_empty() {
        return None;
    }

    Some(
        json!({
            "type": "content_block_delta",
            "index": 0,
            "delta": { "type": "text_delta", "text": text },
        })
        .to_string(),
    )
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::{response_to_claude_shape, transform_chunk_to_anthropic};

    #[test]
    fn transform_chunk_to_anthropic_handles_text_and_delta_shapes() {
        let r1 = json!({
            "choices": [{ "text": " hello", "stop_reason": null }],
        })
        .to_string();
        let v3 = json!({
            "choices": [{ "delta": { "content": " world" } }],
        })
        .to_string();

        for (chunk, expected) in [(r1, " hello"), (v3, " world")] {
            let rewritten = transform_chunk_to_anthropic(&chunk)
                .unwrap_or_else(|| panic!("expected delta from chunk: {chunk}"));
            let parsed: Value = serde_json::from_str(&rewritten).unwrap();
            assert_eq!(parsed["type"], "content_block_delta");
            assert_eq!(parsed["delta"]["text"], expected);
        }
    }

    #[test]
    fn transform_chunk_to_anthropic_drops_terminal_empty_text() {
        let chunk = json!({
            "choices": [{ "text": "", "stop_reason": "stop" }],
        })
        .to_string();
        assert!(transform_chunk_to_anthropic(&chunk).is_none());
    }

    #[test]
    fn response_to_claude_shape_lifts_text_and_token_counts() {
        let r1 = json!({
            "choices": [{ "text": "r1 reply", "stop_reason": "length" }],
            "usage": { "prompt_tokens": 10, "completion_tokens": 5 },
        });
        let r1_shape = response_to_claude_shape(r1);
        assert_eq!(r1_shape["content"][0]["text"], "r1 reply");
        assert_eq!(r1_shape["stop_reason"], "max_tokens");
        assert_eq!(r1_shape["usage"]["input_tokens"], 10);
        assert_eq!(r1_shape["usage"]["output_tokens"], 5);

        let v3 = json!({
            "choices": [{
                "message": { "role": "assistant", "content": "v3 reply" },
                "finish_reason": "stop"
            }]
        });
        let v3_shape = response_to_claude_shape(v3);
        assert_eq!(v3_shape["content"][0]["text"], "v3 reply");
        assert_eq!(v3_shape["stop_reason"], "stop");
    }
}
