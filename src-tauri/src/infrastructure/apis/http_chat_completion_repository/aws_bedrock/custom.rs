//! Custom-template response / stream chunk extraction for the AWS Bedrock
//! escape hatch.
//!
//! When the user opts into [`payload::aws_bedrock::custom`], the response /
//! stream JSON arriving from Bedrock can be arbitrary because the request
//! body is user-controlled. We give the user two knobs:
//!
//! - `aws_bedrock_custom_response_path` — dotted JSON path applied to the
//!   non-stream response body (e.g. `output.message.content.0.text`).
//! - `aws_bedrock_custom_stream_path` — dotted JSON path applied to each
//!   streaming chunk JSON (e.g. `delta.text` or `choices.0.delta.content`).
//!
//! Both paths use the simple `a.b.0.c` syntax that maps 1:1 to RFC 6901 JSON
//! Pointers (`/a/b/0/c`). Empty extractions are forwarded as empty deltas;
//! missing paths are silently dropped so terminal sentinel chunks (empty
//! `delta = {}`, `finish_reason`-only frames, ...) never surface as blank
//! deltas to the renderer.

use serde_json::{Map, Value, json};

/// Translate a dotted JSON path (`a.b.0.c`) into an RFC 6901 JSON Pointer
/// (`/a/b/0/c`). Empty / whitespace-only inputs return `None` so callers can
/// short-circuit. The translation also handles RFC 6901's escape rules: `/`
/// becomes `~1` and `~` becomes `~0`.
pub(super) fn to_json_pointer(path: &str) -> Option<String> {
    let path = path.trim();
    if path.is_empty() {
        return None;
    }
    // Already a JSON Pointer? Forward verbatim so power users keep escape
    // semantics they already understand.
    if path.starts_with('/') {
        return Some(path.to_string());
    }
    let mut out = String::with_capacity(path.len() + 1);
    for segment in path.split('.') {
        out.push('/');
        for ch in segment.chars() {
            match ch {
                '~' => out.push_str("~0"),
                '/' => out.push_str("~1"),
                other => out.push(other),
            }
        }
    }
    Some(out)
}

/// Extract the substring living at `path` inside `body` and project it into a
/// Claude-shaped envelope `normalize_claude_response` can consume. Missing
/// paths surface as an empty `text` block so the OpenAI projection downstream
/// still emits a syntactically-valid `chat.completion`.
pub(super) fn response_to_claude_shape(body: Value, path: &str) -> Value {
    let pointer = to_json_pointer(path);
    let text = pointer
        .as_deref()
        .and_then(|ptr| body.pointer(ptr))
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_default();

    let mut claude_body = Map::new();
    claude_body.insert(
        "content".to_string(),
        Value::Array(vec![json!({ "type": "text", "text": text })]),
    );
    claude_body.insert(
        "stop_reason".to_string(),
        Value::String("end_turn".to_string()),
    );
    Value::Object(claude_body)
}

/// Pull the text at `path` from a streaming chunk JSON and rewrap it as an
/// Anthropic `content_block_delta` frame. Returns `None` for chunks that
/// don't carry text at the given path (terminal `usage`-only frames, sentinel
/// events, ...).
pub(super) fn transform_chunk_to_anthropic(decoded: &str, path: &str) -> Option<String> {
    let pointer = to_json_pointer(path)?;
    let value: Value = serde_json::from_str(decoded).ok()?;
    let text = value.pointer(&pointer).and_then(Value::as_str)?;
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

    use super::{response_to_claude_shape, to_json_pointer, transform_chunk_to_anthropic};

    #[test]
    fn to_json_pointer_translates_dotted_paths() {
        assert_eq!(
            to_json_pointer("output.message.content.0.text").as_deref(),
            Some("/output/message/content/0/text"),
        );
        assert_eq!(to_json_pointer("delta.text").as_deref(), Some("/delta/text"));
        assert_eq!(to_json_pointer("text").as_deref(), Some("/text"));
        assert_eq!(to_json_pointer("").as_deref(), None);
        assert_eq!(to_json_pointer("   ").as_deref(), None);
        // Already a JSON Pointer — pass through.
        assert_eq!(
            to_json_pointer("/already/escaped").as_deref(),
            Some("/already/escaped"),
        );
        // `/` segment escapes to `~1` per RFC 6901.
        assert_eq!(
            to_json_pointer("a/b.c").as_deref(),
            Some("/a~1b/c"),
        );
        // `~` segment escapes to `~0`.
        assert_eq!(
            to_json_pointer("a~b.c").as_deref(),
            Some("/a~0b/c"),
        );
    }

    #[test]
    fn response_to_claude_shape_extracts_text_at_user_path() {
        let body = json!({
            "output": {
                "message": {
                    "content": [{ "text": "hello world" }]
                }
            }
        });
        let claude = response_to_claude_shape(body, "output.message.content.0.text");
        assert_eq!(claude["content"][0]["text"], "hello world");
        assert_eq!(claude["stop_reason"], "end_turn");
    }

    #[test]
    fn response_to_claude_shape_falls_back_to_empty_text_when_path_missing() {
        let body = json!({ "output": {} });
        let claude = response_to_claude_shape(body, "output.message.content.0.text");
        assert_eq!(claude["content"][0]["text"], "");
    }

    #[test]
    fn transform_chunk_extracts_text_and_wraps_as_anthropic_delta() {
        let chunk = json!({ "delta": { "text": "incremental " } }).to_string();
        let rewritten = transform_chunk_to_anthropic(&chunk, "delta.text")
            .expect("chunk with text must surface a delta");
        let parsed: Value = serde_json::from_str(&rewritten).unwrap();
        assert_eq!(parsed["type"], "content_block_delta");
        assert_eq!(parsed["delta"]["text"], "incremental ");
    }

    #[test]
    fn transform_chunk_drops_chunks_without_text_at_path() {
        let chunk = json!({ "delta": {} }).to_string();
        assert!(transform_chunk_to_anthropic(&chunk, "delta.text").is_none());
        let chunk = json!({ "delta": { "text": "" } }).to_string();
        assert!(transform_chunk_to_anthropic(&chunk, "delta.text").is_none());
    }

    #[test]
    fn transform_chunk_returns_none_for_empty_path() {
        let chunk = json!({ "delta": { "text": "x" } }).to_string();
        assert!(transform_chunk_to_anthropic(&chunk, "").is_none());
    }
}
