use serde_json::json;

use super::{
    WORKSPACE_APPLY_PATCH, WORKSPACE_COMMIT, WORKSPACE_LIST_FILES, WORKSPACE_READ_FILE,
    WORKSPACE_SEARCH_FILES, WORKSPACE_SHELL, WORKSPACE_WRITE_FILE,
};
use tt_domain::models::tool::{ToolDescriptor, ToolId};

pub(in crate::services::agent_tools) fn workspace_list_files_descriptor() -> ToolDescriptor {
    ToolDescriptor {
        id: ToolId::builtin(WORKSPACE_LIST_FILES).expect("builtin tool name must be valid"),
        title: Some("Workspace List Files".to_string()),
        description: Some("List workspace files and directories.".to_string()),
        input_schema: json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Directory to list. Defaults to the workspace root."
                },
                "depth": {
                    "type": "integer",
                    "description": "Depth to list. Defaults to 2, at most 4."
                }
            }
        }),
        output_schema: None,
        annotations: json!({ "readOnly": true }),
    }
}

pub(in crate::services::agent_tools) fn workspace_read_file_descriptor() -> ToolDescriptor {
    ToolDescriptor {
        id: ToolId::builtin(WORKSPACE_READ_FILE).expect("builtin tool name must be valid"),
        title: Some("Workspace Read File".to_string()),
        description: Some("Read a workspace text file with line numbers. Long results return a preview and the next line to read.".to_string()),
        input_schema: json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "Workspace file path, e.g. output/main.md."
                },
                "offset": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "1-based first line. Defaults to 1."
                },
                "limit": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Lines to return. Defaults to the rest of the file."
                }
            },
            "required": ["file_path"]
        }),
        output_schema: None,
        annotations: json!({ "readOnly": true }),
    }
}

pub(in crate::services::agent_tools) fn workspace_search_files_descriptor() -> ToolDescriptor {
    ToolDescriptor {
        id: ToolId::builtin(WORKSPACE_SEARCH_FILES).expect("builtin tool name must be valid"),
        title: Some("Workspace Search Files".to_string()),
        description: Some("Search file contents with a regular expression (Rust regex syntax; prefix (?i) to ignore case). Returns matching lines with paths and line numbers.".to_string()),
        input_schema: json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Regular expression to match within a line."
                },
                "path": {
                    "type": "string",
                    "description": "File or directory to search. Defaults to readable directories other than tool-results/ and skills/, plus chat floor messages."
                }
            },
            "required": ["pattern"]
        }),
        output_schema: None,
        annotations: json!({ "readOnly": true }),
    }
}

pub(in crate::services::agent_tools) fn workspace_write_file_descriptor() -> ToolDescriptor {
    ToolDescriptor {
        id: ToolId::builtin(WORKSPACE_WRITE_FILE).expect("builtin tool name must be valid"),
        title: Some("Workspace Write File".to_string()),
        description: Some("Create a workspace text file or replace its full content; mode append adds text to the end.".to_string()),
        input_schema: json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "File path in a writable directory."
                },
                "content": {
                    "type": "string",
                    "description": "Full file content, or the text to append."
                },
                "mode": {
                    "type": "string",
                    "enum": ["replace", "append"],
                    "description": "Defaults to replace."
                }
            },
            "required": ["file_path", "content"]
        }),
        output_schema: None,
        annotations: json!({ "mutating": true }),
    }
}

pub(in crate::services::agent_tools) fn workspace_apply_patch_descriptor() -> ToolDescriptor {
    ToolDescriptor {
        id: ToolId::builtin(WORKSPACE_APPLY_PATCH).expect("builtin tool name must be valid"),
        title: Some("Workspace Apply Patch".to_string()),
        description: Some("Replace exact text in a workspace file. old_string must occur exactly once unless replace_all is true.".to_string()),
        input_schema: json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "File path in a writable directory."
                },
                "old_string": {
                    "type": "string",
                    "description": "Exact text to replace, without line-number prefixes."
                },
                "new_string": {
                    "type": "string",
                    "description": "Replacement text; empty deletes the match."
                },
                "replace_all": {
                    "type": "boolean",
                    "description": "Replace every match. Defaults to false."
                }
            },
            "required": ["file_path", "old_string", "new_string"]
        }),
        output_schema: None,
        annotations: json!({ "mutating": true }),
    }
}

pub(in crate::services::agent_tools) fn workspace_shell_descriptor() -> ToolDescriptor {
    ToolDescriptor {
        id: ToolId::builtin(WORKSPACE_SHELL).expect("builtin tool name must be valid"),
        title: Some("Workspace Shell".to_string()),
        description: Some("Run commands in the workspace: ls/find to list, mv/rm to move or delete, jq, a Python subset (python/python3) and js for batch processing. Run `js --help` for the workspace JS API. Each call starts a fresh session; files persist.".to_string()),
        input_schema: json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "command": {
                    "type": "string",
                    "description": "Commands to run."
                },
                "workdir": {
                    "type": "string",
                    "description": "Working directory. Defaults to /."
                }
            },
            "required": ["command"]
        }),
        output_schema: None,
        annotations: json!({ "mutating": true }),
    }
}

pub(in crate::services::agent_tools) fn workspace_commit_descriptor() -> ToolDescriptor {
    ToolDescriptor {
        id: ToolId::builtin(WORKSPACE_COMMIT).expect("builtin tool name must be valid"),
        title: Some("Workspace Commit".to_string()),
        description: Some("Publish a workspace file as this run's chat message. Only committed text reaches the chat; plain-text replies are never shown. Set finish: true on the final commit to end the run.".to_string()),
        input_schema: json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "File to publish. Defaults to output/main.md."
                },
                "reason": {
                    "type": "string",
                    "description": "One sentence on what this commit delivers and why."
                },
                "mode": {
                    "type": "string",
                    "enum": ["replace", "append"],
                    "description": "replace (default) rewrites this run's message; append adds to it."
                },
                "finish": {
                    "type": "boolean",
                    "description": "End the run after this commit succeeds. Must be the last call in its turn."
                }
            },
            "required": ["reason"]
        }),
        output_schema: None,
        annotations: json!({ "control": true, "mutating": true }),
    }
}
