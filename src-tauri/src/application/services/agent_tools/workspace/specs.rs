use serde_json::json;

use super::{
    MODEL_WORKSPACE_ROOTS_FOR_MODEL, WORKSPACE_APPLY_PATCH, WORKSPACE_FINISH, WORKSPACE_LIST_FILES,
    WORKSPACE_READ_FILE, WORKSPACE_WRITE_FILE,
};
use crate::domain::models::agent::AgentToolSpec;

const MODEL_WORKSPACE_LIST_FILES: &str = "workspace_list_files";
const MODEL_WORKSPACE_READ_FILE: &str = "workspace_read_file";
const MODEL_WORKSPACE_WRITE_FILE: &str = "workspace_write_file";
const MODEL_WORKSPACE_APPLY_PATCH: &str = "workspace_apply_patch";
const MODEL_WORKSPACE_FINISH: &str = "workspace_finish";

pub(in crate::application::services::agent_tools) fn workspace_list_files_spec() -> AgentToolSpec {
    AgentToolSpec {
        name: WORKSPACE_LIST_FILES.to_string(),
        model_name: MODEL_WORKSPACE_LIST_FILES.to_string(),
        title: "Workspace List Files".to_string(),
        description: format!(
            "List visible Agent workspace files under {MODEL_WORKSPACE_ROOTS_FOR_MODEL}. Use this before reading when you need to inspect available artifacts."
        ),
        input_schema: json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Optional relative workspace directory or file path. Omit to list the visible workspace roots."
                },
                "depth": {
                    "type": "integer",
                    "description": "Directory depth to list. Defaults to 2; maximum is 4."
                }
            }
        }),
        output_schema: None,
        annotations: json!({ "readOnly": true }),
        source: "builtin".to_string(),
    }
}

pub(in crate::application::services::agent_tools) fn workspace_read_file_spec() -> AgentToolSpec {
    AgentToolSpec {
        name: WORKSPACE_READ_FILE.to_string(),
        model_name: MODEL_WORKSPACE_READ_FILE.to_string(),
        title: "Workspace Read File".to_string(),
        description: "Read a visible UTF-8 Agent workspace file with line numbers. Fully read a file before using workspace_apply_patch on it; partial reads are only for inspection.".to_string(),
        input_schema: json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "path": {
                    "type": "string",
                    "description": format!("Relative workspace file path under {MODEL_WORKSPACE_ROOTS_FOR_MODEL}.")
                },
                "start_line": {
                    "type": "integer",
                    "description": "1-based starting line. Omit for a full read."
                },
                "line_count": {
                    "type": "integer",
                    "description": "Number of lines to read. Omit for a full read."
                }
            },
            "required": ["path"]
        }),
        output_schema: None,
        annotations: json!({ "readOnly": true }),
        source: "builtin".to_string(),
    }
}

pub(in crate::application::services::agent_tools) fn workspace_write_file_spec() -> AgentToolSpec {
    AgentToolSpec {
        name: WORKSPACE_WRITE_FILE.to_string(),
        model_name: MODEL_WORKSPACE_WRITE_FILE.to_string(),
        title: "Workspace Write File".to_string(),
        description: "Write complete UTF-8 text to a writable Agent workspace file. Use output/main.md for the final chat message body, then call workspace_finish.".to_string(),
        input_schema: json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "path": {
                    "type": "string",
                    "description": format!("Relative workspace path. Writable prefixes are {MODEL_WORKSPACE_ROOTS_FOR_MODEL}.")
                },
                "content": {
                    "type": "string",
                    "description": "Complete UTF-8 file content."
                }
            },
            "required": ["path", "content"]
        }),
        output_schema: None,
        annotations: json!({ "mutating": true }),
        source: "builtin".to_string(),
    }
}

pub(in crate::application::services::agent_tools) fn workspace_apply_patch_spec() -> AgentToolSpec {
    AgentToolSpec {
        name: WORKSPACE_APPLY_PATCH.to_string(),
        model_name: MODEL_WORKSPACE_APPLY_PATCH.to_string(),
        title: "Workspace Apply Patch".to_string(),
        description: "Apply a precise single-file string replacement. The file must have been fully read with workspace_read_file or created by workspace_write_file in this run. old_string must match exactly and uniquely unless replace_all is true.".to_string(),
        input_schema: json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "path": {
                    "type": "string",
                    "description": format!("Relative writable workspace file path under {MODEL_WORKSPACE_ROOTS_FOR_MODEL}.")
                },
                "old_string": {
                    "type": "string",
                    "description": "Exact text to replace. Do not include line number prefixes from read output."
                },
                "new_string": {
                    "type": "string",
                    "description": "Replacement text."
                },
                "replace_all": {
                    "type": "boolean",
                    "description": "Replace every occurrence of old_string. Defaults to false."
                }
            },
            "required": ["path", "old_string", "new_string"]
        }),
        output_schema: None,
        annotations: json!({ "mutating": true }),
        source: "builtin".to_string(),
    }
}

pub(in crate::application::services::agent_tools) fn workspace_finish_spec() -> AgentToolSpec {
    AgentToolSpec {
        name: WORKSPACE_FINISH.to_string(),
        model_name: MODEL_WORKSPACE_FINISH.to_string(),
        title: "Workspace Finish".to_string(),
        description: "Finish the Agent loop after the final artifact has been written. The default final_path is output/main.md.".to_string(),
        input_schema: json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "final_path": {
                    "type": "string",
                    "description": "Relative workspace path for the final artifact. Defaults to output/main.md."
                },
                "reason": {
                    "type": "string",
                    "description": "Short completion reason."
                }
            }
        }),
        output_schema: None,
        annotations: json!({ "control": true }),
        source: "builtin".to_string(),
    }
}
