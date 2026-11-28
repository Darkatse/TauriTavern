//! Merges the events of a recorded stream into a response document shaped like the one
//! recorded for a non-streamed generation, so one projection serves both.
//!
//! The merge rules repeat tt-adapter-provider-http's accumulators without their rejections:
//! a log has to show a stream that stopped early or carried several candidates.

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};
use tt_contracts::observability::RequestKindDto;
use tt_ports::repositories::chat_completion_repository::{NATIVE_CLAUDE, NATIVE_GEMINI};

type Object = Map<String, Value>;

const MESSAGE_FRAGMENTS: [&str; 4] = ["content", "reasoning_content", "reasoning", "tool_plan"];
const TEXT_OUTPUT_FRAGMENTS: [&str; 4] = ["text", "content", "reasoning", "thinking"];

pub fn merge_stream_events(kind: RequestKindDto, events: impl Iterator<Item = Object>) -> Value {
    let mut totals = Totals::default();
    let choices = match kind {
        RequestKindDto::Chat => {
            let mut candidates = BTreeMap::<u64, Candidate>::new();
            for event in events {
                totals.push(&event);
                push_chat_event(&mut candidates, &mut totals, &event);
            }
            candidates
                .into_iter()
                .map(|(index, candidate)| candidate.into_choice(index))
                .collect()
        }
        RequestKindDto::Text => {
            let mut outputs = BTreeMap::<u64, Object>::new();
            for event in events {
                totals.push(&event);
                push_text_event(&mut outputs, &event);
            }
            outputs.into_values().map(Value::Object).collect()
        }
    };
    totals.into_document(choices)
}

#[derive(Default)]
struct Totals {
    usage: Object,
    usage_metadata: Object,
    error: Option<Value>,
}

impl Totals {
    fn push(&mut self, event: &Object) {
        self.push_usage(event.get("usage"));
        if let Some(Value::Object(usage)) = event.get("usageMetadata") {
            merge_fields(&mut self.usage_metadata, usage, &[]);
        }
        if let Some(error) = event.get("error").filter(|error| !error.is_null()) {
            self.error = Some(error.clone());
        }
    }

    /// Usage arrives as cumulative snapshots or, from Anthropic, split over two events.
    fn push_usage(&mut self, usage: Option<&Value>) {
        if let Some(Value::Object(usage)) = usage {
            merge_fields(&mut self.usage, usage, &[]);
        }
    }

    fn into_document(self, choices: Vec<Value>) -> Value {
        let mut document = Object::new();
        document.insert("choices".to_string(), Value::Array(choices));
        if !self.usage.is_empty() {
            document.insert("usage".to_string(), Value::Object(self.usage));
        }
        if !self.usage_metadata.is_empty() {
            document.insert(
                "usageMetadata".to_string(),
                Value::Object(self.usage_metadata),
            );
        }
        if let Some(error) = self.error {
            document.insert("error".to_string(), error);
        }
        Value::Object(document)
    }
}

#[derive(Default)]
struct Candidate {
    message: Object,
    finish_reason: Option<String>,
    tool_calls: BTreeMap<u64, ToolCall>,
    cohere_content: BTreeMap<u64, Object>,
    claude_content: BTreeMap<u64, Object>,
    gemini_content: Object,
    gemini_parts: Vec<Value>,
}

#[derive(Default)]
struct ToolCall {
    call: Object,
    function: Object,
}

fn push_chat_event(candidates: &mut BTreeMap<u64, Candidate>, totals: &mut Totals, event: &Object) {
    if let Some(Value::Array(choices)) = event.get("choices") {
        for choice in choices.iter().filter_map(Value::as_object) {
            candidates
                .entry(candidate_index(choice))
                .or_default()
                .push_chunk_choice(choice);
        }
    }
    // The adapters read the first Gemini candidate only.
    if let Some(Value::Object(candidate)) = event.get("candidates").and_then(|c| c.get(0)) {
        candidates
            .entry(0)
            .or_default()
            .push_gemini_candidate(candidate);
    }
    match event.get("type").and_then(Value::as_str) {
        Some("message_start") => {
            if let Some(Value::Object(message)) = event.get("message") {
                if let Some(role) = message.get("role") {
                    let candidate = candidates.entry(0).or_default();
                    merge_field(&mut candidate.message, "role", role, &[]);
                }
                totals.push_usage(message.get("usage"));
            }
        }
        Some("content_block_start") => {
            if let (Some(index), Some(Value::Object(block))) =
                (event_index(event), event.get("content_block"))
            {
                candidates
                    .entry(0)
                    .or_default()
                    .claude_content
                    .insert(index, block.clone());
            }
        }
        Some("content_block_delta") => {
            if let (Some(index), Some(Value::Object(delta))) =
                (event_index(event), event.get("delta"))
            {
                candidates
                    .entry(0)
                    .or_default()
                    .push_claude_delta(index, delta);
            }
        }
        Some("message_delta") => {
            let stop_reason = event
                .get("delta")
                .and_then(|delta| delta.get("stop_reason"));
            candidates
                .entry(0)
                .or_default()
                .set_finish_reason(stop_reason);
        }
        Some(
            "message-start" | "content-start" | "content-delta" | "tool-plan-delta"
            | "tool-call-start" | "tool-call-delta" | "message-end",
        ) => {
            if let Some(Value::Object(delta)) = event.get("delta") {
                totals.push_usage(delta.get("usage"));
                candidates
                    .entry(0)
                    .or_default()
                    .push_cohere_delta(event_index(event), delta);
            }
        }
        _ => {}
    }
}

