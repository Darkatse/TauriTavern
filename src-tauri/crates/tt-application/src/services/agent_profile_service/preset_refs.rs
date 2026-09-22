use crate::errors::ApplicationError;
use tt_domain::models::agent::profile::{
    AgentPresetBinding, AgentPresetBindingMode, AgentPresetRef,
};
use tt_domain::models::preset::PresetType;
use tt_ports::repositories::preset_repository::PresetRepository;

use super::validation::{
    migrate_profile_schema, normalize_context_policy, validate_profile_header,
};
use super::{
    AgentProfileExternalReferencePolicy, AgentProfilePresetRetargetResult, AgentProfileService,
};

impl AgentProfileService {
    pub async fn retarget_preset_refs(
        &self,
        from: AgentPresetRef,
        to: AgentPresetRef,
    ) -> Result<AgentProfilePresetRetargetResult, ApplicationError> {
        let (from, to) =
            validate_preset_retarget_pair(from, to, self.preset_repository.as_ref()).await?;
        let summaries = self.list_profiles().await?.profiles;
        let mut updated = Vec::new();

        for summary in summaries {
            let Some(mut profile) = self
                .profile_repository
                .load_profile(&summary.id)
                .await
                .map_err(ApplicationError::from)?
            else {
                continue;
            };
            let migrated = migrate_profile_schema(&mut profile)?;
            validate_profile_header(&profile)?;
            let retargeted = retarget_preset_binding(&mut profile.preset, &from, &to);
            if retargeted {
                normalize_context_policy(&mut profile.context)?;
            }
            if migrated || retargeted {
                self.profile_repository.save_profile(&profile).await?;
            }
            if retargeted {
                updated.push(profile.id);
            }
        }

        let mut session_profile_updated = false;
        if let Some(mut profile) = self.session_repository.load_session_profile().await?
            && retarget_preset_binding(&mut profile.preset, &from, &to)
        {
            self.session_repository
                .save_session_profile(&profile)
                .await?;
            session_profile_updated = true;
        }
        Ok(AgentProfilePresetRetargetResult {
            profile_ids: updated,
            session_profile_updated,
        })
    }
}

fn retarget_preset_binding(
    binding: &mut AgentPresetBinding,
    from: &AgentPresetRef,
    to: &AgentPresetRef,
) -> bool {
    if binding.mode != AgentPresetBindingMode::Ref || binding.ref_.as_ref() != Some(from) {
        return false;
    }
    binding.ref_ = Some(to.clone());
    true
}

pub(super) async fn validate_preset_binding(
    binding: &AgentPresetBinding,
    preset_repository: &dyn PresetRepository,
    external_reference_policy: AgentProfileExternalReferencePolicy,
) -> Result<(), ApplicationError> {
    match binding.mode {
        AgentPresetBindingMode::CurrentPromptSnapshot | AgentPresetBindingMode::None => Ok(()),
        AgentPresetBindingMode::Ref => {
            let Some(ref_) = binding.ref_.as_ref() else {
                return Err(ApplicationError::ValidationError(
                    "agent.profile_preset_ref_required: preset.ref is required when preset.mode is ref"
                        .to_string(),
                ));
            };
            let preset_type = PresetType::from_api_id(ref_.api_id.as_str()).ok_or_else(|| {
                ApplicationError::ValidationError(format!(
                    "agent.profile_preset_api_invalid: unsupported preset apiId `{}`",
                    ref_.api_id
                ))
            })?;
            if ref_.name.trim().is_empty() {
                return Err(ApplicationError::ValidationError(
                    "agent.profile_preset_name_required: preset.ref.name cannot be empty"
                        .to_string(),
                ));
            }
            if !binding.required
                || external_reference_policy == AgentProfileExternalReferencePolicy::AllowDangling
            {
                return Ok(());
            }
            if !preset_exists_for_type(preset_repository, ref_.name.as_str(), &preset_type).await? {
                return Err(ApplicationError::ValidationError(format!(
                    "agent.profile_preset_missing: required preset `{}` for apiId `{}` does not exist",
                    ref_.name, ref_.api_id
                )));
            }
            Ok(())
        }
    }
}

async fn validate_preset_retarget_pair(
    from: AgentPresetRef,
    to: AgentPresetRef,
    preset_repository: &dyn PresetRepository,
) -> Result<(AgentPresetRef, AgentPresetRef), ApplicationError> {
    let from = normalize_preset_ref(from, "from")?;
    let to = normalize_preset_ref(to, "to")?;
    if from == to {
        return Err(ApplicationError::ValidationError(
            "agent.profile_preset_retarget_same_ref: from and to preset refs must differ"
                .to_string(),
        ));
    }
    if from.api_id != to.api_id {
        return Err(ApplicationError::ValidationError(
            "agent.profile_preset_retarget_api_mismatch: preset refs cannot be retargeted across apiId"
                .to_string(),
        ));
    }
    let preset_type = PresetType::from_api_id(to.api_id.as_str()).ok_or_else(|| {
        ApplicationError::ValidationError(format!(
            "agent.profile_preset_api_invalid: unsupported preset apiId `{}`",
            to.api_id
        ))
    })?;
    if !preset_exists_for_type(preset_repository, to.name.as_str(), &preset_type).await? {
        return Err(ApplicationError::ValidationError(format!(
            "agent.profile_preset_retarget_target_missing: target preset `{}` for apiId `{}` does not exist",
            to.name, to.api_id
        )));
    }
    Ok((from, to))
}

pub(crate) async fn preset_exists_for_type(
    preset_repository: &dyn PresetRepository,
    name: &str,
    preset_type: &PresetType,
) -> Result<bool, ApplicationError> {
    Ok(preset_repository.preset_exists(name, preset_type).await?
        || preset_repository
            .get_default_preset(name, preset_type)
            .await?
            .is_some())
}

fn normalize_preset_ref(
    mut ref_: AgentPresetRef,
    label: &str,
) -> Result<AgentPresetRef, ApplicationError> {
    ref_.api_id = ref_.api_id.trim().to_string();
    ref_.name = ref_.name.trim().to_string();
    if ref_.api_id.is_empty() {
        return Err(ApplicationError::ValidationError(format!(
            "agent.profile_preset_retarget_{label}_api_required: {label}.apiId cannot be empty"
        )));
    }
    if ref_.name.is_empty() {
        return Err(ApplicationError::ValidationError(format!(
            "agent.profile_preset_retarget_{label}_name_required: {label}.name cannot be empty"
        )));
    }
    Ok(ref_)
}
