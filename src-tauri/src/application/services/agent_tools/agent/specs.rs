use serde_json::json;

use super::{AGENT_AWAIT, AGENT_DELEGATE, AGENT_HANDOFF, AGENT_LIST, TASK_RETURN};
use crate::domain::models::agent::AgentToolSpec;

const MODEL_AGENT_AWAIT: &str = "agent_await";
const MODEL_AGENT_DELEGATE: &str = "agent_delegate";
const MODEL_AGENT_HANDOFF: &str = "agent_handoff";
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
        description: "Ask another Agent to start a focused task. Include any workspace paths it should read or write in the task brief. You can continue other work after delegating; use agent_await when you need its result or status before deciding.".to_string(),
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
                    "description": "Clear task brief for the selected Agent. Mention relevant workspace paths when you expect it to inspect, edit, or create files.",
                    "additionalProperties": true,
                    "properties": {
                        "title": {
                            "type": "string",
                            "description": "Optional short task name for display. Omit it when the objective already makes the task clear."
                        },
                        "objective": {
                            "type": "string",
                            "description": "What you need this Agent to accomplish. Prefer the outcome over step-by-step instructions."
                        },
                        "context": {
                            "type": "object",
                            "description": "Relevant facts, constraints, draft text, style notes, or workspace paths such as output/section.md, plan/outline.md, or persist/story_state.md.",
                            "additionalProperties": true
                        },
                        "expectedOutput": {
                            "type": "object",
                            "description": "Preferred answer shape, including whether the Agent should only return a summary or also write artifacts and reference their paths in task_return.",
                            "additionalProperties": true
                        }
                    },
                    "required": ["objective"]
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
        description: "Wait for or inspect tasks you started with agent_delegate. Use nextCompleted when one finished result is enough, allCompleted when all selected tasks are needed, or statusOnly to check progress without waiting.".to_string(),
        input_schema: json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "taskIds": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional returned task handles. Omit to target all delegated tasks you started."
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

pub(in crate::application::services::agent_tools) fn agent_handoff_spec() -> AgentToolSpec {
    AgentToolSpec {
        name: AGENT_HANDOFF.to_string(),
        model_name: MODEL_AGENT_HANDOFF.to_string(),
        title: "Agent Handoff".to_string(),
        description: "Ask another Agent to take over the next stage of this run. Use this when you have done your part and the next Agent should continue from the shared workspace.".to_string(),
        input_schema: json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "agentId": {
                    "type": "string",
                    "description": "Agent id returned by agent_list with purpose handoff."
                },
                "handoff": {
                    "type": "object",
                    "description": "Brief the next Agent so it can continue without asking you. Include the objective, relevant workspace paths, context, constraints, and completion criteria.",
                    "additionalProperties": true,
                    "properties": {
                        "title": {
                            "type": "string",
                            "description": "Optional short handoff name for display."
                        },
                        "reason": {
                            "type": "string",
                            "description": "Why you are handing off now."
                        },
                        "objective": {
                            "type": "string",
                            "description": "What you want the next Agent to accomplish."
                        },
                        "contextSummary": {
                            "type": "string",
                            "description": "What you have done, what matters next, and any decisions or constraints the next Agent needs."
                        },
                        "workspaceRefs": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Workspace paths the next Agent should inspect or continue from."
                        },
                        "mustPreserve": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Facts, style constraints, plot points, or edits that must not be lost."
                        },
                        "completionCriteria": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "What done looks like for the next Agent."
                        }
                    },
                    "required": ["objective"]
                },
                "pendingTaskPolicy": {
                    "type": "string",
                    "enum": ["denyIfPending"],
                    "description": "Use denyIfPending so handoff waits until delegated tasks you started are finished."
                }
            },
            "required": ["agentId", "handoff"]
        }),
        output_schema: None,
        annotations: json!({ "readOnly": false, "sourceKind": "agent" }),
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
                            "path": {
                                "type": "string",
                                "description": "Workspace path for the artifact, such as an assigned output path or a supporting note path you created."
                            },
                            "kind": {
                                "type": "string",
                                "description": "Artifact format, such as markdown, json, or text."
                            },
                            "role": {
                                "type": "string",
                                "description": "How the requesting Agent should use this artifact, such as draft, outline, evidence, revision, or memory_update."
                            }
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
