use serde_json::Value;
use tt_contracts::observability::{
    MessageViewDto, ParameterViewDto, PartViewDto, RequestKindDto, RequestViewDto,
};

use super::projection::{pointer_index, pointer_push};

pub(super) fn project_text_request(payload: &Value) -> RequestViewDto {
    let prompt = payload.get("prompt").and_then(|value| match value {
        Value::Null => None,
        Value::String(text) => Some(text.clone()),
        other => Some(other.to_string()),
    });

    let mut parameters = Vec::new();
    if let Some(root) = payload.as_object() {
        for (key, value) in root {
            if key == "prompt" {
                continue;
            }
            parameters.push(ParameterViewDto {
                key: key.clone(),
                path: pointer_push("", key),
                value: value.clone(),
            });
        }
    }

    RequestViewDto {
        kind: RequestKindDto::Text,
        http: None,
        system_field: None,
        system_path: None,
        system: Vec::new(),
        messages: Vec::new(),
        tools: Vec::new(),
        tool_choice: None,
        parameters,
        prompt,
    }
}

/// The candidates of a text completion: OpenAI-style `choices`, llama.cpp's array of
/// outputs, or a single llama.cpp or Ollama output.
pub(super) fn project_text_candidates(
    document: &Value,
    base_path: Option<&str>,
) -> (Vec<MessageViewDto>, Option<String>) {
    let outputs: Vec<(&Value, Option<String>)> = match document {
        Value::Array(outputs) => outputs
            .iter()
            .enumerate()
            .map(|(index, output)| (output, base_path.map(|base| pointer_index(base, index))))
            .collect(),
        _ => match document.get("choices").and_then(Value::as_array) {
            Some(choices) => {
                let choices_path = base_path.map(|base| pointer_push(base, "choices"));
                choices
                    .iter()
                    .enumerate()
                    .map(|(index, choice)| {
                        let path = choices_path
                            .as_deref()
                            .map(|base| pointer_index(base, index));
                        (choice, path)
                    })
                    .collect()
            }
            None => vec![(document, base_path.map(str::to_string))],
        },
    };

    let finish_reason = outputs
        .iter()
        .find_map(|(output, _)| output_finish_reason(output));
    let candidates = outputs
        .into_iter()
        .map(|(output, path)| text_candidate(output, path))
        .collect();
    (candidates, finish_reason)
}

fn text_candidate(output: &Value, path: Option<String>) -> MessageViewDto {
    let mut parts = Vec::new();
    if let Some((key, text)) = first_field(output, &["reasoning", "thinking"]) {
        parts.push(PartViewDto::Reasoning {
            text: text.to_string(),
            path: path.as_deref().map(|base| pointer_push(base, key)),
        });
    }
    if let Some((key, text)) = first_field(output, &["text", "content", "response"]) {
        parts.push(PartViewDto::Text {
            text: text.to_string(),
            cache_breakpoint: false,
            path: path.as_deref().map(|base| pointer_push(base, key)),
        });
    }
    MessageViewDto {
        role: "assistant".to_string(),
        name: None,
        path,
        parts,
    }
}

fn first_field<'a>(output: &'a Value, keys: &[&'static str]) -> Option<(&'static str, &'a str)> {
    keys.iter().find_map(|key| {
        output
            .get(*key)
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
            .map(|text| (*key, text))
    })
}

fn output_finish_reason(output: &Value) -> Option<String> {
    for key in ["stop_type", "done_reason", "finish_reason"] {
        if let Some(reason) = output
            .get(key)
            .and_then(Value::as_str)
            .filter(|reason| !reason.is_empty())
        {
            return Some(reason.to_string());
        }
    }
    None
}
