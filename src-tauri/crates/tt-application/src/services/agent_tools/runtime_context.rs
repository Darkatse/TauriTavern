use serde_json::{Map, Value, json};

use crate::errors::ApplicationError;

/// 把本次 run 的宿主事实投影为引擎无关的 JSON context。
pub(crate) fn build_script_context_json(
    prompt_snapshot: &Value,
) -> Result<Value, ApplicationError> {
    let entries = prompt_snapshot
        .get("worldInfoActivation")
        .and_then(|batch| batch.get("entries"))
        .and_then(Value::as_array)
        .ok_or_else(|| invalid_script_context("worldInfoActivation.entries must be an array"))?;
    let world_info_entries = entries
        .iter()
        .enumerate()
        .map(|(index, entry)| super::world_info::normalize_entry_json(index, entry))
        .collect::<Result<Vec<_>, ApplicationError>>()?;

    let frozen = prompt_snapshot
        .get("frozenRunInputSnapshot")
        .map(|value| {
            value
                .as_object()
                .ok_or_else(|| invalid_script_context("frozenRunInputSnapshot must be an object"))
        })
        .transpose()?;
    let variables = frozen
        .and_then(|frozen| frozen.get("variables"))
        .map(|variables| {
            let variables = variables
                .as_object()
                .ok_or_else(|| invalid_script_context("variables must be an object"))?;
            let local = variables
                .get("local")
                .and_then(Value::as_object)
                .ok_or_else(|| invalid_script_context("variables.local must be an object"))?;
            let global = variables
                .get("global")
                .and_then(Value::as_object)
                .ok_or_else(|| invalid_script_context("variables.global must be an object"))?;
            Ok::<Value, ApplicationError>(json!({ "local": local, "global": global }))
        })
        .transpose()?
        .unwrap_or_else(|| json!({ "local": {}, "global": {} }));

    let empty_macro_context = Map::new();
    let macro_context = frozen
        .and_then(|frozen| frozen.get("macroContext"))
        .map(|value| {
            value
                .as_object()
                .ok_or_else(|| invalid_script_context("macroContext must be an object"))
        })
        .transpose()?
        .unwrap_or(&empty_macro_context);

    Ok(json!({
        "worldInfo": { "entries": world_info_entries },
        "variables": variables,
        "macro": macro_context,
    }))
}

fn invalid_script_context(message: &str) -> ApplicationError {
    ApplicationError::ValidationError(format!("agent.invalid_script_context: {message}"))
}
