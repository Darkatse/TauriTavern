use serde_json::json;

use super::{SKILL_LIST, SKILL_READ};
use crate::domain::models::agent::AgentToolSpec;

const MODEL_SKILL_LIST: &str = "skill_list";
const MODEL_SKILL_READ: &str = "skill_read";

pub(in crate::application::services::agent_tools) fn skill_list_spec() -> AgentToolSpec {
    AgentToolSpec {
        name: SKILL_LIST.to_string(),
        model_name: MODEL_SKILL_LIST.to_string(),
        title: "Skill List".to_string(),
        description: "List installed Agent Skills by name and description. Use this before skill_read when reusable writing, editing, planning, or character guidance may help.".to_string(),
        input_schema: json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {}
        }),
        output_schema: None,
        annotations: json!({ "readOnly": true, "sourceKind": "skill" }),
        source: "builtin".to_string(),
    }
}

pub(in crate::application::services::agent_tools) fn skill_read_spec() -> AgentToolSpec {
    AgentToolSpec {
        name: SKILL_READ.to_string(),
        model_name: MODEL_SKILL_READ.to_string(),
        title: "Skill Read".to_string(),
        description: "Read a UTF-8 file from an installed Agent Skill. Start with SKILL.md, then follow referenced paths such as references/*.md or examples/*.md as needed.".to_string(),
        input_schema: json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Installed Skill name from skill_list."
                },
                "path": {
                    "type": "string",
                    "description": "Skill package relative file path. Defaults to SKILL.md."
                },
                "max_chars": {
                    "type": "integer",
                    "description": "Maximum characters to read. Defaults to 20000; maximum is 80000."
                }
            },
            "required": ["name"]
        }),
        output_schema: None,
        annotations: json!({ "readOnly": true, "sourceKind": "skill" }),
        source: "builtin".to_string(),
    }
}
