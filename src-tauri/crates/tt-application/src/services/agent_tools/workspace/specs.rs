use serde_json::json;

use super::{
    MODEL_WORKSPACE_ROOTS_FOR_MODEL, WORKSPACE_APPLY_PATCH, WORKSPACE_COMMIT, WORKSPACE_FINISH,
    WORKSPACE_LIST_FILES, WORKSPACE_READ_FILE, WORKSPACE_SEARCH_FILES, WORKSPACE_WRITE_FILE,
};
use tt_domain::models::agent::AgentToolSpec;

const MODEL_WORKSPACE_LIST_FILES: &str = "workspace_list_files";
const MODEL_WORKSPACE_SEARCH_FILES: &str = "workspace_search_files";
const MODEL_WORKSPACE_READ_FILE: &str = "workspace_read_file";
const MODEL_WORKSPACE_WRITE_FILE: &str = "workspace_write_file";
const MODEL_WORKSPACE_APPLY_PATCH: &str = "workspace_apply_patch";
const MODEL_WORKSPACE_COMMIT: &str = "workspace_commit";
const MODEL_WORKSPACE_FINISH: &str = "workspace_finish";

pub(in crate::services::agent_tools) fn workspace_list_files_spec() -> AgentToolSpec {
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

pub(in crate::services::agent_tools) fn workspace_read_file_spec() -> AgentToolSpec {
    AgentToolSpec {
        name: WORKSPACE_READ_FILE.to_string(),
        model_name: MODEL_WORKSPACE_READ_FILE.to_string(),
        title: "Workspace Read File".to_string(),
        description: "Read a visible UTF-8 Agent workspace file with line numbers. Read the exact text you want to replace before using workspace_apply_patch; if a patch fails, fully read the file before retrying. `path` MUST refer to a regular file (e.g. `persist/MEMORY.md`), NOT a directory or workspace root (`persist`, `output`, ...). Call workspace_list_files first when you do not know which file to open.".to_string(),
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
                },
                "start_char": {
                    "type": "integer",
                    "description": "Optional 0-based character offset for character-range reads. Do not combine with start_line or line_count."
                },
                "max_chars": {
                    "type": "integer",
                    "description": "Optional maximum characters for character-range reads. Maximum is 80000."
                }
            },
            "required": ["path"]
        }),
        output_schema: None,
        annotations: json!({ "readOnly": true }),
        source: "builtin".to_string(),
    }
}

pub(in crate::services::agent_tools) fn workspace_search_files_spec() -> AgentToolSpec {
    AgentToolSpec {
        name: WORKSPACE_SEARCH_FILES.to_string(),
        model_name: MODEL_WORKSPACE_SEARCH_FILES.to_string(),
        title: "Workspace Search Files".to_string(),
        description: format!(
            "Search visible UTF-8 Agent workspace files under {MODEL_WORKSPACE_ROOTS_FOR_MODEL}. Results return snippets and refs; use workspace_read_file to read exact ranges."
        ),
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
                    "description": format!("Optional visible workspace file or directory path under {MODEL_WORKSPACE_ROOTS_FOR_MODEL}. Omit to search all visible roots.")
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
        source: "builtin".to_string(),
    }
}

pub(in crate::services::agent_tools) fn workspace_write_file_spec() -> AgentToolSpec {
    AgentToolSpec {
        name: WORKSPACE_WRITE_FILE.to_string(),
        model_name: MODEL_WORKSPACE_WRITE_FILE.to_string(),
        title: "Workspace Write File".to_string(),
        description: "Write UTF-8 text to a writable Agent workspace file. mode replace writes the complete file. mode append adds content exactly to the end and creates the file when missing; include a leading newline in content when you want a new line.".to_string(),
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
                    "description": "Complete UTF-8 file content for replace, or the exact suffix to add for append."
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
        source: "builtin".to_string(),
    }
}

pub(in crate::services::agent_tools) fn workspace_apply_patch_spec() -> AgentToolSpec {
    AgentToolSpec {
        name: WORKSPACE_APPLY_PATCH.to_string(),
        model_name: MODEL_WORKSPACE_APPLY_PATCH.to_string(),
        title: "Workspace Apply Patch".to_string(),
        description: "Apply a precise single-file string replacement. old_string must come from text you already read with workspace_read_file or from a file you created/replaced in this run. old_string must match exactly and uniquely unless replace_all is true. If a patch fails, fully read the file before retrying.".to_string(),
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

pub(in crate::services::agent_tools) fn workspace_finish_spec() -> AgentToolSpec {
    AgentToolSpec {
        name: WORKSPACE_FINISH.to_string(),
        model_name: MODEL_WORKSPACE_FINISH.to_string(),
        title: "Workspace Finish".to_string(),
        description: "Finish the Agent run after required foreground chat commits and workspace changes are complete.".to_string(),
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
        source: "builtin".to_string(),
    }
}

pub(in crate::services::agent_tools) fn workspace_commit_spec() -> AgentToolSpec {
    AgentToolSpec {
        name: WORKSPACE_COMMIT.to_string(),
        model_name: MODEL_WORKSPACE_COMMIT.to_string(),
        title: "Workspace Commit".to_string(),
        description: "Commit a workspace text file to the current chat message. With no arguments, replace the current run message with output/main.md. append adds the file text to the same message, creating it when this run has not committed yet. You may keep editing and commit again as needed; after the final commit, call workspace_finish to close the run. Do not reply in plain text as the final answer.".to_string(),
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
        source: "builtin".to_string(),
    }
}
