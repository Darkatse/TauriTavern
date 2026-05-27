//! Amazon Nova non-stream / stream chunk → Claude-shape projection.
//!
//! Nova on Bedrock always speaks the Converse-style schema, even on the
//! `/model/{id}/invoke` and `/invoke-with-response-stream` endpoints. We
//! pivot both the non-stream response and the EventStream frames into the
//! Anthropic-flavoured envelopes that the parent module already
//! understands, so the OpenAI projection the frontend sees is identical to
//! the Anthropic path.

use serde_json::{Map, Value, json};

/// Reshape an Amazon Nova non-stream `invoke` response into the Claude-style
/// `{ content: [...], stop_reason, usage }` envelope that
/// `normalize_claude_response` already understands. Doing the translation here
/// (rather than building a parallel normalizer) keeps the OpenAI-shape choice
/// the frontend sees identical to the Claude path.
///
/// Nova's response shape (Converse-style even when called via `/invoke`):
/// ```json
/// {
///   "output": { "message": { "role": "assistant", "content": [{ "text": "..." }] } },
///   "stopReason": "end_turn",
///   "usage": { "inputTokens": N, "outputTokens": M, "totalTokens": N+M }
/// }
/// ```
pub(super) fn response_to_claude_shape(body: Value) -> Value {
    let mut content_blocks: Vec<Value> = Vec::new();
    if let Some(parts) = body
        .pointer("/output/message/content")
        .and_then(Value::as_array)
    {
        for part in parts {
            if let Some(text) = part.get("text").and_then(Value::as_str) {
                content_blocks.push(json!({ "type": "text", "text": text }));
            }
        }
    }

    let stop_reason = body
        .get("stopReason")
        .and_then(Value::as_str)
        .unwrap_or("end_turn")
        .to_string();

    let mut usage = Map::new();
    if let Some(usage_value) = body.get("usage").and_then(Value::as_object) {
        if let Some(input_tokens) = usage_value
            .get("inputTokens")
            .or_else(|| usage_value.get("input_tokens"))
            .and_then(Value::as_u64)
        {
            usage.insert("input_tokens".to_string(), json!(input_tokens));
        }
        if let Some(output_tokens) = usage_value
            .get("outputTokens")
            .or_else(|| usage_value.get("output_tokens"))
            .and_then(Value::as_u64)
        {
            usage.insert("output_tokens".to_string(), json!(output_tokens));
        }
    }

    let mut claude_body = Map::new();
    claude_body.insert("content".to_string(), Value::Array(content_blocks));
    claude_body.insert("stop_reason".to_string(), Value::String(stop_reason));
    if !usage.is_empty() {
        claude_body.insert("usage".to_string(), Value::Object(usage));
    }
    Value::Object(claude_body)
}

/// Nova streams Converse-style events. Each EventStream `bytes` chunk decodes
/// into one of:
///   - `{ "messageStart": { "role": "assistant" } }`
///   - `{ "contentBlockStart": { "start": {...}, "contentBlockIndex": 0 } }`
///   - `{ "contentBlockDelta": { "delta": { "text": "..." }, "contentBlockIndex": 0 } }`
///   - `{ "contentBlockStop": { "contentBlockIndex": 0 } }`
///   - `{ "messageStop": { "stopReason": "end_turn" } }`
///   - `{ "metadata": { "usage": {...} } }`
///
/// Only the `contentBlockDelta.delta.text` payload carries user-visible text;
/// every other event is dropped. The delta is rewrapped as an Anthropic
/// `content_block_delta` text frame.
pub(super) fn transform_chunk_to_anthropic(decoded: &str) -> Option<String> {
    let value: Value = serde_json::from_str(decoded).ok()?;
    let delta_text = value
        .pointer("/contentBlockDelta/delta/text")
        .and_then(Value::as_str)?;
    if delta_text.is_empty() {
        return None;
    }
    let index = value
        .pointer("/contentBlockDelta/contentBlockIndex")
        .and_then(Value::as_u64)
        .unwrap_or(0);

    Some(
        json!({
            "type": "content_block_delta",
            "index": index,
            "delta": { "type": "text_delta", "text": delta_text },
        })
        .to_string(),
    )
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::{response_to_claude_shape, transform_chunk_to_anthropic};

    #[test]
    fn transform_chunk_to_anthropic_extracts_content_block_delta_text() {
        let chunk = json!({
            "contentBlockDelta": {
                "delta": { "text": "Hello, " },
                "contentBlockIndex": 0
            }
        })
        .to_string();
        let rewritten =
            transform_chunk_to_anthropic(&chunk).expect("delta frames must surface text");
        let parsed: Value = serde_json::from_str(&rewritten).unwrap();
        assert_eq!(parsed["type"], "content_block_delta");
        assert_eq!(parsed["index"], 0);
        assert_eq!(parsed["delta"]["type"], "text_delta");
        assert_eq!(parsed["delta"]["text"], "Hello, ");
    }

    #[test]
    fn transform_chunk_to_anthropic_drops_non_text_envelopes() {
        let cases = [
            json!({ "messageStart": { "role": "assistant" } }).to_string(),
            json!({ "messageStop": { "stopReason": "end_turn" } }).to_string(),
            json!({ "metadata": { "usage": { "inputTokens": 1, "outputTokens": 1 } } })
                .to_string(),
        ];
        for chunk in cases {
            assert!(
                transform_chunk_to_anthropic(&chunk).is_none(),
                "non-text envelope must be dropped: {chunk}",
            );
        }
    }

    #[test]
    fn response_to_claude_shape_lifts_text_and_usage() {
        let nova_body = json!({
            "output": {
                "message": {
                    "role": "assistant",
                    "content": [{ "text": "Hello, " }, { "text": "world!" }]
                }
            },
            "stopReason": "end_turn",
            "usage": { "inputTokens": 12, "outputTokens": 3 }
        });

        let claude_body = response_to_claude_shape(nova_body);
        assert_eq!(claude_body["stop_reason"], "end_turn");
        let content = claude_body["content"].as_array().unwrap();
        assert_eq!(content.len(), 2);
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[0]["text"], "Hello, ");
        assert_eq!(content[1]["text"], "world!");
        assert_eq!(claude_body["usage"]["input_tokens"], 12);
        assert_eq!(claude_body["usage"]["output_tokens"], 3);
    }
}
