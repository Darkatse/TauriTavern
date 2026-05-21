use serde_json::json;

use super::super::dispatcher::AgentToolEffect;
use super::super::session::AgentToolSession;
use crate::application::errors::ApplicationError;
use crate::domain::models::agent::profile::{AgentSkillPolicy, ResolvedAgentProfile};
use crate::domain::models::agent::{AgentToolCall, AgentToolResult};

pub(in crate::application::services::agent_tools) async fn list(
    call: &AgentToolCall,
    session: &AgentToolSession,
    profile: &ResolvedAgentProfile,
) -> Result<(AgentToolResult, AgentToolEffect), ApplicationError> {
    let skills = session
        .effective_skills()
        .iter()
        .filter(|skill| skill_is_visible(&profile.skills, skill.name.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    let content = if skills.is_empty() {
        "No Agent Skills are visible in the current profile.".to_string()
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

pub(super) fn skill_is_visible(policy: &AgentSkillPolicy, name: &str) -> bool {
    if policy
        .deny
        .iter()
        .any(|denied| denied == "*" || denied == name)
    {
        return false;
    }
    policy
        .visible
        .iter()
        .any(|visible| visible == "*" || visible == name)
}

#[cfg(test)]
mod tests {
    use super::skill_is_visible;
    use crate::domain::models::agent::profile::AgentSkillPolicy;

    #[test]
    fn wildcard_deny_hides_skills_even_when_visible_allows_all() {
        let policy = AgentSkillPolicy {
            visible: vec!["*".to_string()],
            deny: vec!["*".to_string()],
            max_read_chars_per_call: 1,
            max_read_chars_per_run: 1,
        };

        assert!(!skill_is_visible(&policy, "writer"));
        assert!(!skill_is_visible(&policy, "editor"));
    }
}