impl Candidate {
    fn set_finish_reason(&mut self, reason: Option<&Value>) {
        if let Some(reason) = reason
            .and_then(Value::as_str)
            .filter(|reason| !reason.is_empty())
        {
            self.finish_reason = Some(reason.to_string());
        }
    }

    fn push_chunk_choice(&mut self, choice: &Object) {
        self.set_finish_reason(choice.get("finish_reason"));
        let Some(Value::Object(delta)) = choice.get("delta") else {
            return;
        };
        for (key, value) in delta {
            match (key.as_str(), value) {
                ("tool_calls", Value::Array(calls)) => {
                    for call in calls.iter().filter_map(Value::as_object) {
                        self.push_tool_call(call);
                    }
                }
                ("reasoning_details", Value::Array(details)) => {
                    extend_array(&mut self.message, key, details)
                }
                _ => merge_field(&mut self.message, key, value, &MESSAGE_FRAGMENTS),
            }
        }
    }

    /// The adapter's identity rule: the wire index, or the id when a provider sends no index.
    fn push_tool_call(&mut self, call: &Object) {
        let index = match call.get("index").and_then(Value::as_u64) {
            Some(index) => index,
            None => {
                let Some(id) = call
                    .get("id")
                    .and_then(Value::as_str)
                    .filter(|id| !id.is_empty())
                else {
                    return;
                };
                let known = self
                    .tool_calls
                    .iter()
                    .find(|(_, existing)| {
                        existing.call.get("id").and_then(Value::as_str) == Some(id)
                    })
                    .map(|(index, _)| *index);
                known.unwrap_or_else(|| {
                    self.tool_calls
                        .last_key_value()
                        .map_or(0, |(last, _)| last + 1)
                })
            }
        };
        self.tool_calls.entry(index).or_default().push(call);
    }

    /// A delta whose block never started (the text frames Bedrock's non-Claude models are
    /// rewritten to) opens a block of the matching type.
    fn push_claude_delta(&mut self, index: u64, delta: &Object) {
        let (block_type, field, fragment) = match delta.get("type").and_then(Value::as_str) {
            Some("text_delta") => ("text", "text", delta.get("text")),
            Some("thinking_delta") => ("thinking", "thinking", delta.get("thinking")),
            Some("signature_delta") => ("thinking", "signature", delta.get("signature")),
            Some("input_json_delta") => ("tool_use", "input", delta.get("partial_json")),
            _ => return,
        };
        let Some(fragment) = fragment.and_then(Value::as_str) else {
            return;
        };
        let block = self
            .claude_content
            .entry(index)
            .or_insert_with(|| Object::from_iter([("type".to_string(), Value::from(block_type))]));
        append_fragment(block, field, fragment);
    }

    fn push_cohere_delta(&mut self, index: Option<u64>, delta: &Object) {
        self.set_finish_reason(delta.get("finish_reason"));
        let Some(Value::Object(message)) = delta.get("message") else {
            return;
        };
        for (key, value) in message {
            match (key.as_str(), value, index) {
                ("content", Value::Object(block), Some(index)) => {
                    merge_fields(
                        self.cohere_content.entry(index).or_default(),
                        block,
                        &["text", "thinking"],
                    );
                }
                ("tool_calls", Value::Object(call), Some(index)) => {
                    self.tool_calls.entry(index).or_default().push(call);
                }
                _ => merge_field(&mut self.message, key, value, &MESSAGE_FRAGMENTS),
            }
        }
    }

    fn push_gemini_candidate(&mut self, candidate: &Object) {
        self.set_finish_reason(candidate.get("finishReason"));
        let Some(Value::Object(content)) = candidate.get("content") else {
            return;
        };
        for (key, value) in content {
            match (key.as_str(), value) {
                ("parts", Value::Array(parts)) => {
                    for (position, part) in parts.iter().enumerate() {
                        self.push_gemini_part(part, position == 0);
                    }
                }
                _ => merge_field(&mut self.gemini_content, key, value, &[]),
            }
        }
    }

    /// Follows `GeminiStreamAccumulator::apply_part`.
    fn push_gemini_part(&mut self, part: &Value, first_in_event: bool) {
        if first_in_event
            && let Value::Object(fields) = part
            && fields.get("text").is_some_and(Value::is_string)
            && let Some(Value::Object(previous)) = self.gemini_parts.last_mut()
            && previous.get("text").is_some_and(Value::is_string)
            && previous.get("thought").and_then(Value::as_bool)
                == fields.get("thought").and_then(Value::as_bool)
        {
            merge_fields(previous, fields, &["text"]);
        } else {
            self.gemini_parts.push(part.clone());
        }
    }

