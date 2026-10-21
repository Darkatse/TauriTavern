use std::collections::{HashMap, HashSet};
use std::ops::Range;

use serde_json::Value;
use tt_domain::models::agent::session::AgentSessionMessage;
use tt_domain::models::agent::{AgentModelContentPart, AgentModelMessage, AgentModelRole};

use crate::dto::agent_dto::AgentPromptAssemblyBrokerRequestDto;
use crate::errors::ApplicationError;

#[derive(Debug)]
pub(super) struct SessionInvocation {
    pub assembly_request: AgentPromptAssemblyBrokerRequestDto,
}

pub(super) struct SessionGroup {
    pub range: Range<usize>,
    pub interrupted: bool,
}

pub(super) fn session_context(history: &[AgentSessionMessage]) -> Vec<&AgentSessionMessage> {
    history.iter().collect()
}

pub(super) fn session_groups(
    entries: &[&AgentSessionMessage],
) -> Result<Vec<SessionGroup>, ApplicationError> {
    let mut groups = Vec::new();
    let mut index = 0;
    while index < entries.len() {
        let start = index;
        let message = &entries[index].message;
        if message.role == AgentModelRole::Tool {
            return Err(ApplicationError::ValidationError(
                "agent.tool_history_orphan: tool result has no preceding call".into(),
            ));
        }
        let mut pending = HashSet::new();
        for part in &message.parts {
            if let AgentModelContentPart::ToolCall { call } = part
                && !pending.insert(call.call_id.as_str())
            {
                return Err(ApplicationError::ValidationError(
                    "agent.tool_history_duplicate: repeated call id".into(),
                ));
            }
        }
        index += 1;
        while index < entries.len() && entries[index].message.role == AgentModelRole::Tool {
            let mut results = entries[index]
                .message
                .parts
                .iter()
                .filter_map(|part| match part {
                    AgentModelContentPart::ToolResult { result } => Some(result),
                    _ => None,
                });
            let result = results.next();
            if results.next().is_some()
                || result.is_none_or(|result| !pending.remove(result.call_id.as_str()))
            {
                return Err(ApplicationError::ValidationError(
                    "agent.tool_history_orphan: tool result does not match an unanswered call"
                        .into(),
                ));
            }
            index += 1;
        }
        groups.push(SessionGroup {
            range: start..index,
            interrupted: !pending.is_empty(),
        });
    }
    Ok(groups)
}

pub(super) fn session_history_groups(
    entries: &[&AgentSessionMessage],
) -> Result<Vec<Vec<AgentModelMessage>>, ApplicationError> {
    session_groups(entries)?.into_iter().map(|group| {
        let entries = &entries[group.range];
        if !group.interrupted {
            return Ok(entries.iter().map(|entry| entry.message.clone()).collect());
        }
        let results = entries.iter().flat_map(|entry| &entry.message.parts)
            .filter_map(|part| match part {
                AgentModelContentPart::ToolResult { result } => Some((result.call_id.as_str(), result)),
                _ => None,
            }).collect::<HashMap<_, _>>();
        let mut lines = vec!["The previous turn stopped. Actions without recorded results have an unknown outcome; check the current state before repeating them.".to_string()];
        for entry in entries {
            for part in &entry.message.parts {
                match part {
                    AgentModelContentPart::Text { text } => lines.push(text.clone()),
                    AgentModelContentPart::ToolCall { call } => {
                        let alias = call.provider_metadata.get("modelAlias").and_then(Value::as_str)
                            .ok_or_else(|| ApplicationError::ValidationError(
                                "agent.tool_history_alias_missing: historical call has no modelAlias".into()
                            ))?;
                        let outcome = match results.get(call.call_id.as_str()) {
                            Some(result) => format!("Recorded {}: {}", if result.is_error { "error" } else { "result" }, result.content),
                            None => "Outcome unknown: no recorded result.".into(),
                        };
                        let arguments = serde_json::to_string(&call.arguments)
                            .expect("tool arguments contain JSON values or a string");
                        lines.push(format!("{alias}({arguments})\n{outcome}"));
                    }
                    _ => {}
                }
            }
        }
        Ok(vec![AgentModelMessage {
            role: AgentModelRole::Assistant,
            parts: vec![AgentModelContentPart::Text { text: lines.join("\n\n") }],
            provider_metadata: Value::Null,
        }])
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use serde_json::json;

    #[test]
    fn session_history_pairs_out_of_order_results_in_an_interrupted_group() {
        let calls = ["saved", "pending", "missing"].map(|name| {
            json!({
                "type": "toolCall",
                "call": {
                    "callId": name,
                    "toolId": "builtin:workspace.read_file",
                    "arguments": { "path": format!("work/{name}.md") },
                    "providerMetadata": { "modelAlias": "workspace_read_file" }
                }
            })
        });
        let result = |id: &str, content: &str, is_error: bool| {
            json!({
                "role": "tool",
                "parts": [{
                    "type": "toolResult",
                    "result": {
                        "callId": id,
                        "toolId": "builtin:workspace.read_file",
                        "content": content,
                        "isError": is_error,
                        "structured": {},
                        "resourceRefs": []
                    }
                }]
            })
        };
        let history = [
            json!({ "role": "assistant", "parts": calls }),
            result("missing", "File does not exist.", true),
            result("saved", "The saved content.", false),
        ]
        .into_iter()
        .enumerate()
        .map(|(index, message)| AgentSessionMessage {
            seq: index as u64 + 1,
            run_id: "interrupted".into(),
            created_at: Utc::now(),
            message: serde_json::from_value(message).unwrap(),
            origin: None,
        })
        .collect::<Vec<_>>();

        let groups = session_history_groups(&session_context(&history)).unwrap();
        let [group] = groups.as_slice() else {
            panic!("expected one interrupted group")
        };
        let [message] = group.as_slice() else {
            panic!("expected a protocol-safe explanation")
        };
        assert_eq!(message.role, AgentModelRole::Assistant);
        let [AgentModelContentPart::Text { text }] = message.parts.as_slice() else {
            panic!("interrupted calls must become text instead of replayable tool calls")
        };
        let outcomes = text
            .split("workspace_read_file(")
            .skip(1)
            .collect::<Vec<_>>();
        assert_eq!(outcomes.len(), 3);
        assert!(outcomes[0].contains("work/saved.md"));
        assert!(outcomes[0].contains("Recorded result: The saved content."));
        assert!(!outcomes[0].contains("File does not exist."));
        assert!(outcomes[1].contains("work/pending.md"));
        assert!(outcomes[1].contains("Outcome unknown"));
        assert!(!outcomes[1].contains("The saved content."));
        assert!(outcomes[2].contains("work/missing.md"));
        assert!(outcomes[2].contains("Recorded error: File does not exist."));
    }
}
