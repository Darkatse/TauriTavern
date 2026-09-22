use serde_json::{Map, Value, json};

use tt_domain::errors::DomainError;

/// Normalize the existing flat Chat record at disk/checkpoint read boundaries.
/// New records always use the tagged target; an unknown target stays an error.
pub fn canonicalize_agent_run_record(value: &mut Value) -> Result<(), DomainError> {
    let object = value
        .as_object_mut()
        .ok_or_else(|| DomainError::InvalidData("Agent run record must be a JSON object".into()))?;
    const CHAT_FIELDS: &[&str] = &[
        "stableChatId",
        "chatRef",
        "generationType",
        "skillScopeRefs",
        "persistBaseStateId",
        "inputMessageCount",
        "presentation",
    ];
    if object.contains_key("target") {
        if CHAT_FIELDS.iter().any(|field| object.contains_key(*field)) {
            return Err(DomainError::InvalidData(
                "Agent run record mixes target and legacy Chat fields".into(),
            ));
        }
    } else {
        for field in ["stableChatId", "chatRef", "generationType"] {
            if !object.contains_key(field) {
                return Err(DomainError::InvalidData(format!(
                    "Legacy agent run record is missing {field}"
                )));
            }
        }
        let mut target = Map::from_iter([("kind".into(), json!("chat"))]);
        for field in CHAT_FIELDS {
            if let Some(value) = object.remove(*field) {
                target.insert((*field).into(), value);
            }
        }
        target.entry("presentation").or_insert(json!("foreground"));
        object.insert("target".into(), Value::Object(target));
    }
    if object.get("status").and_then(Value::as_str) == Some("awaiting_commit") {
        object.insert("status".into(), json!("awaiting_host_commit"));
    }
    Ok(())
}
