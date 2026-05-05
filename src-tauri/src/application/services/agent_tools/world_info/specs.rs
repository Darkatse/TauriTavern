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
        description: "Inspect World Info entries activated for this Agent run. Omit arguments to list active refs without content; pass entries with refs and optional character ranges to read selected lore text. Reads the run prompt snapshot, not global World Info.".to_string(),
        input_schema: json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "entries": {
                    "type": "array",
                    "description": "Optional entries to read. Omit this parameter to list active World Info refs without content.",
                    "items": {
                        "type": "object",
                        "additionalProperties": false,
                        "properties": {
                            "ref": {
                                "type": "string",
                                "description": "Active World Info ref returned by the no-argument index call."
                            },
                            "start_char": {
                                "type": "integer",
                                "description": "Optional 0-based character offset inside the entry content."
                            },
                            "max_chars": {
                                "type": "integer",
                                "description": "Optional maximum characters to read from this entry. Maximum is 8000."
                            }
                        },
                        "required": ["ref"]
                    },
                    "minItems": 1
                }
            }
        }),
        output_schema: None,
        annotations: json!({ "readOnly": true, "sourceKind": "worldInfo" }),
        source: "builtin".to_string(),
    }
}
