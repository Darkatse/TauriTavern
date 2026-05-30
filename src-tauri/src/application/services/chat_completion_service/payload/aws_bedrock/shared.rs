//! Cross-provider helpers shared by every Bedrock builder branch.
//!
//! The non-Anthropic providers (Nova, Llama, Mistral, DeepSeek, Cohere,
//! AI21 Jamba, ...) all start from the same OpenAI-shape `messages` array
//! and need a common way to:
//!
//! 1. Flatten that array into a `(system_text, [user/assistant turns])`
//!    pair so each provider can re-shape it (Nova `system`, Llama prompt
//!    template, Cohere `chat_history` + `preamble`, ...).
//! 2. Optionally pass an OpenAI-shape messages array through (Mistral chat,
//!    DeepSeek V3+, AI21 Jamba).
//! 3. Coerce numeric fields without panicking on non-numbers.
//!
//! Provider-specific prompt templates (Llama 3, Mistral instruct, DeepSeek
//! R1, ...) live in the matching submodule alongside their `build_*` entry
//! point.

use serde_json::{Value, json};

use super::super::shared::message_content_to_text;

/// Bedrock invoke path suffix — the body posts to
/// `/model/{modelId}/invoke` and the streaming variant rewrites the tail to
/// `invoke-with-response-stream` in the infrastructure layer.
pub(super) const BEDROCK_INVOKE_SUFFIX: &str = "invoke";

/// Lightweight per-message representation used by every non-Anthropic builder.
/// Provider-native shapes (Nova content blocks, Llama prompt template, Cohere
/// chat_history, ...) are reconstructed from this flat list in each builder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct FlatMessage {
    pub role: String,
    pub text: String,
}

/// Flatten an OpenAI-style `messages` array into a `(system_text, [user/assistant turns])`
/// pair. System messages are concatenated with `\n\n` and pulled out of the
/// conversation list — every Bedrock non-Anthropic provider treats `system`
/// as a separate top-level field (Nova `system`, Llama `<|start_header_id|>system`,
/// Cohere `preamble`, AI21 `role:"system"`, DeepSeek prompt prefix, ...).
///
/// `tool` role messages are demoted to `user` text with a `[tool_result] ...`
/// envelope. We could later add per-provider tool-call wiring, but for the
/// initial multi-provider release we focus on plain chat.
pub(super) fn flatten_openai_messages(
    messages: Option<&Value>,
) -> (Option<String>, Vec<FlatMessage>) {
    let mut system_parts: Vec<String> = Vec::new();
    let mut turns: Vec<FlatMessage> = Vec::new();

    let Some(messages) = messages else {
        return (None, turns);
    };

    if let Some(prompt) = messages.as_str() {
        turns.push(FlatMessage {
            role: "user".to_string(),
            text: prompt.to_string(),
        });
        return (None, turns);
    }

    let Some(entries) = messages.as_array() else {
        return (None, turns);
    };

    for entry in entries {
        let Some(message) = entry.as_object() else {
            continue;
        };

        let role = message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("user")
            .trim()
            .to_lowercase();
        let text = message_content_to_text(message.get("content"));
        if text.is_empty() {
            continue;
        }

        match role.as_str() {
            "system" | "developer" => system_parts.push(text),
            "tool" | "function" => turns.push(FlatMessage {
                role: "user".to_string(),
                text: format!("[tool_result] {text}"),
            }),
            "assistant" => turns.push(FlatMessage {
                role: "assistant".to_string(),
                text,
            }),
            _ => turns.push(FlatMessage {
                role: "user".to_string(),
                text,
            }),
        }
    }

    let system_text = if system_parts.is_empty() {
        None
    } else {
        Some(system_parts.join("\n\n"))
    };

    (system_text, turns)
}

/// Pass an OpenAI-shape messages array through to a chat-completion endpoint
/// mostly verbatim. We collapse multi-part content into plain text because
/// none of the Bedrock chat-completion `/invoke` schemas document content-block
/// arrays (multimodal goes through the Converse API).
///
/// Roles are folded to the OpenAI core set (`system`/`user`/`assistant`);
/// `tool`/`function` are demoted to `user` so providers that don't grok the
/// `tool` role (Mistral, AI21 Jamba on Bedrock, ...) don't reject the call.
pub(super) fn passthrough_chat_messages(messages: Option<&Value>) -> Vec<Value> {
    let mut out = Vec::new();
    let Some(messages) = messages else { return out };

    if let Some(prompt) = messages.as_str() {
        out.push(json!({ "role": "user", "content": prompt }));
        return out;
    }

    let Some(entries) = messages.as_array() else {
        return out;
    };

    for entry in entries {
        let Some(message) = entry.as_object() else {
            continue;
        };
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("user")
            .trim()
            .to_lowercase();
        let role = match role.as_str() {
            "system" | "developer" => "system",
            "assistant" => "assistant",
            "tool" | "function" => "user",
            _ => "user",
        };
        let text = message_content_to_text(message.get("content"));
        if text.is_empty() {
            continue;
        }
        out.push(json!({ "role": role, "content": text }));
    }

    out
}

/// Coerce an optional JSON value to a strictly positive `i64`, dropping
/// zero / negative / non-integer inputs.
pub(super) fn value_to_positive_i64(value: Option<&Value>) -> Option<i64> {
    value.and_then(Value::as_i64).filter(|number| *number > 0)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{FlatMessage, flatten_openai_messages, passthrough_chat_messages};

    #[test]
    fn flatten_openai_messages_extracts_system_and_normalizes_roles() {
        let messages = json!([
            { "role": "system", "content": "rules apply" },
            { "role": "developer", "content": "extra system" },
            { "role": "user", "content": "hi" },
            { "role": "assistant", "content": "hello" },
            { "role": "tool", "content": "tool result" }
        ]);

        let (system, turns) = flatten_openai_messages(Some(&messages));
        assert_eq!(system.as_deref(), Some("rules apply\n\nextra system"));
        assert_eq!(
            turns,
            vec![
                FlatMessage {
                    role: "user".to_string(),
                    text: "hi".to_string()
                },
                FlatMessage {
                    role: "assistant".to_string(),
                    text: "hello".to_string()
                },
                FlatMessage {
                    role: "user".to_string(),
                    text: "[tool_result] tool result".to_string(),
                },
            ]
        );
    }

    #[test]
    fn passthrough_chat_messages_demotes_tool_role_to_user() {
        let messages = json!([
            { "role": "system", "content": "be terse" },
            { "role": "tool", "content": "tool output" },
            { "role": "assistant", "content": "ack" }
        ]);
        let projected = passthrough_chat_messages(Some(&messages));
        assert_eq!(projected.len(), 3);
        assert_eq!(projected[0]["role"], "system");
        assert_eq!(projected[1]["role"], "user");
        assert_eq!(projected[1]["content"], "tool output");
        assert_eq!(projected[2]["role"], "assistant");
    }
}
