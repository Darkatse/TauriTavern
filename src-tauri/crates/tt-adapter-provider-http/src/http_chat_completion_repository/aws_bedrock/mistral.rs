//! Mistral non-stream / stream chunk → Claude-shape projection.
//!
//! Mistral on Bedrock exposes two response dialects, switched by model id:
//!
//! - **Legacy text-completion** (7B, Mixtral, large-2402): the body is
//!   `{ "outputs": [{ "text": "...", "stop_reason": "stop" }] }`.
//! - **Chat-completion** (large-2407+, small, medium, Pixtral): the body may
//!   be `{ "content": [{ "role": "assistant", "content": [{ "text": "..." }] }] }`
//!   (mistral-large-2407 spec) **or** `{ "choices": [{ "index":0,
//!   "message": { "role":"assistant", "content":"string" }, "stop_reason":"stop" }] }`
//!   (the generic Mistral chat-completion spec).
//!
//! We probe each shape in turn and emit a Claude-style payload so the existing
//! `normalize_claude_response` does the OpenAI-shape rewriting. Stream chunks
//! mirror the same three flavours and are unified into Anthropic
//! `content_block_delta` text frames.

use serde_json::{Map, Value, json};

pub(super) fn response_to_claude_shape(body: Value) -> Value {
    let text = if let Some(outputs) = body.get("outputs").and_then(Value::as_array) {
        outputs
            .iter()
            .filter_map(|output| output.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("")
    } else if let Some(choices) = body.get("choices").and_then(Value::as_array) {
        choices
            .iter()
            .filter_map(|choice| {
                choice
                    .pointer("/message/content")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .collect::<Vec<_>>()
            .join("")
    } else if let Some(content) = body.get("content").and_then(Value::as_array) {
        content
            .iter()
            .filter_map(|message| {
                message
                    .get("content")
                    .and_then(Value::as_array)
                    .map(|parts| {
                        parts
                            .iter()
                            .filter_map(|part| {
                                part.get("text").and_then(Value::as_str).map(str::to_string)
                            })
                            .collect::<Vec<_>>()
                            .join("")
                    })
            })
            .collect::<Vec<_>>()
            .join("")
    } else {
        String::new()
    };

    let stop_reason = body
        .pointer("/choices/0/stop_reason")
        .or_else(|| body.pointer("/outputs/0/stop_reason"))
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

/// Mistral stream chunks come in three flavours depending on the model:
///
/// - Legacy text-completion: `{ "outputs": [{ "text": "...", "stop_reason": null }] }`
/// - Chat (large-2407 spec):  `{ "content": [{ "text": "..." }] }`
/// - Chat (chat-completion):  `{ "choices": [{ "delta": { "content": "..." } }] }`
///
/// We probe each one and rewrap the extracted text as an Anthropic
/// `content_block_delta` frame. Chunks without user-visible text (final
/// `stop_reason` markers, empty `content`, ...) are silently dropped.
pub(super) fn transform_chunk_to_anthropic(decoded: &str) -> Option<String> {
    let value: Value = serde_json::from_str(decoded).ok()?;

    let text = value
        .pointer("/choices/0/delta/content")
        .and_then(Value::as_str)
        .or_else(|| value.pointer("/content/0/text").and_then(Value::as_str))
        .or_else(|| value.pointer("/outputs/0/text").and_then(Value::as_str))?;

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
    fn transform_chunk_to_anthropic_handles_all_three_shapes() {
        let legacy = json!({ "outputs": [{ "text": " hi", "stop_reason": null }] }).to_string();
        let chat_2407 = json!({ "content": [{ "text": " from" }] }).to_string();
        let chat_openai = json!({
            "choices": [{ "index": 0, "delta": { "content": " mistral" } }],
        })
        .to_string();

        for (chunk, expected) in [
            (legacy, " hi"),
            (chat_2407, " from"),
            (chat_openai, " mistral"),
        ] {
            let rewritten = transform_chunk_to_anthropic(&chunk)
                .unwrap_or_else(|| panic!("expected delta from chunk: {chunk}"));
            let parsed: Value = serde_json::from_str(&rewritten).unwrap();
            assert_eq!(parsed["type"], "content_block_delta");
            assert_eq!(parsed["delta"]["text"], expected);
        }
    }

    #[test]
    fn transform_chunk_to_anthropic_drops_terminal_stop_reason_chunks() {
        let chunk = json!({ "outputs": [{ "text": "", "stop_reason": "stop" }] }).to_string();
        assert!(transform_chunk_to_anthropic(&chunk).is_none());
    }

    #[test]
    fn response_to_claude_shape_picks_up_text_from_each_response_dialect() {
        let legacy = json!({
            "outputs": [{ "text": "legacy text", "stop_reason": "stop" }]
        });
        let chat_2407 = json!({
            "content": [{
                "role": "assistant",
                "content": [{ "text": "chat 2407 text" }]
            }]
        });
        let chat_openai = json!({
            "choices": [{
                "index": 0,
                "message": { "role": "assistant", "content": "chat openai text" },
                "stop_reason": "length"
            }]
        });

        let legacy_shape = response_to_claude_shape(legacy);
        assert_eq!(legacy_shape["content"][0]["text"], "legacy text");
        assert_eq!(legacy_shape["stop_reason"], "stop");

        let chat_2407_shape = response_to_claude_shape(chat_2407);
        assert_eq!(chat_2407_shape["content"][0]["text"], "chat 2407 text");

        let chat_openai_shape = response_to_claude_shape(chat_openai);
        assert_eq!(chat_openai_shape["content"][0]["text"], "chat openai text");
        assert_eq!(
            chat_openai_shape["stop_reason"], "max_tokens",
            "length must map to Claude max_tokens",
        );
    }
}
