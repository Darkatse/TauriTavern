use serde_json::json;

use super::{SKILL_LIST, SKILL_READ, SKILL_SEARCH};
use tt_domain::models::tool::{ToolDescriptor, ToolId};

pub(in crate::services::agent_tools) fn skill_list_descriptor() -> ToolDescriptor {
    ToolDescriptor {
        id: ToolId::builtin(SKILL_LIST).expect("builtin tool name must be valid"),
        title: Some("Skill List".to_string()),
        description: Some("List installed Agent Skills by name and description. Use this before skill_search or skill_read when reusable writing, editing, planning, or character guidance may help.".to_string()),
        input_schema: json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {}
        }),
        output_schema: None,
        annotations: json!({ "readOnly": true, "sourceKind": "skill" }),
    }
}

pub(in crate::services::agent_tools) fn skill_search_descriptor() -> ToolDescriptor {
    ToolDescriptor {
        id: ToolId::builtin(SKILL_SEARCH).expect("builtin tool name must be valid"),
        title: Some("Skill Search".to_string()),
        description: Some("Search UTF-8 text files inside one visible installed Agent Skill. Results return snippets and refs; call skill_read with path and a range to read exact text.".to_string()),
        input_schema: json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Visible installed Skill name from skill_list."
                },
                "query": {
                    "type": "string",
                    "description": "Plain text to search for inside this Skill."
                },
                "path": {
                    "type": "string",
                    "description": "Optional Skill package relative file or directory path. Omit to search all text files in the Skill."
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum hits to return. Defaults to 20; maximum is 50."
                },
                "context_lines": {
                    "type": "integer",
                    "description": "Context lines before and after each match. Defaults to 2; maximum is 5."
                }
            },
            "required": ["name", "query"]
        }),
        output_schema: None,
        annotations: json!({ "readOnly": true, "sourceKind": "skill" }),
    }
}

pub(in crate::services::agent_tools) fn skill_read_descriptor() -> ToolDescriptor {
    ToolDescriptor {
        id: ToolId::builtin(SKILL_READ).expect("builtin tool name must be valid"),
        title: Some("Skill Read".to_string()),
        description: Some("Read a UTF-8 file or range from an installed Agent Skill. Start with SKILL.md, use skill_search for large files, then read exact referenced ranges as needed.".to_string()),
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
                    "description": "Maximum characters to return in this call. Current policy controls the exact per-call and per-run Skill read budgets."
                },
                "start_line": {
                    "type": "integer",
                    "description": "Optional 1-based starting line. Do not combine with start_char."
                },
                "line_count": {
                    "type": "integer",
                    "description": "Optional number of lines to read. Do not combine with start_char."
                },
                "start_char": {
                    "type": "integer",
                    "description": "Optional 0-based character offset for character-range reads. Do not combine with start_line or line_count."
                }
            },
            "required": ["name"]
        }),
        output_schema: None,
        annotations: json!({ "readOnly": true, "sourceKind": "skill" }),
    }
}
