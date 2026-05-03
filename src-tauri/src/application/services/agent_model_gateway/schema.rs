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
    schema
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
