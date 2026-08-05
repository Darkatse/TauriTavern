use serde_json::json;

use super::DICE_ROLL;
use tt_domain::models::tool::{ToolDescriptor, ToolId};

pub(in crate::services::agent_tools) fn dice_roll_descriptor() -> ToolDescriptor {
    ToolDescriptor {
        id: ToolId::builtin(DICE_ROLL).expect("builtin tool name must be valid"),
        title: Some("Dice Roll".to_string()),
        description: Some("Roll dice when the task explicitly needs randomization, such as tabletop, roleplay, or chance checks. Supports droll-style formulas such as d6, 1d20, 3d6+4, or 2d10-1; a plain number such as 20 means 1d20.".to_string()),
        input_schema: json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "formula": {
                    "type": "string",
                    "description": "Dice formula to roll, e.g. d6, 1d20, 3d6+4, 2d10-1, or plain 20 for 1d20."
                }
            },
            "required": ["formula"]
        }),
        output_schema: None,
        annotations: json!({ "readOnly": true, "idempotent": false, "sourceKind": "random" }),
    }
}
