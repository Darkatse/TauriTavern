use std::collections::HashMap;

use serde_json::{Map, Value, json};
use uuid::Uuid;

const PROMPT_PLACEHOLDER: &str = "Let's get started.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PromptProcessingType {
    None,
    Merge,
    MergeTools,
    Semi,
    SemiTools,
    Strict,
    StrictTools,
    Single,
}

impl PromptProcessingType {
    pub(super) fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "" => Self::None,
            "claude" => Self::Merge,
            "merge" => Self::Merge,
            "merge_tools" => Self::MergeTools,
            "semi" => Self::Semi,
            "semi_tools" => Self::SemiTools,
            "strict" => Self::Strict,
            "strict_tools" => Self::StrictTools,
            "single" => Self::Single,
            _ => Self::None,
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct PromptNames {
    pub(super) char_name: String,
    pub(super) user_name: String,
    pub(super) group_names: Vec<String>,
}

impl PromptNames {
    pub(super) fn from_payload(payload: &Map<String, Value>) -> Self {
        let char_name = payload
            .get("char_name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string();
        let user_name = payload
            .get("user_name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string();
        let group_names = payload
            .get("group_names")
            .and_then(Value::as_array)
            .map(|entries| {
                entries
                    .iter()
                    .filter_map(|value| value.as_str())
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        Self {
            char_name,
            user_name,
            group_names,
        }
    }

    pub(super) fn starts_with_group_name(&self, message: &str) -> bool {
        self.group_names
            .iter()
            .any(|name| !name.is_empty() && message.starts_with(&format!("{name}: ")))
    }
}

#[derive(Debug, Clone, Copy)]
struct MergeOptions {
    strict: bool,
    placeholders: bool,
    single: bool,
    tools: bool,
}

pub(super) fn apply_custom_prompt_post_processing(payload: &mut Map<String, Value>) {
    let post_processing = payload
        .get("custom_prompt_post_processing")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let post_processing = PromptProcessingType::parse(post_processing);
    if post_processing == PromptProcessingType::None {
        return;
    }

    let names = PromptNames::from_payload(payload);

    let Some(messages) = payload.get_mut("messages").and_then(Value::as_array_mut) else {
        return;
    };

    let messages = std::mem::take(messages);
    let processed = post_process_prompt(messages, post_processing, &names);
    payload.insert("messages".to_string(), Value::Array(processed));
}

pub(super) fn post_process_prompt(
    messages: Vec<Value>,
    processing: PromptProcessingType,
    names: &PromptNames,
) -> Vec<Value> {
    match processing {
        PromptProcessingType::None => messages,
        PromptProcessingType::Merge => merge_messages(
            messages,
            names,
            MergeOptions {
                strict: false,
                placeholders: false,
                single: false,
                tools: false,
            },
        ),
        PromptProcessingType::MergeTools => merge_messages(
            messages,
            names,
            MergeOptions {
                strict: false,
                placeholders: false,
                single: false,
                tools: true,
            },
        ),
        PromptProcessingType::Semi => merge_messages(
            messages,
            names,
            MergeOptions {
                strict: true,
                placeholders: false,
                single: false,
                tools: false,
            },
        ),
        PromptProcessingType::SemiTools => merge_messages(
            messages,
            names,
            MergeOptions {
                strict: true,
                placeholders: false,
                single: false,
                tools: true,
            },
        ),
        PromptProcessingType::Strict => merge_messages(
            messages,
            names,
            MergeOptions {
                strict: true,
                placeholders: true,
                single: false,
                tools: false,
            },
        ),
        PromptProcessingType::StrictTools => merge_messages(
            messages,
            names,
            MergeOptions {
                strict: true,
                placeholders: true,
                single: false,
                tools: true,
            },
        ),
        PromptProcessingType::Single => merge_messages(
            messages,
            names,
            MergeOptions {
                strict: true,
                placeholders: false,
                single: true,
                tools: false,
            },
        ),
    }
}

fn merge_messages(messages: Vec<Value>, names: &PromptNames, options: MergeOptions) -> Vec<Value> {
    let mut content_tokens: HashMap<String, Value> = HashMap::new();

    let mut normalized = Vec::with_capacity(messages.len());
    for mut message in messages {
        let Some(message_object) = message.as_object_mut() else {
            continue;
        };

        if !message_object.contains_key("content") || message_object.get("content").is_some_and(Value::is_null) {
            message_object.insert("content".to_string(), Value::String(String::new()));
        }

        if let Some(content) = message_object.get_mut("content") {
            normalize_message_content(content, &mut content_tokens);
        }

        let role = message_object
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("user")
            .trim()
            .to_ascii_lowercase();
        let name = message_object.get("name").and_then(Value::as_str).map(str::trim);
        let content_text = message_object
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();

        let content_text = if role == "system" && name == Some("example_assistant") {
            prefix_if_missing(
                &content_text,
                &names.char_name,
                |content| names.starts_with_group_name(content),
            )
        } else if role == "system" && name == Some("example_user") {
            prefix_if_missing(&content_text, &names.user_name, |_| false)
        } else {
            content_text
        };

        let mut content_text = if let Some(name) = name.filter(|_| role != "system") {
            let prefix = format!("{name}: ");
            if content_text.starts_with(&prefix) {
                content_text
            } else {
                format!("{prefix}{content_text}")
            }
        } else {
            content_text
        };

        let mut role = role;
        if role == "tool" && !options.tools {
            role = "user".to_string();
        }

        if options.single {
            if role == "assistant" {
                content_text = prefix_if_missing(
                    &content_text,
                    &names.char_name,
                    |content| names.starts_with_group_name(content),
                );
            } else if role == "user" {
                content_text = prefix_if_missing(&content_text, &names.user_name, |_| false);
            }
            role = "user".to_string();
        }

        message_object.insert("role".to_string(), Value::String(role));
        message_object.insert("content".to_string(), Value::String(content_text));
        message_object.remove("name");

        if !options.tools {
            message_object.remove("tool_calls");
            message_object.remove("tool_call_id");
        }

        normalized.push(message);
    }

    let mut merged: Vec<Value> = Vec::new();
    for message in normalized {
        let can_merge = (|| {
            let Some(current_object) = message.as_object() else {
                return false;
            };

            let current_role = current_object
                .get("role")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if current_role == "tool" {
                return false;
            }

            let current_content = current_object
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if current_content.is_empty() {
                return false;
            }

            let Some(last_object) = merged.last().and_then(Value::as_object) else {
                return false;
            };

            last_object
                .get("role")
                .and_then(Value::as_str)
                .unwrap_or_default()
                == current_role
        })();

        if can_merge {
            let current_content = message
                .as_object()
                .and_then(|object| object.get("content"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();

            if let Some(last_object) = merged.last_mut().and_then(Value::as_object_mut) {
                let previous_content = last_object
                    .get("content")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let joined = format!("{previous_content}\n\n{current_content}");
                last_object.insert("content".to_string(), Value::String(joined));
                continue;
            }
        }

        merged.push(message);
    }

    if merged.is_empty() {
        merged.push(json!({
            "role": "user",
            "content": PROMPT_PLACEHOLDER,
        }));
    }

    if !content_tokens.is_empty() {
        for message in merged.iter_mut() {
            let Some(message_object) = message.as_object_mut() else {
                continue;
            };

            let Some(content) = message_object.get("content").and_then(Value::as_str) else {
                continue;
            };
            let content = content.to_string();

            let has_token = content_tokens.keys().any(|token| content.contains(token));
            if !has_token {
                continue;
            }

            let segments = content.split("\n\n");
            let mut parts: Vec<Value> = Vec::new();

            for segment in segments {
                if let Some(media) = content_tokens.get(segment) {
                    parts.push(media.clone());
                    continue;
                }

                let appended = match parts.last_mut().and_then(Value::as_object_mut) {
                    Some(last_object)
                        if last_object.get("type").and_then(Value::as_str) == Some("text") =>
                    {
                        let previous = last_object
                            .get("text")
                            .and_then(Value::as_str)
                            .unwrap_or_default();
                        last_object.insert(
                            "text".to_string(),
                            Value::String(format!("{previous}\n\n{segment}")),
                        );
                        true
                    }
                    _ => false,
                };

                if !appended {
                    parts.push(json!({ "type": "text", "text": segment }));
                }
            }

            message_object.insert("content".to_string(), Value::Array(parts));
        }
    }

    if options.strict {
        for (idx, message) in merged.iter_mut().enumerate() {
            if idx == 0 {
                continue;
            }
            let Some(message_object) = message.as_object_mut() else {
                continue;
            };
            if message_object.get("role").and_then(Value::as_str) == Some("system") {
                message_object.insert("role".to_string(), Value::String("user".to_string()));
            }
        }

        if options.placeholders && !merged.is_empty() {
            let first_role = merged
                .first()
                .and_then(Value::as_object)
                .and_then(|object| object.get("role"))
                .and_then(Value::as_str)
                .unwrap_or_default();

            let second_role = merged
                .get(1)
                .and_then(Value::as_object)
                .and_then(|object| object.get("role"))
                .and_then(Value::as_str);

            if first_role == "system" && second_role != Some("user") {
                merged.insert(
                    1,
                    json!({
                        "role": "user",
                        "content": PROMPT_PLACEHOLDER,
                    }),
                );
            } else if first_role != "system" && first_role != "user" {
                merged.insert(
                    0,
                    json!({
                        "role": "user",
                        "content": PROMPT_PLACEHOLDER,
                    }),
                );
            }
        }

        return merge_messages(
            merged,
            names,
            MergeOptions {
                strict: false,
                placeholders: options.placeholders,
                single: false,
                tools: options.tools,
            },
        );
    }

    merged
}

fn normalize_message_content(content: &mut Value, content_tokens: &mut HashMap<String, Value>) {
    match content {
        Value::String(_) => {}
        Value::Array(parts) => {
            let mut segments = Vec::with_capacity(parts.len());
            for part in parts.iter() {
                match part {
                    Value::String(fragment) => segments.push(fragment.clone()),
                    Value::Object(object) => match object.get("type").and_then(Value::as_str) {
                        Some("text") => segments.push(
                            object
                                .get("text")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_string(),
                        ),
                        Some("image_url") | Some("video_url") | Some("audio_url") => {
                            let token = format!("__TAURITAVERN_MEDIA_TOKEN_{}__", Uuid::new_v4());
                            content_tokens.insert(token.clone(), part.clone());
                            segments.push(token);
                        }
                        _ => segments.push(String::new()),
                    },
                    _ => segments.push(String::new()),
                }
            }
            *content = Value::String(segments.join("\n\n"));
        }
        Value::Null => {
            *content = Value::String(String::new());
        }
        ref other => {
            *content = Value::String(other.to_string());
        }
    }
}

fn prefix_if_missing(
    content: &str,
    prefix: &str,
    should_skip: impl FnOnce(&str) -> bool,
) -> String {
    let prefix = prefix.trim();
    if prefix.is_empty() {
        return content.to_string();
    }

    let decorated = format!("{prefix}: ");
    if content.starts_with(&decorated) || should_skip(content) {
        return content.to_string();
    }

    format!("{decorated}{content}")
}

pub(super) fn add_reasoning_content_to_tool_calls(messages: &mut [Value]) {
    for message in messages {
        let Some(message_object) = message.as_object_mut() else {
            continue;
        };

        let has_tool_calls = message_object
            .get("tool_calls")
            .and_then(Value::as_array)
            .is_some_and(|calls| !calls.is_empty());
        if !has_tool_calls || message_object.contains_key("reasoning_content") {
            continue;
        }

        message_object.insert(
            "reasoning_content".to_string(),
            Value::String(String::new()),
        );
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::{PromptNames, PromptProcessingType, post_process_prompt};

    #[test]
    fn merge_squashes_consecutive_roles() {
        let names = PromptNames {
            char_name: String::new(),
            user_name: String::new(),
            group_names: Vec::new(),
        };
        let messages = vec![
            json!({"role":"user","content":"a"}),
            json!({"role":"user","content":"b"}),
            json!({"role":"assistant","content":"c"}),
            json!({"role":"assistant","content":"d"}),
            json!({"role":"user","content":"e"}),
        ];

        let merged = post_process_prompt(messages, PromptProcessingType::Merge, &names);
        let merged = Value::Array(merged);

        let roles: Vec<String> = merged
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|msg| msg.get("role").and_then(Value::as_str).map(|s| s.to_string()))
            .collect();
        assert_eq!(roles, vec!["user", "assistant", "user"]);

        let first_content = merged
            .as_array()
            .unwrap()
            .first()
            .and_then(|msg| msg.get("content"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        assert_eq!(first_content, "a\n\nb");
    }

    #[test]
    fn strict_inserts_placeholder_user_first() {
        let names = PromptNames {
            char_name: String::new(),
            user_name: String::new(),
            group_names: Vec::new(),
        };
        let messages = vec![json!({"role":"assistant","content":"prefill"})];

        let merged = post_process_prompt(messages, PromptProcessingType::Strict, &names);
        let roles: Vec<String> = merged
            .iter()
            .filter_map(|msg| msg.get("role").and_then(Value::as_str).map(|s| s.to_string()))
            .collect();
        assert_eq!(roles, vec!["user", "assistant"]);
    }
}
