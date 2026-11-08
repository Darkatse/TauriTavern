use tt_application::dto::agent_dto::AgentListProfilesResultDto;
use tt_application::dto::settings_dto::TauriTavernSettingsDto;
use tt_application::errors::ApplicationError;
use tt_application::services::agent_profile_service::default_writer_profile;
use tt_domain::models::settings::TauriTavernSettings;

pub fn native_defaults() -> Result<serde_json::Value, ApplicationError> {
    Ok(serde_json::json!({
        "settings": TauriTavernSettingsDto::from(TauriTavernSettings::default()),
        "agentProfiles": AgentListProfilesResultDto {
            profiles: vec![default_writer_profile()?.summary()],
            issues: Vec::new(),
        },
    }))
}
