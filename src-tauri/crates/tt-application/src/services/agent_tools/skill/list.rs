use serde::Serialize;

use super::super::dispatcher::AgentToolEffect;
use super::super::session::AgentToolSession;
use crate::errors::ApplicationError;
use tt_domain::models::agent::profile::{AgentSkillPolicy, ResolvedAgentProfile};
use tt_domain::models::agent::{AgentToolCall, AgentToolResult};
use tt_domain::models::skill::SkillIndexEntry;

use super::super::structured::structured_value;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SkillListStructured<'a> {
    skills: Vec<SkillListItem<'a>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SkillListItem<'a> {
    name: &'a str,
    description: &'a str,
}

pub(in crate::services::agent_tools) async fn list(
    call: &AgentToolCall,
    session: &AgentToolSession,
    profile: &ResolvedAgentProfile,
) -> Result<(AgentToolResult, AgentToolEffect), ApplicationError> {
    Ok((
        build_result(call, session.effective_skills(), &profile.skills),
        AgentToolEffect::None,
    ))
}

fn build_result(
    call: &AgentToolCall,
    effective_skills: &[SkillIndexEntry],
    policy: &AgentSkillPolicy,
) -> AgentToolResult {
    let skills = visible_skills(effective_skills, policy);
    let content = render_content(&skills);
    let structured = structured_value(SkillListStructured {
        skills: skill_list_items(&skills),
    });

    AgentToolResult {
        call_id: call.id.clone(),
        name: call.name.clone(),
        content,
        structured,
        is_error: false,
        error_code: None,
        resource_refs: Vec::new(),
    }
}

fn visible_skills<'a>(
    effective_skills: &'a [SkillIndexEntry],
    policy: &AgentSkillPolicy,
) -> Vec<&'a SkillIndexEntry> {
    effective_skills
        .iter()
        .filter(|skill| skill_is_visible(policy, skill.name.as_str()))
        .collect()
}

fn skill_list_items<'a>(skills: &[&'a SkillIndexEntry]) -> Vec<SkillListItem<'a>> {
    skills
        .iter()
        .map(|skill| SkillListItem {
            name: skill.name.as_str(),
            description: skill.description.as_str(),
        })
        .collect()
}

fn render_content(skills: &[&SkillIndexEntry]) -> String {
    if skills.is_empty() {
        "No Agent Skills are available under the current policy.".to_string()
    } else {
        let mut content = skills
            .iter()
            .map(|skill| format!("- {}: {}", skill.name, skill.description))
            .collect::<Vec<_>>()
            .join("\n");
        content.push_str(
            "\n\nUse skill_read with an exact skill name and path (default SKILL.md), plus start_line/line_count or start_char/max_chars when needed, to read exact text.",
        );
        content
    }
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
    use chrono::Utc;
    use serde_json::{Value, json};

    use super::{build_result, skill_is_visible};
    use tt_domain::models::agent::AgentToolCall;
    use tt_domain::models::agent::profile::AgentSkillPolicy;
    use tt_domain::models::skill::{SkillIndexEntry, SkillScope, SkillSourceRef};

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

    #[test]
    fn list_returns_only_model_facing_skill_summaries() {
        let skills = vec![skill_with_internal_fields()];
        let policy = AgentSkillPolicy {
            visible: vec!["*".to_string()],
            deny: Vec::new(),
            max_read_chars_per_call: 1,
            max_read_chars_per_run: 1,
        };
        let call = AgentToolCall {
            id: "call_skill_list".to_string(),
            name: "skill.list".to_string(),
            arguments: json!({}),
            provider_metadata: Value::Null,
        };

        let result = build_result(&call, &skills, &policy);

        assert!(!result.is_error);
        assert_eq!(
            result.structured,
            json!({
                "skills": [{
                    "name": "banned_list",
                    "description": "Keeps banned terms visible to the writer."
                }]
            })
        );
        assert!(
            result
                .content
                .contains("- banned_list: Keeps banned terms visible to the writer.")
        );
        assert!(
            result
                .content
                .contains("Use skill_read with an exact skill name and path")
        );

        let model_visible = serde_json::to_string(&json!({
            "content": result.content,
            "structured": result.structured,
        }))
        .expect("serialize model visible skill list");
        for hidden in [
            "installedHash",
            "fileCount",
            "totalBytes",
            "hasScripts",
            "hasBinary",
            "installedAt",
            "sourceRefs",
            "scope",
            "Display Name",
            "hash_should_not_leak",
        ] {
            assert!(
                !model_visible.contains(hidden),
                "skill_list leaked internal field or value `{hidden}`"
            );
        }
    }

    fn skill_with_internal_fields() -> SkillIndexEntry {
        SkillIndexEntry {
            scope: SkillScope::Preset {
                api_id: "openai".to_string(),
                name: "preset-one".to_string(),
            },
            name: "banned_list".to_string(),
            description: "Keeps banned terms visible to the writer.".to_string(),
            display_name: Some("Display Name".to_string()),
            source_kind: Some("preset".to_string()),
            license: Some("MIT".to_string()),
            author: Some("Author".to_string()),
            version: Some("1.0.0".to_string()),
            tags: vec!["style".to_string()],
            installed_hash: "hash_should_not_leak".to_string(),
            file_count: 1,
            total_bytes: 42,
            has_scripts: true,
            has_binary: false,
            installed_at: Utc::now(),
            source_refs: vec![SkillSourceRef {
                kind: "preset".to_string(),
                id: "preset-one".to_string(),
                label: "Preset One".to_string(),
                installed_hash: "source_hash_should_not_leak".to_string(),
            }],
        }
    }
}
