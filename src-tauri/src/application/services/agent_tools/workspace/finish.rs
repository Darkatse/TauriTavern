use serde_json::json;

use super::args::{parse_workspace_path, required_trimmed_string_arg, tool_error};
use crate::application::errors::ApplicationError;
use crate::domain::models::agent::profile::ResolvedAgentProfile;
use crate::domain::models::agent::{AgentToolCall, AgentToolResult};

use super::super::dispatcher::AgentToolEffect;

pub(in crate::application::services::agent_tools) fn finish(
    call: &AgentToolCall,
    profile: &ResolvedAgentProfile,
) -> Result<(AgentToolResult, AgentToolEffect), ApplicationError> {
    let args = call.arguments.as_object();
    let expected_final_path = profile.output.message_body_path.as_str();
    let final_path = args
        .and_then(|args| required_trimmed_string_arg(args, "final_path"))
        .unwrap_or(expected_final_path);
    let final_path = match parse_workspace_path(call, final_path) {
        Ok(path) => path,
        Err(result) => return Ok((result, AgentToolEffect::None)),
    };
    if final_path.as_str() != expected_final_path {
        return Ok((
            tool_error(
                call,
                "workspace.final_path_mismatch",
                &format!(
                    "final_path must be the current profile message body artifact path `{expected_final_path}`"
                ),
            ),
            AgentToolEffect::None,
        ));
    }

    let result = AgentToolResult {
        call_id: call.id.clone(),
        name: call.name.clone(),
        content: format!("Finished with final artifact {}.", final_path.as_str()),
        structured: json!({
            "finalPath": final_path.as_str(),
            "reason": args.and_then(|args| required_trimmed_string_arg(args, "reason")),
        }),
        is_error: false,
        error_code: None,
        resource_refs: vec![final_path.as_str().to_string()],
    };

    Ok((result, AgentToolEffect::Finish { final_path }))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::finish;
    use crate::application::services::agent_tools::AgentToolEffect;
    use crate::domain::models::agent::AgentToolCall;
    use crate::domain::models::agent::profile::ResolvedAgentProfile;

    #[test]
    fn finish_defaults_to_profile_message_body_path() {
        let call = AgentToolCall {
            id: "call_1".to_string(),
            name: "workspace.finish".to_string(),
            arguments: json!({}),
            provider_metadata: json!(null),
        };
        let profile = test_profile("output/reply.md");

        let (_result, effect) = finish(&call, &profile).expect("finish");

        match effect {
            AgentToolEffect::Finish { final_path } => {
                assert_eq!(final_path.as_str(), "output/reply.md");
            }
            _ => panic!("expected finish effect"),
        }
    }

    #[test]
    fn finish_rejects_non_profile_final_path_as_recoverable_tool_error() {
        let call = AgentToolCall {
            id: "call_1".to_string(),
            name: "workspace.finish".to_string(),
            arguments: json!({ "final_path": "output/main.md" }),
            provider_metadata: json!(null),
        };
        let profile = test_profile("output/reply.md");

        let (result, effect) = finish(&call, &profile).expect("finish");

        assert!(result.is_error);
        assert_eq!(
            result.error_code.as_deref(),
            Some("workspace.final_path_mismatch")
        );
        assert!(matches!(effect, AgentToolEffect::None));
    }

    fn test_profile(message_body_path: &str) -> ResolvedAgentProfile {
        serde_json::from_value(json!({
            "schemaVersion": 1,
            "kind": "tauritavern.agentProfile",
            "id": "test",
            "displayName": "Test",
            "preset": {
                "mode": "none",
                "required": false
            },
            "model": {
                "mode": "currentPromptSnapshot"
            },
            "instructions": {},
            "tools": {
                "allow": ["workspace.write_file", "workspace.finish"],
                "deny": [],
                "toolDescriptions": {},
                "maxRounds": 1,
                "maxCallsPerRun": 1,
                "maxCallsPerTool": {}
            },
            "skills": {
                "visible": ["*"],
                "deny": [],
                "maxReadCharsPerCall": 1,
                "maxReadCharsPerRun": 1
            },
            "workspace": {
                "visibleRoots": ["output"],
                "writableRoots": ["output"]
            },
            "plan": {
                "mode": "none",
                "beta": true,
                "nodes": []
            },
            "output": {
                "artifacts": [{
                    "id": "main",
                    "path": message_body_path,
                    "kind": "markdown",
                    "target": "message_body",
                    "required": true,
                    "assemblyOrder": 0
                }],
                "messageBodyArtifactId": "main",
                "messageBodyPath": message_body_path
            },
            "sourceTrace": {
                "profileSource": "test"
            }
        }))
        .expect("test profile")
    }
}
