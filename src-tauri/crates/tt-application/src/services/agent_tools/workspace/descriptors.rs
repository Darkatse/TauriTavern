use serde_json::json;

use super::{
    WORKSPACE_APPLY_PATCH, WORKSPACE_COMMIT, WORKSPACE_FINISH, WORKSPACE_LIST_FILES,
    WORKSPACE_READ_FILE, WORKSPACE_SEARCH_FILES, WORKSPACE_SHELL, WORKSPACE_WRITE_FILE,
};
use tt_domain::models::tool::{ToolDescriptor, ToolId};

pub(in crate::services::agent_tools) fn workspace_list_files_descriptor() -> ToolDescriptor {
    ToolDescriptor {
        id: ToolId::builtin(WORKSPACE_LIST_FILES).expect("builtin tool name must be valid"),
        title: Some("Workspace List Files".to_string()),
        description: Some(
            "List workspace files and directories. Use this to find paths before reading files."
                .to_string(),
        ),
        input_schema: json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Optional workspace directory or file path. Omit to list from the workspace root."
                },
                "depth": {
                    "type": "integer",
                    "description": "Directory depth to list. Defaults to 2; maximum is 4."
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
        description: Some("Read a UTF-8 workspace file with line numbers. Use this to inspect text before editing. Large results include a preview and the next line to read.".to_string()),
        input_schema: json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Workspace file path, such as output/main.md."
                },
                "start_line": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "1-based starting line. Omit to start at line 1."
                },
                "line_count": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Number of lines to read. Omit to read through the end; oversized results return a shorter preview."
                }
            },
            "required": ["path"]
        }),
        output_schema: None,
        annotations: json!({ "readOnly": true }),
    }
}

pub(in crate::services::agent_tools) fn workspace_search_files_descriptor() -> ToolDescriptor {
    ToolDescriptor {
        id: ToolId::builtin(WORKSPACE_SEARCH_FILES).expect("builtin tool name must be valid"),
        title: Some("Workspace Search Files".to_string()),
        description: Some("Find text in workspace files. Results include snippets; use workspace_read_file for the full text or exact lines.".to_string()),
        input_schema: json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Plain text to search for in visible workspace files."
                },
                "path": {
                    "type": "string",
                    "description": "Optional workspace file or directory path. Omit to search all readable directories."
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
            "required": ["query"]
        }),
        output_schema: None,
        annotations: json!({ "readOnly": true }),
    }
}

pub(in crate::services::agent_tools) fn workspace_write_file_descriptor() -> ToolDescriptor {
    ToolDescriptor {
        id: ToolId::builtin(WORKSPACE_WRITE_FILE).expect("builtin tool name must be valid"),
        title: Some("Workspace Write File".to_string()),
        description: Some("Create a UTF-8 text file, append text, or replace its full contents. Use workspace_apply_patch for local edits. Before replacing an existing file, read it with workspace_read_file unless you created or replaced its current contents with the text tools.".to_string()),
        input_schema: json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Workspace file path in a writable directory."
                },
                "content": {
                    "type": "string",
                    "description": "Complete file content for replace, or the exact text to add for append. Include any needed newlines."
                },
                "mode": {
                    "type": "string",
                    "enum": ["replace", "append"],
                    "description": "replace writes the complete file; append adds content to the end, creating the file if missing. Defaults to replace."
                }
            },
            "required": ["path", "content"]
        }),
        output_schema: None,
        annotations: json!({ "mutating": true }),
    }
}

pub(in crate::services::agent_tools) fn workspace_apply_patch_descriptor() -> ToolDescriptor {
    ToolDescriptor {
        id: ToolId::builtin(WORKSPACE_APPLY_PATCH).expect("builtin tool name must be valid"),
        title: Some("Workspace Apply Patch".to_string()),
        description: Some("Replace exact text in one file. Read the text with workspace_read_file first, unless you created or replaced the file with the text tools. old_string must match exactly and be unique unless replace_all is true. After a failed patch, read the full file before retrying.".to_string()),
        input_schema: json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Workspace file path in a writable directory."
                },
                "old_string": {
                    "type": "string",
                    "description": "Exact text to replace. Do not include line number prefixes from read output."
                },
                "new_string": {
                    "type": "string",
                    "description": "Replacement text without line number prefixes."
                },
                "replace_all": {
                    "type": "boolean",
                    "description": "Replace every occurrence of old_string. Requires a full read or a file created/replaced with the text tools. Defaults to false."
                }
            },
            "required": ["path", "old_string", "new_string"]
        }),
        output_schema: None,
        annotations: json!({ "mutating": true }),
    }
}

pub(in crate::services::agent_tools) fn workspace_shell_descriptor() -> ToolDescriptor {
    ToolDescriptor {
        id: ToolId::builtin(WORKSPACE_SHELL).expect("builtin tool name must be valid"),
        title: Some("Workspace Shell".to_string()),
        description: Some("Run workspace commands for scripts, pipelines, data processing, batch changes, and copying, moving or deleting files. Prefer workspace_read_file, workspace_write_file and workspace_apply_patch for straightforward text work. Includes jq, a Python subset (python/python3), and JavaScript (js). Use js --help for JavaScript syntax and workspace APIs. Completed file changes persist after failure or cancellation.".to_string()),
        input_schema: json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "command": {
                    "type": "string",
                    "description": "Commands to execute in a new shell session."
                },
                "workdir": {
                    "type": "string",
                    "description": "Working directory in the workspace. Defaults to /."
                }
            },
            "required": ["command"]
        }),
        output_schema: None,
        annotations: json!({ "mutating": true }),
    }
}

pub(in crate::services::agent_tools) fn workspace_finish_descriptor() -> ToolDescriptor {
    ToolDescriptor {
        id: ToolId::builtin(WORKSPACE_FINISH).expect("builtin tool name must be valid"),
        title: Some("Workspace Finish".to_string()),
        description: Some("Finish the Agent run after required foreground chat commits and workspace changes are complete.".to_string()),
        input_schema: json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "reason": {
                    "type": "string",
                    "description": "Short completion reason."
                }
            }
        }),
        output_schema: None,
        annotations: json!({ "control": true }),
    }
}

pub(in crate::services::agent_tools) fn workspace_commit_descriptor() -> ToolDescriptor {
    ToolDescriptor {
        id: ToolId::builtin(WORKSPACE_COMMIT).expect("builtin tool name must be valid"),
        title: Some("Workspace Commit".to_string()),
        description: Some("Commit a workspace text file to the current chat message. With no arguments, replace the current run message with output/main.md. append adds the file text to the same message, creating it when this run has not committed yet. You may keep editing and commit again as needed; after the final commit, call workspace_finish to close the run. Do not reply in plain text as the final answer.".to_string()),
        input_schema: json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Relative visible workspace file path to publish. Defaults to output/main.md."
                },
                "mode": {
                    "type": "string",
                    "enum": ["replace", "append"],
                    "description": "replace overwrites this run's chat message; append appends to the same message. Defaults to replace."
                },
                "reason": {
                    "type": "string",
                    "description": "Short commit reason."
                }
            }
        }),
        output_schema: None,
        annotations: json!({ "control": true, "mutating": true }),
    }
}
