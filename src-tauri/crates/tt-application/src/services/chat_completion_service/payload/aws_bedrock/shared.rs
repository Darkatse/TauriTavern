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

use std::collections::HashMap;

use serde_json::{Map, Value, json};

use crate::errors::ApplicationError;

use super::super::content_parts::reject_media_for_text_only_provider;
use super::super::shared::message_content_to_text;
use super::super::tool_calls::{
    answered_tool_name, extract_openai_tool_calls, message_tool_call_id,
};

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

/// These models take no tool turns, so calls and results travel as text: a call reads as
/// `name(arguments)` after the assistant's text, and a result as user text naming its tool.
#[derive(Default)]
struct ToolTurnText {
    call_names: HashMap<String, String>,
}

impl ToolTurnText {
    fn render(&mut self, role: &str, message: &Map<String, Value>) -> String {
        let text = message_content_to_text(message.get("content"));
        match role {
            "assistant" => {
                let mut lines = Vec::new();
                if !text.is_empty() {
                    lines.push(text);
                }
                for call in extract_openai_tool_calls(message.get("tool_calls")) {
                    lines.push(format!(
                        "{}({})",
                        call.name,
                        call.arguments.encode_for_replay()
                    ));
                    self.call_names.insert(call.id, call.name);
                }
                lines.join("\n")
            }
            "tool" | "function" if !text.is_empty() => {
                let call_name = message_tool_call_id(message)
                    .and_then(|id| self.call_names.get(&id).map(String::as_str));
                match answered_tool_name(call_name, message) {
                    Some(name) => format!("{name} returned:\n{text}"),
                    None => format!("Tool returned:\n{text}"),
                }
            }
            _ => text,
        }
    }
}

/// Flatten an OpenAI-style `messages` array into a `(system_text, [user/assistant turns])`
/// pair. System messages are concatenated with `\n\n` and pulled out of the
/// conversation list — every Bedrock non-Anthropic provider treats `system`
/// as a separate top-level field (Nova `system`, Llama `<|start_header_id|>system`,
/// Cohere `preamble`, AI21 `role:"system"`, DeepSeek prompt prefix, ...).
/// Tool calls and results become text (see `ToolTurnText`).
pub(super) fn flatten_openai_messages(
    messages: Option<&Value>,
) -> Result<(Option<String>, Vec<FlatMessage>), ApplicationError> {
    reject_media_for_text_only_provider("AWS Bedrock text-only invoke", messages)?;

    let mut system_parts: Vec<String> = Vec::new();
    let mut turns: Vec<FlatMessage> = Vec::new();

    let Some(messages) = messages else {
        return Ok((None, turns));
    };

    if let Some(prompt) = messages.as_str() {
        turns.push(FlatMessage {
            role: "user".to_string(),
            text: prompt.to_string(),
        });
        return Ok((None, turns));
    }

    let Some(entries) = messages.as_array() else {
        return Ok((None, turns));
    };

    let mut tool_turns = ToolTurnText::default();
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
        let text = tool_turns.render(&role, message);
        if text.is_empty() {
            continue;
        }

        match role.as_str() {
            "system" | "developer" => system_parts.push(text),
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

    Ok((system_text, turns))
}

/// Pass an OpenAI-shape messages array through to a chat-completion endpoint
/// mostly verbatim. We collapse multi-part content into plain text because
/// none of the Bedrock chat-completion `/invoke` schemas document content-block
/// arrays (multimodal goes through the Converse API).
///
/// Roles are folded to the OpenAI core set (`system`/`user`/`assistant`);
/// tool results become `user` text (see `ToolTurnText`) so providers that don't
/// grok the `tool` role (Mistral, AI21 Jamba on Bedrock, ...) don't reject the call.
pub(super) fn passthrough_chat_messages(
    messages: Option<&Value>,
) -> Result<Vec<Value>, ApplicationError> {
    reject_media_for_text_only_provider("AWS Bedrock text-only invoke", messages)?;

    let mut out = Vec::new();
    let Some(messages) = messages else {
        return Ok(out);
    };

    if let Some(prompt) = messages.as_str() {
        out.push(json!({ "role": "user", "content": prompt }));
        return Ok(out);
    }

    let Some(entries) = messages.as_array() else {
        return Ok(out);
    };

    let mut tool_turns = ToolTurnText::default();
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
        let text = tool_turns.render(&role, message);
        if text.is_empty() {
            continue;
        }
        let role = match role.as_str() {
            "system" | "developer" => "system",
            "assistant" => "assistant",
            _ => "user",
        };
        out.push(json!({ "role": role, "content": text }));
    }

    Ok(out)
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

        let (system, turns) =
            flatten_openai_messages(Some(&messages)).expect("messages should flatten");
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
                    text: "Tool returned:\ntool result".to_string(),
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
        let projected = passthrough_chat_messages(Some(&messages)).expect("messages should pass");
        assert_eq!(projected.len(), 3);
        assert_eq!(projected[0]["role"], "system");
        assert_eq!(projected[1]["role"], "user");
        assert_eq!(projected[1]["content"], "Tool returned:\ntool output");
        assert_eq!(projected[2]["role"], "assistant");
    }

    #[test]
    fn tool_turns_become_text_that_names_the_tool() {
        let messages = json!([
            { "role": "user", "content": "How is Mara?" },
            { "role": "assistant", "content": "", "tool_calls": [{
                "id": "call_1", "type": "function",
                "function": { "name": "read_state", "arguments": "{\"target\":\"Mara\"}" }
            }] },
            { "role": "tool", "tool_call_id": "call_1", "content": "hp: 3" },
            { "role": "tool", "name": "roll", "content": "4" }
        ]);
        let expected = [
            ("user", "How is Mara?"),
            ("assistant", "read_state({\"target\":\"Mara\"})"),
            ("user", "read_state returned:\nhp: 3"),
            ("user", "roll returned:\n4"),
        ];

        let (_, turns) = flatten_openai_messages(Some(&messages)).expect("messages should flatten");
        let flattened: Vec<_> = turns
            .iter()
            .map(|turn| (turn.role.as_str(), turn.text.as_str()))
            .collect();
        assert_eq!(flattened, expected);

        let passed = passthrough_chat_messages(Some(&messages)).expect("messages should pass");
        let passed: Vec<_> = passed
            .iter()
            .map(|message| {
                (
                    message["role"].as_str().unwrap(),
                    message["content"].as_str().unwrap(),
                )
            })
            .collect();
        assert_eq!(passed, expected);
    }

    #[test]
    fn text_only_bedrock_helpers_reject_media_parts() {
        let messages = json!([{
            "role": "user",
            "content": [
                { "type": "text", "text": "describe" },
                { "type": "image_url", "image_url": { "url": "data:image/png;base64,AAAA" } }
            ]
        }]);

        let error = flatten_openai_messages(Some(&messages)).expect_err("media should fail fast");
        assert!(error.to_string().contains("cannot preserve image input"));
    }
}
