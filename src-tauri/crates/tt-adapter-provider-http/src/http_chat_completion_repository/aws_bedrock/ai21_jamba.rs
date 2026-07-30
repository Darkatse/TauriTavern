//! AI21 Jamba non-stream / stream chunk → Claude-shape projection.
//!
//! AI21 Jamba non-stream responses (per AI21 chat-completion spec /
//! Bedrock model card `model-parameters-jamba.md`):
//!
//! ```json
//! { "id": "...", "choices": [{ "index": 0,
//!     "message": { "role": "assistant", "content": "..." },
//!     "finish_reason": "stop|length|content_filter" }],
//!   "usage": { "prompt_tokens": N, "completion_tokens": M } }
//! ```
//!
//! Streams are OpenAI-shape with the usual `delta` envelope; only chunks that
//! carry user-visible text on `choices[0].delta.content` are forwarded.

use serde_json::{Map, Value, json};

/// Lift `choices[0].message.content` into a Claude-shaped `content[].text`
/// block, project Jamba's `finish_reason` (`length` -> `max_tokens`, `stop` ->
/// `end_turn`, `content_filter` -> `content_filter`), and forward
/// `usage.prompt_tokens` / `usage.completion_tokens`.
pub(super) fn response_to_claude_shape(body: Value) -> Value {
    let text = body
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_default();

    let stop_reason = body
        .pointer("/choices/0/finish_reason")
        .and_then(Value::as_str)
        .map(|value| match value {
            "length" => "max_tokens".to_string(),
            "stop" => "end_turn".to_string(),
            other => other.to_string(),
        })
        .unwrap_or_else(|| "end_turn".to_string());

    let mut usage = Map::new();
    if let Some(usage_value) = body.get("usage").and_then(Value::as_object) {
        if let Some(input_tokens) = usage_value.get("prompt_tokens").and_then(Value::as_u64) {
            usage.insert("input_tokens".to_string(), json!(input_tokens));
        }
        if let Some(output_tokens) = usage_value.get("completion_tokens").and_then(Value::as_u64) {
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

/// AI21 Jamba stream chunks are OpenAI-shape:
///
/// ```json
/// { "id": "...", "choices": [{
///     "index": 0,
///     "delta": { "role": "assistant", "content": "..." },
///     "finish_reason": null | "stop" | "length"
/// }] }
/// ```
///
/// The first chunk carries `delta.role = "assistant"` with no content; the
/// terminal chunk has `delta = {}` (or empty content) and a `usage` summary.
/// We extract `choices[0].delta.content`, drop empty or sentinel chunks, and
/// rewrap as an Anthropic `content_block_delta` text frame.
pub(super) fn transform_chunk_to_anthropic(decoded: &str) -> Option<String> {
    let value: Value = serde_json::from_str(decoded).ok()?;
    let text = value
        .pointer("/choices/0/delta/content")
        .and_then(Value::as_str)?;

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
    fn transform_chunk_to_anthropic_extracts_delta_content() {
        let chunk = json!({
            "id": "abc",
            "choices": [{
                "index": 0,
                "delta": { "role": "assistant", "content": " jamba" },
                "finish_reason": null,
            }],
        })
        .to_string();

        let rewritten =
            transform_chunk_to_anthropic(&chunk).expect("delta with text must surface a frame");
        let parsed: Value = serde_json::from_str(&rewritten).unwrap();
        assert_eq!(parsed["type"], "content_block_delta");
        assert_eq!(parsed["delta"]["text"], " jamba");
    }

    #[test]
    fn transform_chunk_to_anthropic_drops_role_only_chunks() {
        let chunk = json!({
            "choices": [{
                "delta": { "role": "assistant" },
                "finish_reason": null,
            }],
        })
        .to_string();
        assert!(transform_chunk_to_anthropic(&chunk).is_none());

        let empty_content = json!({
            "choices": [{
                "delta": { "role": "assistant", "content": "" },
                "finish_reason": "stop",
            }],
        })
        .to_string();
        assert!(transform_chunk_to_anthropic(&empty_content).is_none());
    }

    #[test]
    fn response_to_claude_shape_lifts_message_and_finish_reason() {
        let body = json!({
            "id": "abc",
            "choices": [{
                "index": 0,
                "message": { "role": "assistant", "content": "jamba reply" },
                "finish_reason": "length"
            }],
            "usage": { "prompt_tokens": 12, "completion_tokens": 3 }
        });
        let shape = response_to_claude_shape(body);
        assert_eq!(shape["content"][0]["text"], "jamba reply");
        assert_eq!(shape["stop_reason"], "max_tokens");
        assert_eq!(shape["usage"]["input_tokens"], 12);
        assert_eq!(shape["usage"]["output_tokens"], 3);

        let stop = json!({
            "choices": [{
                "message": { "content": "" },
                "finish_reason": "stop"
            }]
        });
        assert_eq!(
            response_to_claude_shape(stop)["stop_reason"],
            "end_turn",
            "stop must map to Claude end_turn",
        );
    }
}
