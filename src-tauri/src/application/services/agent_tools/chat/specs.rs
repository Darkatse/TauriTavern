use serde_json::json;

use super::{CHAT_READ_MESSAGES, CHAT_SEARCH};
use crate::domain::models::agent::AgentToolSpec;

const MODEL_CHAT_READ_MESSAGES: &str = "chat_read_messages";
const MODEL_CHAT_SEARCH: &str = "chat_search";

pub(in crate::application::services::agent_tools) fn chat_read_messages_spec() -> AgentToolSpec {
    AgentToolSpec {
        name: CHAT_READ_MESSAGES.to_string(),
        model_name: MODEL_CHAT_READ_MESSAGES.to_string(),
        title: "Chat Read Messages".to_string(),
        description: "Read selected messages from the current chat by 0-based message index. Use chat_search first when you do not know the message index. For long messages, set start_char and max_chars on that message.".to_string(),
        input_schema: json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "messages": {
                    "type": "array",
                    "description": "Messages to read. Each item needs an absolute 0-based message index; optional start_char and max_chars read a slice of that message.",
                    "items": {
                        "type": "object",
                        "additionalProperties": false,
                        "properties": {
                            "index": {
                                "type": "integer",
                                "description": "0-based message index in the current chat."
                            },
                            "start_char": {
                                "type": "integer",
                                "description": "Optional 0-based character offset inside the message text."
                            },
                            "max_chars": {
                                "type": "integer",
                                "description": "Optional maximum characters to read from this message."
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
        source: "builtin".to_string(),
    }
}

pub(in crate::application::services::agent_tools) fn chat_search_spec() -> AgentToolSpec {
    AgentToolSpec {
        name: CHAT_SEARCH.to_string(),
        model_name: MODEL_CHAT_SEARCH.to_string(),
        title: "Chat Search".to_string(),
        description: "Search messages in the current chat. Only query is required. Results return message indexes and snippets; call chat_read_messages to read exact messages or ranges.".to_string(),
        input_schema: json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Text to search for in the current chat."
                },
                "limit": {
                    "type": "integer",
                    "description": "Optional maximum hits to return. Defaults to 20; maximum is 50."
                },
                "role": {
                    "type": "string",
                    "enum": ["user", "assistant", "system"],
                    "description": "Optional role filter."
                },
                "start_message": {
                    "type": "integer",
                    "description": "Optional first 0-based message index to search."
                },
                "end_message": {
                    "type": "integer",
                    "description": "Optional last 0-based message index to search."
                },
                "scan_limit": {
                    "type": "integer",
                    "description": "Optional maximum number of recent messages to scan."
                }
            },
            "required": ["query"]
        }),
        output_schema: None,
        annotations: json!({ "readOnly": true, "sourceKind": "chat" }),
        source: "builtin".to_string(),
    }
}
