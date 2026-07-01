//! Meta Llama non-stream / stream chunk → Claude-shape projection.

use serde_json::{Map, Value, json};

/// Llama non-stream response shape (per AWS Bedrock User Guide
/// `model-parameters-meta.md`):
/// ```json
/// { "generation": "...", "prompt_token_count": N, "generation_token_count": M, "stop_reason": "stop" }
/// ```
/// We translate it into a single-block Claude payload so the existing
/// `normalize_claude_response` can fold it into an OpenAI `chat.completion`.
/// Llama's `stop_reason` values (`stop`, `length`) already align with Claude's
/// `end_turn` / `max_tokens` after the Claude finish-reason mapping.
pub(super) fn response_to_claude_shape(body: Value) -> Value {
    let text = body
        .get("generation")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let stop_reason = match body.get("stop_reason").and_then(Value::as_str) {
        Some("length") => "max_tokens".to_string(),
        Some(other) => other.to_string(),
        None => "end_turn".to_string(),
    };

    let mut usage = Map::new();
    if let Some(input_tokens) = body.get("prompt_token_count").and_then(Value::as_u64) {
        usage.insert("input_tokens".to_string(), json!(input_tokens));
    }
    if let Some(output_tokens) = body.get("generation_token_count").and_then(Value::as_u64) {
        usage.insert("output_tokens".to_string(), json!(output_tokens));
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

/// Llama stream chunks each carry the next token group in `generation`. The
/// terminal chunk also carries `stop_reason` so the frontend never sees
/// `null`. We map every non-empty `generation` value to an Anthropic
/// `content_block_delta` text frame and drop the empty trailing chunk.
pub(super) fn transform_chunk_to_anthropic(decoded: &str) -> Option<String> {
    let value: Value = serde_json::from_str(decoded).ok()?;
    let text = value.get("generation").and_then(Value::as_str)?;
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
    fn transform_chunk_to_anthropic_extracts_generation_text() {
        let chunk = json!({
            "generation": "world!",
            "prompt_token_count": 5,
            "generation_token_count": 1,
            "stop_reason": null
        })
        .to_string();
        let rewritten =
            transform_chunk_to_anthropic(&chunk).expect("non-empty generations must surface text");
        let parsed: Value = serde_json::from_str(&rewritten).unwrap();
        assert_eq!(parsed["type"], "content_block_delta");
        assert_eq!(parsed["index"], 0);
        assert_eq!(parsed["delta"]["type"], "text_delta");
        assert_eq!(parsed["delta"]["text"], "world!");
    }

    #[test]
    fn transform_chunk_to_anthropic_drops_trailing_empty_generation() {
        let chunk = json!({ "generation": "", "stop_reason": "stop" }).to_string();
        assert!(transform_chunk_to_anthropic(&chunk).is_none());
    }

    #[test]
    fn response_to_claude_shape_lifts_generation_text_and_token_counts() {
        let body = json!({
            "generation": "hello there",
            "prompt_token_count": 7,
            "generation_token_count": 4,
            "stop_reason": "stop"
        });
        let claude_body = response_to_claude_shape(body);
        assert_eq!(claude_body["stop_reason"], "stop");
        let content = claude_body["content"].as_array().unwrap();
        assert_eq!(content.len(), 1);
        assert_eq!(content[0]["text"], "hello there");
        assert_eq!(claude_body["usage"]["input_tokens"], 7);
        assert_eq!(claude_body["usage"]["output_tokens"], 4);
    }

    #[test]
    fn response_length_stop_reason_maps_to_claude_max_tokens() {
        let body = json!({ "generation": "truncated", "stop_reason": "length" });
        let claude_body = response_to_claude_shape(body);
        assert_eq!(claude_body["stop_reason"], "max_tokens");
    }
}
