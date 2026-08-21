use serde_json::json;

use super::{SKILL_LIST, SKILL_READ, SKILL_SCRIPT, SKILL_SEARCH};
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
        description: Some("Read a UTF-8 file from an installed Agent Skill. Start with SKILL.md. Omit start_line and line_count to read the full file; oversized files return a bounded preview with the next line to read. Use skill_search to locate relevant text in large supporting files.".to_string()),
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
                "start_line": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Optional 1-based starting line. Omit to start at line 1."
                },
                "line_count": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Optional number of lines to read. Omit to read through the end; oversized results return a shorter preview."
                }
            },
            "required": ["name"]
        }),
        output_schema: None,
        annotations: json!({ "readOnly": true, "sourceKind": "skill" }),
    }
}

pub(in crate::services::agent_tools) fn skill_script_descriptor() -> ToolDescriptor {
    ToolDescriptor {
        id: ToolId::builtin(SKILL_SCRIPT).expect("builtin tool name must be valid"),
        title: Some("Run Skill Script".to_string()),
        description: Some("Run a JavaScript script shipped with an installed Agent Skill in a sandboxed engine. Each script's arguments and return value are documented in the skill's SKILL.md — read it before calling. Scripts can only read and write this run's workspace and cannot access the network.".to_string()),
        input_schema: json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "skill": {
                    "type": "string",
                    "description": "Visible installed Skill name from skill_list that ships this script."
                },
                "script": {
                    "type": "string",
                    "description": "Script file name under the skill's scripts/ directory, without the .js extension."
                },
                "args": {
                    "type": "object",
                    "description": "Arguments object passed to the script's default/main export.",
                    "additionalProperties": true
                }
            },
            "required": ["skill", "script"]
        }),
        output_schema: None,
        annotations: json!({ "readOnly": false, "sourceKind": "skill" }),
    }
}
