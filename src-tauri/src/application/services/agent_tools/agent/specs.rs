use serde_json::json;

use super::{AGENT_AWAIT, AGENT_DELEGATE, AGENT_LIST, TASK_RETURN};
use crate::domain::models::agent::AgentToolSpec;

const MODEL_AGENT_AWAIT: &str = "agent_await";
const MODEL_AGENT_DELEGATE: &str = "agent_delegate";
const MODEL_AGENT_LIST: &str = "agent_list";
const MODEL_TASK_RETURN: &str = "task_return";

pub(in crate::application::services::agent_tools) fn agent_list_spec() -> AgentToolSpec {
    AgentToolSpec {
        name: AGENT_LIST.to_string(),
        model_name: MODEL_AGENT_LIST.to_string(),
        title: "Agent List".to_string(),
        description: "Find other Agents you can ask for focused help. This tool is read-only and does not start any work.".to_string(),
        input_schema: json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "purpose": {
                    "type": "string",
                    "enum": ["any", "delegate", "handoff"],
                    "description": "Optional kind of help to look for. Defaults to any."
                },
                "query": {
                    "type": "string",
                    "description": "Optional text filter over Agent id, display name, and description."
                },
                "limit": {
                    "type": "integer",
                    "description": "Optional maximum Agents to return. Defaults to 8; maximum is 20."
                }
            }
        }),
        output_schema: None,
        annotations: json!({ "readOnly": true, "sourceKind": "agent" }),
        source: "builtin".to_string(),
    }
}

pub(in crate::application::services::agent_tools) fn agent_delegate_spec() -> AgentToolSpec {
    AgentToolSpec {
        name: AGENT_DELEGATE.to_string(),
        model_name: MODEL_AGENT_DELEGATE.to_string(),
        title: "Agent Delegate".to_string(),
        description: "Ask another Agent to work on a self-contained task. The result is not returned immediately; use agent_await with the returned task id to collect it.".to_string(),
        input_schema: json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "agentId": {
                    "type": "string",
                    "description": "Agent id returned by agent_list."
                },
                "task": {
                    "type": "object",
                    "description": "Clear task brief for the selected Agent.",
                    "additionalProperties": true,
                    "properties": {
                        "title": {
                            "type": "string",
                            "description": "Short task name."
                        },
                        "objective": {
                            "type": "string",
                            "description": "What you need this Agent to accomplish."
                        },
                        "context": {
                            "type": "object",
                            "description": "Relevant facts, constraints, draft text, or style notes.",
                            "additionalProperties": true
                        },
                        "expectedOutput": {
                            "type": "object",
                            "description": "Preferred answer shape, such as bullets, critique notes, scene beats, or revision suggestions.",
                            "additionalProperties": true
                        }
                    },
                    "required": ["title", "objective"]
                },
                "budget": {
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                        "maxRounds": {
                            "type": "integer",
                            "description": "Optional round budget. Must be >= 1 and no greater than the target Agent allows."
                        },
                        "maxToolCalls": {
                            "type": "integer",
                            "description": "Optional tool-call budget. Must be >= 1 and no greater than the target Agent allows."
                        }
                    }
                }
            },
            "required": ["agentId", "task"]
        }),
        output_schema: None,
        annotations: json!({ "readOnly": false, "sourceKind": "agent" }),
        source: "builtin".to_string(),
    }
}

pub(in crate::application::services::agent_tools) fn agent_await_spec() -> AgentToolSpec {
    AgentToolSpec {
        name: AGENT_AWAIT.to_string(),
        model_name: MODEL_AGENT_AWAIT.to_string(),
        title: "Agent Await".to_string(),
        description: "Check or collect results from tasks you started with agent_delegate. Use nextCompleted for the first finished task, allCompleted for all selected tasks, or statusOnly to inspect without waiting.".to_string(),
        input_schema: json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "taskIds": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional task ids returned by agent_delegate. Omit to target all delegated tasks you started."
                },
                "mode": {
                    "type": "string",
                    "enum": ["nextCompleted", "allCompleted", "statusOnly"],
                    "description": "Await mode. Defaults to nextCompleted."
                },
                "timeoutMs": {
                    "type": "integer",
                    "description": "Optional wait timeout in milliseconds. Defaults to 120000; maximum is 300000."
                }
            }
        }),
        output_schema: None,
        annotations: json!({ "readOnly": true, "sourceKind": "agent" }),
        source: "builtin".to_string(),
    }
}

pub(in crate::application::services::agent_tools) fn task_return_spec() -> AgentToolSpec {
    AgentToolSpec {
        name: TASK_RETURN.to_string(),
        model_name: MODEL_TASK_RETURN.to_string(),
        title: "Task Return".to_string(),
        description: "Send your result for the delegated task and end your work on it.".to_string(),
        input_schema: json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "summary": {
                    "type": "string",
                    "description": "Concise result summary for the requesting Agent."
                },
                "status": {
                    "type": "string",
                    "enum": ["completed", "failed"],
                    "description": "Task outcome. Defaults to completed."
                },
                "confidence": {
                    "type": "string",
                    "enum": ["low", "medium", "high"],
                    "description": "Optional confidence level."
                },
                "artifacts": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "additionalProperties": false,
                        "properties": {
                            "path": { "type": "string" },
                            "kind": { "type": "string" },
                            "role": { "type": "string" }
                        },
                        "required": ["path", "kind", "role"]
                    }
                },
                "findings": {
                    "type": "array",
                    "items": { "type": "object", "additionalProperties": true }
                },
                "warnings": {
                    "type": "array",
                    "items": { "type": "string" }
                },
                "suggestedNextActions": {
                    "type": "array",
                    "items": { "type": "string" }
                },
                "questionsForCaller": {
                    "type": "array",
                    "items": { "type": "string" }
                }
            },
            "required": ["summary"]
        }),
        output_schema: None,
        annotations: json!({ "readOnly": false, "sourceKind": "agent" }),
        source: "builtin".to_string(),
    }
}
