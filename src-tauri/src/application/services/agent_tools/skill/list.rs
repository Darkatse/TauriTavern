use serde_json::json;

use super::super::dispatcher::AgentToolEffect;
use crate::application::errors::ApplicationError;
use crate::application::services::skill_service::SkillService;
use crate::domain::models::agent::{AgentToolCall, AgentToolResult};

pub(in crate::application::services::agent_tools) async fn list(
    skill_service: &SkillService,
    call: &AgentToolCall,
) -> Result<(AgentToolResult, AgentToolEffect), ApplicationError> {
    let skills = skill_service.list_skills().await?;
    let content = if skills.is_empty() {
        "No Agent Skills are installed.".to_string()
    } else {
        skills
            .iter()
            .map(|skill| {
                let display = skill
                    .display_name
                    .as_deref()
                    .filter(|value| !value.is_empty())
                    .unwrap_or(skill.name.as_str());
                format!(
                    "- {}: {}{}",
                    skill.name,
                    skill.description,
                    if display == skill.name {
                        String::new()
                    } else {
                        format!(" (display name: {display})")
                    }
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    Ok((
        AgentToolResult {
            call_id: call.id.clone(),
            name: call.name.clone(),
            content,
            structured: json!({ "skills": skills }),
            is_error: false,
            error_code: None,
            resource_refs: Vec::new(),
        },
        AgentToolEffect::None,
    ))
}
