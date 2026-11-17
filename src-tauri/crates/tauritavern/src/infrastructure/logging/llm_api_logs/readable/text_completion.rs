use serde_json::Value;

use super::super::recording::StreamReadable;
use super::stream::readable_output;

pub(in crate::infrastructure::logging::llm_api_logs) fn format_prompt(payload: &Value) -> String {
    match payload.get("prompt") {
        Some(Value::String(prompt)) => prompt.clone(),
        Some(prompt) => prompt.to_string(),
        None => "<prompt unavailable>".to_string(),
    }
}

/// Every candidate: OpenAI-style `choices`, llama.cpp's array of outputs, or a single llama.cpp or
/// Ollama output.
pub(in crate::infrastructure::logging::llm_api_logs) fn format_completion(
    response: &Value,
) -> String {
    let candidates = match response {
        Value::Array(outputs) => outputs
            .iter()
            .map(|output| readable_output(output_reasoning(output), output_text(output)))
            .collect::<Vec<_>>(),
        _ => match response.get("choices").and_then(Value::as_array) {
            Some(choices) => choices
                .iter()
                .map(|choice| readable_output(choice_reasoning(choice), str_field(choice, "text")))
                .collect(),
            None => vec![readable_output(
                output_reasoning(response),
                output_text(response),
            )],
        },
    };
    candidates.join("\n\n")
}

/// The first candidate of a text completion stream, in any backend's event shape.
#[derive(Default)]
pub(in crate::infrastructure::logging::llm_api_logs) struct CompletionStreamReadable {
    text: String,
    reasoning: String,
}

impl StreamReadable for CompletionStreamReadable {
    fn push(&mut self, event: &str) {
        let Ok(event) = serde_json::from_str::<Value>(event.trim()) else {
            return;
        };

        match event.pointer("/choices/0") {
            Some(choice) if candidate_index(choice) == 0 => {
                self.text.push_str(str_field(choice, "text"));
                self.reasoning.push_str(choice_reasoning(choice));
            }
            None if candidate_index(&event) == 0 => {
                self.text.push_str(str_field(&event, "content"));
                self.reasoning.push_str(output_reasoning(&event));
            }
            _ => {}
        }
    }

    fn into_string(self) -> String {
        readable_output(&self.reasoning, &self.text)
    }
}

fn candidate_index(value: &Value) -> u64 {
    value.get("index").and_then(Value::as_u64).unwrap_or(0)
}

fn choice_reasoning(choice: &Value) -> &str {
    let reasoning = str_field(choice, "reasoning");
    if reasoning.is_empty() {
        str_field(choice, "thinking")
    } else {
        reasoning
    }
}

/// llama.cpp writes `content`; Ollama writes `response`.
fn output_text(value: &Value) -> &str {
    let content = str_field(value, "content");
    if content.is_empty() {
        str_field(value, "response")
    } else {
        content
    }
}

fn output_reasoning(value: &Value) -> &str {
    str_field(value, "thinking")
}

fn str_field<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}