    fn into_choice(self, index: u64) -> Value {
        let mut message = self.message;
        if !self.cohere_content.is_empty() {
            let content = self
                .cohere_content
                .into_values()
                .map(Value::Object)
                .collect();
            message.insert("content".to_string(), Value::Array(content));
        }
        if !self.tool_calls.is_empty() {
            let tool_calls = self
                .tool_calls
                .into_values()
                .map(ToolCall::into_value)
                .collect();
            message.insert("tool_calls".to_string(), Value::Array(tool_calls));
        }
        if !self.claude_content.is_empty() {
            let content = self
                .claude_content
                .into_values()
                .map(finish_claude_block)
                .collect::<Vec<_>>();
            message.insert(
                "native".to_string(),
                json!({ NATIVE_CLAUDE: { "content": content } }),
            );
        }
        if !self.gemini_parts.is_empty() {
            let mut gemini_content = self.gemini_content;
            gemini_content.insert("parts".to_string(), Value::Array(self.gemini_parts));
            // A complete adapter-run Gemini stream ends with a chunk that carries this content
            // as `native`; the merged parts stand in when the stream stopped before it.
            message
                .entry("native")
                .or_insert_with(|| json!({ NATIVE_GEMINI: { "content": gemini_content } }));
        }
        json!({ "index": index, "message": message, "finish_reason": self.finish_reason })
    }
}

impl ToolCall {
    fn push(&mut self, delta: &Object) {
        for (key, value) in delta {
            match (key.as_str(), value) {
                ("function", Value::Object(function)) => {
                    merge_fields(&mut self.function, function, &["arguments"]);
                }
                _ => merge_field(&mut self.call, key, value, &[]),
            }
        }
    }

    fn into_value(mut self) -> Value {
        self.call
            .insert("function".to_string(), Value::Object(self.function));
        Value::Object(self.call)
    }
}

fn push_text_event(outputs: &mut BTreeMap<u64, Object>, event: &Object) {
    match event.get("choices") {
        Some(Value::Array(choices)) => {
            for choice in choices.iter().filter_map(Value::as_object) {
                push_text_output(outputs, choice);
            }
        }
        // llama.cpp streams bare outputs.
        None if event.contains_key("content") => push_text_output(outputs, event),
        _ => {}
    }
}

fn push_text_output(outputs: &mut BTreeMap<u64, Object>, output: &Object) {
    merge_fields(
        outputs.entry(candidate_index(output)).or_default(),
        output,
        &TEXT_OUTPUT_FRAGMENTS,
    );
}

/// Ollama's events and single-candidate streams of some OpenAI-compatible servers carry no index.
fn candidate_index(candidate: &Object) -> u64 {
    candidate.get("index").and_then(Value::as_u64).unwrap_or(0)
}

fn event_index(event: &Object) -> Option<u64> {
    event.get("index").and_then(Value::as_u64)
}

fn merge_fields(target: &mut Object, source: &Object, fragments: &[&str]) {
    for (key, value) in source {
        merge_field(target, key, value, fragments);
    }
}

/// One streamed field: `fragments` grow as strings, any other field keeps its latest
/// non-empty value.
fn merge_field(target: &mut Object, key: &str, value: &Value, fragments: &[&str]) {
    match value {
        Value::Null => {}
        Value::String(text) if fragments.contains(&key) => append_fragment(target, key, text),
        Value::String(text) if text.is_empty() => {}
        _ => {
            target.insert(key.to_string(), value.clone());
        }
    }
}

/// A field holding anything but a string starts over with the fragment: the `input: {}` a
/// `tool_use` block opens with gives way to its streamed JSON text.
fn append_fragment(target: &mut Object, key: &str, fragment: &str) {
    if fragment.is_empty() {
        return;
    }
    match target.get_mut(key) {
        Some(Value::String(text)) => text.push_str(fragment),
        _ => {
            target.insert(key.to_string(), Value::String(fragment.to_string()));
        }
    }
}

fn extend_array(target: &mut Object, key: &str, items: &[Value]) {
    match target.get_mut(key) {
        Some(Value::Array(merged)) => merged.extend_from_slice(items),
        _ => {
            target.insert(key.to_string(), Value::Array(items.to_vec()));
        }
    }
}

/// A block's streamed `input` is JSON text. Complete, it becomes the value a non-streamed
/// response carries; a stream cut off inside it keeps the text.
fn finish_claude_block(mut block: Object) -> Value {
    if let Some(Value::String(text)) = block.get("input")
        && let Ok(input) = serde_json::from_str::<Value>(text)
    {
        block.insert("input".to_string(), input);
    }
    Value::Object(block)
}
