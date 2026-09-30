use serde_json::json;

use super::{CHAT_READ_MESSAGES, CHAT_SEARCH};
use tt_domain::models::tool::{ToolDescriptor, ToolId};

pub(in crate::services::agent_tools) fn chat_read_messages_descriptor() -> ToolDescriptor {
    ToolDescriptor {
        id: ToolId::builtin(CHAT_READ_MESSAGES).expect("builtin tool name must be valid"),
        title: Some("Chat Read Messages".to_string()),
        description: Some(
            "Read chat messages by 0-based index, optionally a line range of each.".to_string(),
        ),
        input_schema: json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "messages": {
                    "type": "array",
                    "description": "Messages to read.",
                    "items": {
                        "type": "object",
                        "additionalProperties": false,
                        "properties": {
                            "index": {
                                "type": "integer",
                                "description": "0-based message index."
                            },
                            "start_line": {
                                "type": "integer",
                                "minimum": 1,
                                "description": "1-based first line. Defaults to 1."
                            },
                            "line_count": {
                                "type": "integer",
                                "minimum": 1,
                                "description": "Lines to return. Defaults to the rest."
                            }
                        },
                        "required": ["index"]
                    },
                    "minItems": 1
                }
            },
            "required": ["messages"]
        }),
        output_schema: None,
        annotations: json!({ "readOnly": true, "sourceKind": "chat" }),
    }
}

pub(in crate::services::agent_tools) fn chat_search_descriptor() -> ToolDescriptor {
    ToolDescriptor {
        id: ToolId::builtin(CHAT_SEARCH).expect("builtin tool name must be valid"),
        title: Some("Chat Search".to_string()),
        description: Some(
            "Search this chat's messages; returns message indexes and snippets.".to_string(),
        ),
        input_schema: json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Text to find."
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum hits. Defaults to 20, at most 50."
                },
                "role": {
                    "type": "string",
                    "enum": ["user", "assistant", "system", "tool"],
                    "description": "Only messages from this role."
                },
                "start_message": {
                    "type": "integer",
                    "description": "First 0-based message index to search."
                },
                "end_message": {
                    "type": "integer",
                    "description": "Last 0-based message index to search."
                },
                "scan_limit": {
                    "type": "integer",
                    "description": "Only scan this many recent messages."
                }
            },
            "required": ["query"]
        }),
        output_schema: None,
        annotations: json!({ "readOnly": true, "sourceKind": "chat" }),
    }
}
