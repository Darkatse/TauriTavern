use serde_json::{Value, json};

use crate::application::services::agent_model_gateway::providers::AgentProviderAdapter;
use crate::domain::models::agent::AgentToolSpec;

pub(super) fn render_openai_tools(
    tools: &[AgentToolSpec],
    adapter: AgentProviderAdapter,
) -> Vec<Value> {
    tools
        .iter()
        .map(|spec| {
            json!({
                "type": "function",
                "function": {
                    "name": spec.model_name.as_str(),
                    "description": spec.description.as_str(),
                    "parameters": sanitize_schema_for_provider(&spec.input_schema, adapter),
                }
            })
        })
        .collect()
}

pub(super) fn sanitize_schema_for_provider(schema: &Value, adapter: AgentProviderAdapter) -> Value {
    let mut schema = schema.clone();
    remove_schema_keys(&mut schema, adapter.schema_keys_to_remove());
    if matches!(
        adapter,
        AgentProviderAdapter::Gemini | AgentProviderAdapter::GeminiInteractions
    ) {
        normalize_gemini_schema(&mut schema, 0);
    }
    schema
}

fn normalize_gemini_schema(value: &mut Value, depth: usize) {
    let Value::Object(object) = value else {
        return;
    };

    if let Some(properties) = object.get_mut("properties").and_then(Value::as_object_mut) {
        for nested in properties.values_mut() {
            normalize_gemini_schema(nested, depth + 1);
        }
    }
    if let Some(items) = object.get_mut("items") {
        normalize_gemini_schema(items, depth + 1);
    }

    if depth > 0 {
        // Gemini rejects nested required arrays in function declarations. Runtime
        // tool validators still return recoverable tool errors for missing fields.
        object.remove("required");
    } else {
        prune_required_to_declared_properties(object);
    }

    if depth > 0
        && object.get("type").and_then(Value::as_str) == Some("object")
        && !object
            .get("properties")
            .and_then(Value::as_object)
            .is_some_and(|properties| !properties.is_empty())
    {
        object.insert("type".to_string(), Value::String("string".to_string()));
        object.remove("properties");
        object.remove("items");
    }
}

fn prune_required_to_declared_properties(object: &mut serde_json::Map<String, Value>) {
    let Some(required) = object.get("required").and_then(Value::as_array) else {
        return;
    };
    let Some(properties) = object.get("properties").and_then(Value::as_object) else {
        object.remove("required");
        return;
    };

    let retained = required
        .iter()
        .filter_map(Value::as_str)
        .filter(|name| properties.contains_key(*name))
        .map(|name| Value::String(name.to_string()))
        .collect::<Vec<_>>();

    if retained.is_empty() {
        object.remove("required");
    } else {
        object.insert("required".to_string(), Value::Array(retained));
    }
}

fn remove_schema_keys(value: &mut Value, keys: &[&str]) {
    match value {
        Value::Object(object) => {
            for key in keys {
                object.remove(*key);
            }
            for nested in object.values_mut() {
                remove_schema_keys(nested, keys);
            }
        }
        Value::Array(items) => {
            for item in items {
                remove_schema_keys(item, keys);
            }
        }
        _ => {}
    }
}
