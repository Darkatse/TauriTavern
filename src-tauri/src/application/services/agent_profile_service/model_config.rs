use crate::application::errors::ApplicationError;
use crate::domain::models::agent::profile::{AgentModelBindingMode, ResolvedAgentProfile};

pub fn profile_model_requires_configuration(profile: &ResolvedAgentProfile) -> bool {
    matches!(
        profile.model.mode,
        AgentModelBindingMode::RequiresConfiguration
    )
}

pub fn profile_model_configuration_error(profile: &ResolvedAgentProfile) -> String {
    format!(
        "agent.profile_model_requires_configuration: Agent profile `{}` requires a local model selection before it can run",
        profile.id.as_str()
    )
}

pub fn ensure_profile_model_configured(
    profile: &ResolvedAgentProfile,
) -> Result<(), ApplicationError> {
    if profile_model_requires_configuration(profile) {
        return Err(ApplicationError::ValidationError(
            profile_model_configuration_error(profile),
        ));
    }
    Ok(())
}
