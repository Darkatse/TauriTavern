use serde_json::json;

use super::{CHAT_READ_MESSAGES, CHAT_SEARCH};
use tt_domain::models::tool::{ToolDescriptor, ToolId};

pub(in crate::services::agent_tools) fn chat_read_messages_descriptor() -> ToolDescriptor {
    ToolDescriptor {
        id: ToolId::builtin(CHAT_READ_MESSAGES).expect("builtin tool name must be valid"),
        title: Some("Chat Read Messages".to_string()),
        description: Some(
            "Read several chat floors at once by floor number, the NNNNNN in floors/NNNNNN."
                .to_string(),
        ),
        input_schema: json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "floors": {
                    "type": "array",
                    "description": "Floors to read.",
                    "items": {
                        "type": "object",
                        "additionalProperties": false,
                        "properties": {
                            "floor": {
                                "type": "integer",
                                "description": "Floor number, from 0."
                            },
                            "offset": {
                                "type": "integer",
                                "minimum": 1,
                                "description": "1-based first line. Defaults to 1."
                            },
                            "limit": {
                                "type": "integer",
                                "minimum": 1,
                                "description": "Lines to return. Defaults to the rest of the floor."
                            }
                        },
                        "required": ["floor"]
                    },
                    "minItems": 1
                }
            },
            "required": ["floors"]
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
            "Search this chat's floors by words; any matching word counts and the best matches come first. Floors are the chat's messages, numbered from 0 as in floors/NNNNNN. Returns floor files (floors/NNNNNN/message.md) with snippets.".to_string(),
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
                    "enum": ["user", "assistant", "tool"],
                    "description": "Only floors with this role."
                },
                "hidden": {
                    "type": "boolean",
                    "description": "true: only hidden floors; false: only floors that are not hidden."
                },
                "start_floor": {
                    "type": "integer",
                    "description": "First floor index to search."
                },
                "end_floor": {
                    "type": "integer",
                    "description": "Last floor index to search."
                },
                "scan_limit": {
                    "type": "integer",
                    "description": "Only scan this many recent floors."
                }
            },
            "required": ["query"]
        }),
        output_schema: None,
        annotations: json!({ "readOnly": true, "sourceKind": "chat" }),
    }
}
