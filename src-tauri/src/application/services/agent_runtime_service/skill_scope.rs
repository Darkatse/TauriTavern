use serde_json::json;

use crate::application::dto::agent_dto::AgentStartRunDto;
use crate::application::errors::ApplicationError;
use crate::domain::models::agent::profile::{
    AgentPresetBindingMode, AgentPresetRef, ResolvedAgentProfile,
};
use crate::domain::models::agent::{AgentChatRef, AgentRunSkillScopeRefs};
use crate::domain::models::skill::{SkillIndexEntry, SkillScope};

pub(super) fn resolve_run_skill_scope_refs(
    dto: &AgentStartRunDto,
    profile: &ResolvedAgentProfile,
) -> Result<AgentRunSkillScopeRefs, ApplicationError> {
    let explicit_preset = dto
        .skill_scope_refs
        .preset
        .as_ref()
        .map(clone_valid_preset_ref)
        .transpose()?;
    let preset = if profile.preset.mode == AgentPresetBindingMode::Ref {
        let profile_preset = profile.preset.ref_.as_ref().ok_or_else(|| {
            ApplicationError::ValidationError(
                "agent.profile_preset_ref_required: preset.ref is required when preset.mode is ref"
                    .to_string(),
            )
        })?;
        let profile_preset = clone_valid_preset_ref(profile_preset)?;
        if let Some(explicit) = explicit_preset.as_ref() {
            if explicit != &profile_preset {
                return Err(ApplicationError::ValidationError(format!(
                    "agent.skill_scope_preset_mismatch: skillScopeRefs.preset `{}/{}` does not match profile preset `{}/{}`",
                    explicit.api_id, explicit.name, profile_preset.api_id, profile_preset.name
                )));
            }
        }
        Some(profile_preset)
    } else {
        explicit_preset
    };

    let chat_character_id = match &dto.chat_ref {
        AgentChatRef::Character { character_id, .. } => Some(character_id.as_str()),
        AgentChatRef::Group { .. } => None,
    };
    let explicit_character_id = normalized_optional_string(
        dto.skill_scope_refs.character_id.as_deref(),
        "agent.skill_scope_character_id_empty: skillScopeRefs.characterId cannot be empty",
    )?;
    if let (Some(explicit), Some(chat)) = (explicit_character_id.as_deref(), chat_character_id) {
        if explicit != chat {
            return Err(ApplicationError::ValidationError(format!(
                "agent.skill_scope_character_mismatch: skillScopeRefs.characterId `{explicit}` does not match chat character `{chat}`"
            )));
        }
    }

    Ok(AgentRunSkillScopeRefs {
        preset,
        character_id: explicit_character_id.or_else(|| chat_character_id.map(str::to_string)),
    })
}

pub(super) fn skill_scope_order_for_profile(
    profile: &ResolvedAgentProfile,
    refs: &AgentRunSkillScopeRefs,
) -> Result<Vec<SkillScope>, ApplicationError> {
    let mut scopes = vec![SkillScope::Global];

    let preset = match profile.preset.mode {
        AgentPresetBindingMode::Ref => {
            let preset = profile.preset.ref_.as_ref().ok_or_else(|| {
                ApplicationError::ValidationError(
                    "agent.profile_preset_ref_required: preset.ref is required when preset.mode is ref"
                        .to_string(),
                )
            })?;
            Some(preset)
        }
        AgentPresetBindingMode::CurrentPromptSnapshot => refs.preset.as_ref(),
        AgentPresetBindingMode::None => None,
    };
    if let Some(preset) = preset {
        let preset = clone_valid_preset_ref(preset)?;
        scopes.push(SkillScope::Preset {
            api_id: preset.api_id,
            name: preset.name,
        });
    }

    scopes.push(SkillScope::Profile {
        profile_id: profile.id.as_str().to_string(),
    });

    if let Some(character_id) = normalized_optional_string(
        refs.character_id.as_deref(),
        "agent.skill_scope_character_id_empty: persisted skillScopeRefs.characterId cannot be empty",
    )? {
        scopes.push(SkillScope::Character { character_id });
    }

    Ok(scopes)
}

pub(super) fn skill_event_summary(skills: &[SkillIndexEntry]) -> Vec<serde_json::Value> {
    skills
        .iter()
        .map(|skill| {
            json!({
                "name": skill.name.as_str(),
                "scope": &skill.scope,
                "installedHash": skill.installed_hash.as_str(),
            })
        })
        .collect()
}

fn clone_valid_preset_ref(preset: &AgentPresetRef) -> Result<AgentPresetRef, ApplicationError> {
    let api_id = normalized_required_string(
        preset.api_id.as_str(),
        "agent.skill_scope_preset_invalid: skillScopeRefs.preset.apiId cannot be empty",
    )?;
    let name = normalized_required_string(
        preset.name.as_str(),
        "agent.skill_scope_preset_invalid: skillScopeRefs.preset.name cannot be empty",
    )?;
    Ok(AgentPresetRef { api_id, name })
}

fn normalized_optional_string(
    value: Option<&str>,
    empty_message: &str,
) -> Result<Option<String>, ApplicationError> {
    match value {
        Some(value) => normalized_required_string(value, empty_message).map(Some),
        None => Ok(None),
    }
}

fn normalized_required_string(
    value: &str,
    empty_message: &str,
) -> Result<String, ApplicationError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(ApplicationError::ValidationError(empty_message.to_string()));
    }
    Ok(value.to_string())
}
