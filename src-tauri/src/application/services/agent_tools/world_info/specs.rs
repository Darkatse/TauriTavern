use serde_json::json;

use super::WORLDINFO_READ_ACTIVATED;
use crate::domain::models::agent::AgentToolSpec;

const MODEL_WORLDINFO_READ_ACTIVATED: &str = "worldinfo_read_activated";

pub(in crate::application::services::agent_tools) fn worldinfo_read_activated_spec() -> AgentToolSpec
{
    AgentToolSpec {
        name: WORLDINFO_READ_ACTIVATED.to_string(),
        model_name: MODEL_WORLDINFO_READ_ACTIVATED.to_string(),
        title: "World Info Read Activated".to_string(),
        description: "Read the World Info entries activated for this Agent run. Returns final activated lore entries, not scan internals.".to_string(),
        input_schema: json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "max_chars": {
                    "type": "integer",
                    "description": "Optional maximum total entry content characters to return. Defaults to 20000; maximum is 50000."
                }
            }
        }),
        output_schema: None,
        annotations: json!({ "readOnly": true, "sourceKind": "worldInfo" }),
        source: "builtin".to_string(),
    }
}
