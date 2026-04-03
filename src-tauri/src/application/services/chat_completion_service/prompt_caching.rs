use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use crate::domain::repositories::prompt_cache_repository::PromptDigestSnapshot;

const PROMPT_CACHE_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClaudeDigestLocation {
    Tool { index: usize },
    System { index: usize },
    Message { message_index: usize, block_index: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OpenRouterDigestLocation {
    Tool { index: usize },
    Message { message_index: usize, part_index: usize },
}

pub(super) fn apply_claude_prompt_caching(
    payload: &mut Value,
    previous: Option<&PromptDigestSnapshot>,
    ttl: &str,
) -> PromptDigestSnapshot {
    let (locations, digests) = collect_claude_digests(payload);
    let last_message_location = last_claude_message_location(&locations);
    let snapshot = PromptDigestSnapshot {
        version: PROMPT_CACHE_VERSION,
        digests,
    };

    if snapshot.digests.is_empty() {
        return snapshot;
    }

    let Some(object) = payload.as_object_mut() else {
        return snapshot;
    };

    object.insert("cache_control".to_string(), cache_control(ttl));

    let system_location = find_claude_system_break_location(payload);
    if let Some(location) = system_location {
        insert_cache_control_claude(payload, location, ttl);
    }

    let pre_history_location = find_claude_pre_history_break_location(payload);
    if let Some(location) = pre_history_location {
        if Some(location) != last_message_location {
            insert_cache_control_claude(payload, location, ttl);
        }
    }

    if let Some(previous) = previous.filter(|snapshot| snapshot.version == PROMPT_CACHE_VERSION)
    {
        let lcp_len = common_prefix_len(&previous.digests, &snapshot.digests);
        if lcp_len > 0 {
            let candidate = locations.get(lcp_len - 1).copied();
            if let Some(candidate) = candidate {
                let is_auto_last = Some(candidate) == last_message_location;
                let is_duplicate = Some(candidate) == system_location
                    || Some(candidate) == pre_history_location
                    || is_auto_last;

                if !is_duplicate
                    && matches!(candidate, ClaudeDigestLocation::Message { .. })
                {
                    insert_cache_control_claude(payload, candidate, ttl);
                }
            }
        }
    }

    snapshot
}

pub(super) fn apply_openrouter_claude_prompt_caching(
    payload: &mut Value,
    previous: Option<&PromptDigestSnapshot>,
    ttl: &str,
) -> PromptDigestSnapshot {
    let (locations, snapshot) = collect_openrouter_digests(payload);
    if snapshot.digests.is_empty() {
        return snapshot;
    }

    let Some(object) = payload.as_object_mut() else {
        return snapshot;
    };

    object.insert("cache_control".to_string(), cache_control(ttl));

    let Some(messages) = object.get_mut("messages").and_then(Value::as_array_mut) else {
        return snapshot;
    };

    let system_location = find_openrouter_system_break_location(messages);
    let last_location = last_openrouter_message_location(messages);
    if let Some(location) = system_location {
        if Some(location) != last_location {
            insert_cache_control_openrouter(messages, location, ttl);
        }
    }

    let pre_history_location =
        find_openrouter_pre_history_break_location(messages, system_location);
    if let Some(location) = pre_history_location {
        if Some(location) != last_location {
            insert_cache_control_openrouter(messages, location, ttl);
        }
    }

    if let Some(previous) = previous.filter(|snapshot| snapshot.version == PROMPT_CACHE_VERSION)
    {
        let lcp_len = common_prefix_len(&previous.digests, &snapshot.digests);
        if lcp_len > 0 {
            let candidate = locations.get(lcp_len - 1).copied();
            if let Some(candidate) = candidate {
                let is_duplicate = Some(candidate) == system_location
                    || Some(candidate) == pre_history_location
                    || Some(candidate) == last_location;

                if !is_duplicate
                    && matches!(candidate, OpenRouterDigestLocation::Message { .. })
                {
                    insert_cache_control_openrouter(messages, candidate, ttl);
                }
            }
        }
    }

    snapshot
}

fn collect_claude_digests(payload: &Value) -> (Vec<ClaudeDigestLocation>, Vec<String>) {
    let mut locations = Vec::new();
    let mut digests = Vec::new();

    let Some(object) = payload.as_object() else {
        return (locations, digests);
    };

    if let Some(tools) = object.get("tools").and_then(Value::as_array) {
        for (index, tool) in tools.iter().enumerate() {
            if !tool.is_object() {
                continue;
            }

            locations.push(ClaudeDigestLocation::Tool { index });
            digests.push(digest_value(&json!({
                "segment": "tools",
                "tool": strip_cache_control(tool),
            })));
        }
    }

    if let Some(system) = object.get("system").and_then(Value::as_array) {
        for (index, block) in system.iter().enumerate() {
            if !is_cache_control_eligible_block(block) {
                continue;
            }

            locations.push(ClaudeDigestLocation::System { index });
            digests.push(digest_value(&json!({
                "segment": "system",
                "block": strip_cache_control(block),
            })));
        }
    }

    if let Some(messages) = object.get("messages").and_then(Value::as_array) {
        for (message_index, message) in messages.iter().enumerate() {
            let Some(message_object) = message.as_object() else {
                continue;
            };

            let role = message_object
                .get("role")
                .and_then(Value::as_str)
                .unwrap_or_default();

            let Some(content) = message_object.get("content").and_then(Value::as_array) else {
                continue;
            };

            for (block_index, block) in content.iter().enumerate() {
                if !is_cache_control_eligible_block(block) {
                    continue;
                }

                locations.push(ClaudeDigestLocation::Message {
                    message_index,
                    block_index,
                });
                digests.push(digest_value(&json!({
                    "segment": "messages",
                    "role": role,
                    "block": strip_cache_control(block),
                })));
            }
        }
    }

    (locations, digests)
}

fn collect_openrouter_digests(
    payload: &mut Value,
) -> (Vec<OpenRouterDigestLocation>, PromptDigestSnapshot) {
    let mut locations = Vec::new();
    let mut digests = Vec::new();

    let Some(object) = payload.as_object_mut() else {
        return (
            locations,
            PromptDigestSnapshot {
                version: PROMPT_CACHE_VERSION,
                digests,
            },
        );
    };

    if let Some(tools) = object.get("tools").and_then(Value::as_array) {
        for (index, tool) in tools.iter().enumerate() {
            if !tool.is_object() {
                continue;
            }

            locations.push(OpenRouterDigestLocation::Tool { index });
            digests.push(digest_value(&json!({
                "segment": "tools",
                "tool": strip_cache_control(tool),
            })));
        }
    }

    let Some(messages) = object.get_mut("messages").and_then(Value::as_array_mut) else {
        return (
            locations,
            PromptDigestSnapshot {
                version: PROMPT_CACHE_VERSION,
                digests,
            },
        );
    };

    for (message_index, message) in messages.iter_mut().enumerate() {
        let Some(message_object) = message.as_object_mut() else {
            continue;
        };

        ensure_openrouter_message_content_parts(message_object);

        let role = message_object
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or_default();

        let Some(content) = message_object.get("content").and_then(Value::as_array) else {
            continue;
        };

        for (part_index, part) in content.iter().enumerate() {
            if !is_cache_control_eligible_block(part) {
                continue;
            }

            locations.push(OpenRouterDigestLocation::Message {
                message_index,
                part_index,
            });
            digests.push(digest_value(&json!({
                "segment": "messages",
                "role": role,
                "block": strip_cache_control(part),
            })));
        }
    }

    (
        locations,
        PromptDigestSnapshot {
            version: PROMPT_CACHE_VERSION,
            digests,
        },
    )
}

fn find_claude_system_break_location(payload: &Value) -> Option<ClaudeDigestLocation> {
    let system = payload
        .as_object()?
        .get("system")
        .and_then(Value::as_array)?;

    for (index, block) in system.iter().enumerate().rev() {
        if is_cache_control_eligible_block(block) {
            return Some(ClaudeDigestLocation::System { index });
        }
    }

    None
}

fn find_claude_pre_history_break_location(payload: &Value) -> Option<ClaudeDigestLocation> {
    let messages = payload
        .as_object()?
        .get("messages")
        .and_then(Value::as_array)?;

    let first_message = messages.first()?.as_object()?;
    let content = first_message.get("content").and_then(Value::as_array)?;

    for (index, block) in content.iter().enumerate().rev() {
        if is_cache_control_eligible_block(block) {
            return Some(ClaudeDigestLocation::Message {
                message_index: 0,
                block_index: index,
            });
        }
    }

    None
}

fn last_claude_message_location(
    locations: &[ClaudeDigestLocation],
) -> Option<ClaudeDigestLocation> {
    locations.iter().rev().find_map(|location| {
        matches!(location, ClaudeDigestLocation::Message { .. }).then_some(*location)
    })
}

fn find_openrouter_system_break_location(
    messages: &[Value],
) -> Option<OpenRouterDigestLocation> {
    let mut last_system_message: Option<usize> = None;

    for (index, message) in messages.iter().enumerate() {
        let Some(message_object) = message.as_object() else {
            continue;
        };

        let role = message_object
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or_default();

        if role == "system" {
            last_system_message = Some(index);
            continue;
        }

        break;
    }

    let message_index = last_system_message?;
    let message_object = messages
        .get(message_index)
        .and_then(Value::as_object)?;
    let content = message_object.get("content").and_then(Value::as_array)?;

    for (part_index, part) in content.iter().enumerate().rev() {
        if is_cache_control_eligible_block(part) {
            return Some(OpenRouterDigestLocation::Message {
                message_index,
                part_index,
            });
        }
    }

    None
}

fn find_openrouter_pre_history_break_location(
    messages: &[Value],
    system_location: Option<OpenRouterDigestLocation>,
) -> Option<OpenRouterDigestLocation> {
    let system_message_index = system_location.and_then(|location| match location {
        OpenRouterDigestLocation::Message { message_index, .. } => Some(message_index),
        _ => None,
    });

    let start_index = system_message_index.map(|index| index + 1).unwrap_or(0);
    for (message_index, message) in messages.iter().enumerate().skip(start_index) {
        let Some(message_object) = message.as_object() else {
            continue;
        };

        let role = message_object
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or_default();

        if role == "system" {
            continue;
        }

        let content = message_object.get("content").and_then(Value::as_array)?;
        for (part_index, part) in content.iter().enumerate().rev() {
            if is_cache_control_eligible_block(part) {
                return Some(OpenRouterDigestLocation::Message {
                    message_index,
                    part_index,
                });
            }
        }

        break;
    }

    None
}

fn last_openrouter_message_location(
    messages: &[Value],
) -> Option<OpenRouterDigestLocation> {
    for (message_index, message) in messages.iter().enumerate().rev() {
        let Some(message_object) = message.as_object() else {
            continue;
        };

        let Some(content) = message_object.get("content").and_then(Value::as_array) else {
            continue;
        };

        for (part_index, part) in content.iter().enumerate().rev() {
            if is_cache_control_eligible_block(part) {
                return Some(OpenRouterDigestLocation::Message {
                    message_index,
                    part_index,
                });
            }
        }
    }

    None
}

fn insert_cache_control_claude(
    payload: &mut Value,
    location: ClaudeDigestLocation,
    ttl: &str,
) {
    match location {
        ClaudeDigestLocation::Tool { .. } => {}
        ClaudeDigestLocation::System { index } => {
            let Some(block) = payload
                .as_object_mut()
                .and_then(|object| object.get_mut("system"))
                .and_then(Value::as_array_mut)
                .and_then(|blocks| blocks.get_mut(index))
            else {
                return;
            };

            insert_cache_control_on_block(block, ttl);
        }
        ClaudeDigestLocation::Message {
            message_index,
            block_index,
        } => {
            let Some(message) = payload
                .as_object_mut()
                .and_then(|object| object.get_mut("messages"))
                .and_then(Value::as_array_mut)
                .and_then(|messages| messages.get_mut(message_index))
                .and_then(Value::as_object_mut)
            else {
                return;
            };

            let Some(block) = message
                .get_mut("content")
                .and_then(Value::as_array_mut)
                .and_then(|blocks| blocks.get_mut(block_index))
            else {
                return;
            };

            insert_cache_control_on_block(block, ttl);
        }
    }
}

fn insert_cache_control_openrouter(
    messages: &mut [Value],
    location: OpenRouterDigestLocation,
    ttl: &str,
) {
    let OpenRouterDigestLocation::Message {
        message_index,
        part_index,
    } = location
    else {
        return;
    };

    let Some(message_object) = messages
        .get_mut(message_index)
        .and_then(Value::as_object_mut)
    else {
        return;
    };

    let Some(block) = message_object
        .get_mut("content")
        .and_then(Value::as_array_mut)
        .and_then(|blocks| blocks.get_mut(part_index))
    else {
        return;
    };

    insert_cache_control_on_block(block, ttl);
}

fn insert_cache_control_on_block(block: &mut Value, ttl: &str) {
    let Some(object) = block.as_object_mut() else {
        return;
    };

    if object.contains_key("cache_control") {
        return;
    }

    object.insert("cache_control".to_string(), cache_control(ttl));
}

fn ensure_openrouter_message_content_parts(message: &mut Map<String, Value>) {
    let Some(content) = message.get_mut("content") else {
        return;
    };

    if let Value::String(text) = content {
        let text = std::mem::take(text);
        *content = Value::Array(vec![json!({
            "type": "text",
            "text": text,
        })]);
    }
}

fn common_prefix_len(previous: &[String], current: &[String]) -> usize {
    let max_len = previous.len().min(current.len());

    for index in 0..max_len {
        if previous[index] != current[index] {
            return index;
        }
    }

    max_len
}

fn is_cache_control_eligible_block(block: &Value) -> bool {
    let Some(object) = block.as_object() else {
        return false;
    };

    let block_type = object
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();

    if block_type.contains("thinking") {
        return false;
    }

    if block_type == "text" {
        let text = object.get("text").and_then(Value::as_str).unwrap_or_default();
        if text.is_empty() {
            return false;
        }
    }

    true
}

fn strip_cache_control(value: &Value) -> Value {
    let Value::Object(object) = value else {
        return value.clone();
    };

    let mut cloned = object.clone();
    cloned.remove("cache_control");
    Value::Object(cloned)
}

fn digest_value(value: &Value) -> String {
    let bytes = serde_json::to_vec(value).unwrap_or_default();
    let digest = Sha256::digest(&bytes);
    encode_hex(&digest)
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";

    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn cache_control(ttl: &str) -> Value {
    json!({
        "type": "ephemeral",
        "ttl": ttl,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::{apply_claude_prompt_caching, apply_openrouter_claude_prompt_caching};

    fn has_cache_control(value: &Value) -> bool {
        value
            .as_object()
            .is_some_and(|object| object.contains_key("cache_control"))
    }

    #[test]
    fn claude_prompt_caching_inserts_expected_breakpoints() {
        let mut payload = json!({
            "model": "claude-3-5-sonnet-latest",
            "system": [
                { "type": "text", "text": "sys1" },
                { "type": "text", "text": "sys2" }
            ],
            "messages": [
                { "role": "user", "content": [{ "type": "text", "text": "prehistory" }] },
                { "role": "assistant", "content": [{ "type": "text", "text": "a1" }] },
                { "role": "user", "content": [{ "type": "text", "text": "u1" }] }
            ]
        });

        let _snapshot = apply_claude_prompt_caching(&mut payload, None, "5m");

        let root = payload.as_object().expect("payload must be object");
        assert!(
            root.get("cache_control")
                .and_then(Value::as_object)
                .is_some_and(|cache_control| {
                    cache_control.get("type").and_then(Value::as_str) == Some("ephemeral")
                        && cache_control.get("ttl").and_then(Value::as_str) == Some("5m")
                })
        );

        let system = root
            .get("system")
            .and_then(Value::as_array)
            .expect("system must be array");
        assert!(has_cache_control(system.last().expect("system must not be empty")));

        let messages = root
            .get("messages")
            .and_then(Value::as_array)
            .expect("messages must be array");

        let pre_history_block = messages
            .first()
            .and_then(Value::as_object)
            .and_then(|message| message.get("content"))
            .and_then(Value::as_array)
            .and_then(|blocks| blocks.last())
            .expect("prehistory block must exist");
        assert!(has_cache_control(pre_history_block));

        let last_block = messages
            .last()
            .and_then(Value::as_object)
            .and_then(|message| message.get("content"))
            .and_then(Value::as_array)
            .and_then(|blocks| blocks.last())
            .expect("last message block must exist");
        assert!(!has_cache_control(last_block));
    }

    #[test]
    fn claude_prompt_caching_marks_last_common_block_when_suffix_changes() {
        let mut payload1 = json!({
            "model": "claude-3-5-sonnet-latest",
            "system": [{ "type": "text", "text": "sys" }],
            "messages": [
                { "role": "user", "content": [{ "type": "text", "text": "prehistory" }] },
                { "role": "assistant", "content": [{ "type": "text", "text": "a1" }] },
                { "role": "user", "content": [{ "type": "text", "text": "u1" }] }
            ]
        });
        let snapshot = apply_claude_prompt_caching(&mut payload1, None, "5m");

        let mut payload2 = json!({
            "model": "claude-3-5-sonnet-latest",
            "system": [{ "type": "text", "text": "sys" }],
            "messages": [
                { "role": "user", "content": [{ "type": "text", "text": "prehistory" }] },
                { "role": "assistant", "content": [{ "type": "text", "text": "a1" }] },
                { "role": "user", "content": [{ "type": "text", "text": "u1 changed" }] }
            ]
        });

        let _snapshot2 = apply_claude_prompt_caching(&mut payload2, Some(&snapshot), "5m");

        let messages = payload2
            .as_object()
            .and_then(|root| root.get("messages"))
            .and_then(Value::as_array)
            .expect("messages must be array");

        let last_common_block = messages
            .get(1)
            .and_then(Value::as_object)
            .and_then(|message| message.get("content"))
            .and_then(Value::as_array)
            .and_then(|blocks| blocks.last())
            .expect("assistant block must exist");
        assert!(has_cache_control(last_common_block));

        let last_block = messages
            .last()
            .and_then(Value::as_object)
            .and_then(|message| message.get("content"))
            .and_then(Value::as_array)
            .and_then(|blocks| blocks.last())
            .expect("last message block must exist");
        assert!(!has_cache_control(last_block));
    }

    #[test]
    fn openrouter_prompt_caching_inserts_expected_breakpoints() {
        let mut payload = json!({
            "model": "anthropic/claude-3.5-sonnet",
            "messages": [
                { "role": "system", "content": "sys" },
                { "role": "user", "content": "prehistory" },
                { "role": "assistant", "content": "a1" },
                { "role": "user", "content": "u1" }
            ]
        });

        let _snapshot = apply_openrouter_claude_prompt_caching(&mut payload, None, "5m");

        let root = payload.as_object().expect("payload must be object");
        assert!(
            root.get("cache_control")
                .and_then(Value::as_object)
                .is_some_and(|cache_control| {
                    cache_control.get("type").and_then(Value::as_str) == Some("ephemeral")
                        && cache_control.get("ttl").and_then(Value::as_str) == Some("5m")
                })
        );

        let messages = root
            .get("messages")
            .and_then(Value::as_array)
            .expect("messages must be array");

        for index in [0_usize, 1_usize] {
            let block = messages
                .get(index)
                .and_then(Value::as_object)
                .and_then(|message| message.get("content"))
                .and_then(Value::as_array)
                .and_then(|parts| parts.last())
                .expect("cached block must exist");
            assert!(has_cache_control(block));
        }

        let last_block = messages
            .last()
            .and_then(Value::as_object)
            .and_then(|message| message.get("content"))
            .and_then(Value::as_array)
            .and_then(|parts| parts.last())
            .expect("last message block must exist");
        assert!(!has_cache_control(last_block));
    }

    #[test]
    fn openrouter_prompt_caching_marks_last_common_block_when_suffix_changes() {
        let mut payload1 = json!({
            "model": "anthropic/claude-3.5-sonnet",
            "messages": [
                { "role": "system", "content": "sys" },
                { "role": "user", "content": "prehistory" },
                { "role": "assistant", "content": "a1" },
                { "role": "user", "content": "u1" }
            ]
        });
        let snapshot = apply_openrouter_claude_prompt_caching(&mut payload1, None, "5m");

        let mut payload2 = json!({
            "model": "anthropic/claude-3.5-sonnet",
            "messages": [
                { "role": "system", "content": "sys" },
                { "role": "user", "content": "prehistory" },
                { "role": "assistant", "content": "a1" },
                { "role": "user", "content": "u1 changed" }
            ]
        });

        let _snapshot2 = apply_openrouter_claude_prompt_caching(&mut payload2, Some(&snapshot), "5m");

        let messages = payload2
            .as_object()
            .and_then(|root| root.get("messages"))
            .and_then(Value::as_array)
            .expect("messages must be array");

        let last_common_block = messages
            .get(2)
            .and_then(Value::as_object)
            .and_then(|message| message.get("content"))
            .and_then(Value::as_array)
            .and_then(|parts| parts.last())
            .expect("assistant block must exist");
        assert!(has_cache_control(last_common_block));
    }
}
