use serde_json::json;

use super::WORLDINFO_READ_ACTIVATED;
use tt_domain::models::tool::{ToolDescriptor, ToolId};

pub(in crate::services::agent_tools) fn worldinfo_read_activated_descriptor() -> ToolDescriptor {
    ToolDescriptor {
        id: ToolId::builtin(WORLDINFO_READ_ACTIVATED)
            .expect("builtin tool name must be valid"),
        title: Some("World Info Read Activated".to_string()),
        description: Some("Inspect World Info entries activated for this Agent run. Omit arguments to list active refs without content; pass entries with refs and optional line ranges to read selected lore text. Entry content is read in full by default; oversized entries return a bounded preview with the next line to read. Reads the run prompt snapshot, not global World Info.".to_string()),
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
                            "start_line": {
                                "type": "integer",
                                "minimum": 1,
                                "description": "Optional 1-based starting line inside the entry content."
                            },
                            "line_count": {
                                "type": "integer",
                                "minimum": 1,
                                "description": "Optional number of lines to read. Omit to read through the end; oversized results return a shorter preview."
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
    }
}
