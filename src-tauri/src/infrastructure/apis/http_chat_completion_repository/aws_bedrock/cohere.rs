//! Cohere Command R / R+ non-stream / stream chunk → Claude-shape projection.
//!
//! Cohere Command R / R+ non-stream responses (per AWS Bedrock User Guide
//! `model-parameters-cohere-command-r-plus.md`) look like:
//!
//! ```json
//! { "response_id": "...", "text": "...", "finish_reason": "complete|max_tokens|error|...",
//!   "meta": { "billed_units": { "input_tokens": N, "output_tokens": M } } }
//! ```
//!
//! Streams emit `{ event_type, ... }` envelopes; only `text-generation`
//! carries user-visible text. Both shapes are flattened into the Claude
//! envelope the parent module's `normalize_claude_response` consumes.

use serde_json::{Map, Value, json};

/// Lift Cohere's `text` straight into a Claude `content[].text` block, project
/// Cohere's `finish_reason` into Claude vocabulary (`complete` -> `end_turn`,
/// `max_tokens` stays, `error_*` -> `error`, ...), and forward
/// `meta.billed_units` as `usage.input_tokens` / `usage.output_tokens`.
pub(super) fn response_to_claude_shape(body: Value) -> Value {
    let text = body
        .get("text")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_default();

    let stop_reason = body
        .get("finish_reason")
        .and_then(Value::as_str)
        .map(map_finish_reason_to_claude)
        .unwrap_or_else(|| "end_turn".to_string());

    let mut usage = Map::new();
    if let Some(billed) = body
        .pointer("/meta/billed_units")
        .and_then(Value::as_object)
    {
        if let Some(input_tokens) = billed.get("input_tokens").and_then(Value::as_u64) {
            usage.insert("input_tokens".to_string(), json!(input_tokens));
        }
        if let Some(output_tokens) = billed.get("output_tokens").and_then(Value::as_u64) {
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

fn map_finish_reason_to_claude(reason: &str) -> String {
    match reason {
        "complete" => "end_turn".to_string(),
        "max_tokens" => "max_tokens".to_string(),
        // error_toxic / error_limit / error / user_cancel collapse into the
        // generic "error" — Anthropic doesn't enumerate a richer vocabulary.
        other if other.starts_with("error") => "error".to_string(),
        "user_cancel" => "error".to_string(),
        other => other.to_string(),
    }
}

/// Cohere streams chat events keyed by `event_type` (per
/// `bedrock-runtime_example_bedrock-runtime_InvokeModelWithResponseStream_CohereCommandR_section`
/// and the Cohere openapi spec):
///
/// - `{ "event_type": "stream-start",   "generation_id": "..." }`
/// - `{ "event_type": "text-generation","text": "...", "is_finished": false }`
/// - `{ "event_type": "stream-end",     "finish_reason": "...", "response": {...} }`
/// - `{ "event_type": "citation-generation" | "tool-calls-*" | ... }`
///
/// Only `text-generation` carries user-visible text; everything else is
/// dropped. We also accept legacy Cohere Command (`generations[0].text`)
/// chunks defensively so the same path works for `cohere.command-text-v14`
/// stream output if it ever lands here.
pub(super) fn transform_chunk_to_anthropic(decoded: &str) -> Option<String> {
    let value: Value = serde_json::from_str(decoded).ok()?;
    let text = match value.get("event_type").and_then(Value::as_str) {
        Some("text-generation") => value.get("text").and_then(Value::as_str)?,
        Some(_) => return None,
        None => value
            .pointer("/generations/0/text")
            .and_then(Value::as_str)?,
    };

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
    fn transform_chunk_to_anthropic_only_keeps_text_generation_events() {
        let text_gen = json!({
            "event_type": "text-generation",
            "text": " hi",
            "is_finished": false,
        })
        .to_string();
        let rewritten = transform_chunk_to_anthropic(&text_gen)
            .expect("text-generation event must surface a delta");
        let parsed: Value = serde_json::from_str(&rewritten).unwrap();
        assert_eq!(parsed["type"], "content_block_delta");
        assert_eq!(parsed["delta"]["text"], " hi");

        let stream_start = json!({
            "event_type": "stream-start",
            "generation_id": "abc",
        })
        .to_string();
        assert!(transform_chunk_to_anthropic(&stream_start).is_none());

        let stream_end = json!({
            "event_type": "stream-end",
            "finish_reason": "complete",
            "response": {},
        })
        .to_string();
        assert!(transform_chunk_to_anthropic(&stream_end).is_none());

        // Legacy Cohere Command (generations[0].text) — kept as a defensive
        // fallback for cohere.command-text-v14 / command-light-text-v14 if
        // they ever stream through here.
        let legacy = json!({
            "generations": [{ "text": " legacy" }]
        })
        .to_string();
        let legacy_delta = transform_chunk_to_anthropic(&legacy)
            .expect("legacy chunks must surface a delta");
        let parsed: Value = serde_json::from_str(&legacy_delta).unwrap();
        assert_eq!(parsed["delta"]["text"], " legacy");
    }

    #[test]
    fn response_to_claude_shape_maps_finish_reason_and_billed_units() {
        let body = json!({
            "response_id": "abc",
            "text": "cohere reply",
            "finish_reason": "max_tokens",
            "meta": {
                "billed_units": { "input_tokens": 42, "output_tokens": 7 }
            }
        });

        let shape = response_to_claude_shape(body);
        assert_eq!(shape["content"][0]["text"], "cohere reply");
        assert_eq!(shape["stop_reason"], "max_tokens");
        assert_eq!(shape["usage"]["input_tokens"], 42);
        assert_eq!(shape["usage"]["output_tokens"], 7);

        let complete = json!({ "text": "done", "finish_reason": "complete" });
        assert_eq!(
            response_to_claude_shape(complete)["stop_reason"],
            "end_turn",
            "complete must map to Claude end_turn",
        );

        let toxic = json!({ "text": "", "finish_reason": "error_toxic" });
        assert_eq!(
            response_to_claude_shape(toxic)["stop_reason"],
            "error",
            "any error_* must collapse into Claude error",
        );
    }
}
