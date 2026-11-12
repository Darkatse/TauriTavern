use serde_json::{Map, Value};

use crate::errors::ApplicationError;
use tt_domain::models::agent::AgentModelRole;

const FIELDS: &str = "fields";

pub(super) fn message_structure_keys(role: AgentModelRole) -> &'static [&'static str] {
    match role {
        AgentModelRole::Assistant => &[
            "role",
            "content",
            "reasoning_content",
            "native",
            "tool_calls",
        ],
        AgentModelRole::Tool => &[
            "role",
            "content",
            "reasoning_content",
            "native",
            "tool_call_id",
            "name",
        ],
        AgentModelRole::System | AgentModelRole::Developer | AgentModelRole::User => {
            &["role", "content", "reasoning_content", "native"]
        }
    }
}

pub(super) const TOOL_CALL_STRUCTURE_KEYS: &[&str] = &["id", "type", "function"];

pub(super) const TOOL_FUNCTION_STRUCTURE_KEYS: &[&str] = &["name", "description", "parameters"];

pub(super) fn split_fields(
    mut object: Map<String, Value>,
    structure_keys: &[&str],
) -> (Map<String, Value>, Map<String, Value>) {
    let structure = structure_keys
        .iter()
        .filter_map(|key| object.remove(*key).map(|value| (key.to_string(), value)))
        .collect();
    (structure, object)
}

pub(super) fn metadata_with_fields(
    mut metadata: Map<String, Value>,
    fields: Map<String, Value>,
) -> Value {
    if !fields.is_empty() {
        metadata.insert(FIELDS.to_string(), Value::Object(fields));
    }
    Value::Object(metadata)
}

pub(super) fn merge_fields(
    object: &mut Map<String, Value>,
    metadata: &Value,
    structure_keys: &[&str],
) -> Result<(), ApplicationError> {
    let Some(fields) = metadata.get(FIELDS) else {
        return Ok(());
    };
    let fields = fields.as_object().ok_or_else(|| {
        ApplicationError::ValidationError(
            "agent.invalid_model_message: providerMetadata.fields must be an object".to_string(),
        )
    })?;
    extend_with_fields(object, fields, structure_keys)
}

pub(super) fn extend_with_fields(
    object: &mut Map<String, Value>,
    fields: &Map<String, Value>,
    structure_keys: &[&str],
) -> Result<(), ApplicationError> {
    if let Some(key) = fields
        .keys()
        .find(|key| structure_keys.contains(&key.as_str()))
    {
        return Err(ApplicationError::ValidationError(format!(
            "agent.invalid_fields: fields cannot contain `{key}`, which the structure builds"
        )));
    }
    object.extend(fields.clone());
    Ok(())
}
