use crate::errors::ApplicationError;
use tt_domain::models::agent::profile::{
    AgentProfileDefinition, AgentProfileId, DEFAULT_AGENT_PROFILE_ID,
};
use tt_ports::repositories::agent_profile_storage_health_repository::AgentProfileStorageRepairAction;

use super::defaults::default_writer_profile;
use super::validation::{migrate_profile_schema, normalize_context_policy};
use super::{AgentProfileExternalReferencePolicy, AgentProfileList, AgentProfileService};

impl AgentProfileService {
    pub async fn list_profiles(&self) -> Result<AgentProfileList, ApplicationError> {
        let scan = self
            .profile_storage_health_repository
            .scan_profiles()
            .await
            .map_err(ApplicationError::from)?;
        let mut list = AgentProfileList {
            profiles: scan.profiles,
            issues: scan.issues,
        };
        if list
            .profiles
            .iter()
            .all(|profile| profile.id.as_str() != DEFAULT_AGENT_PROFILE_ID)
        {
            list.profiles.insert(0, default_writer_profile()?.summary());
        }
        Ok(list)
    }

    pub async fn load_profile(
        &self,
        profile_id: &str,
    ) -> Result<Option<AgentProfileDefinition>, ApplicationError> {
        let id = AgentProfileId::parse(profile_id).map_err(ApplicationError::ValidationError)?;
        let profile = self
            .profile_repository
            .load_profile(&id)
            .await
            .map_err(ApplicationError::from)?;
        if profile.is_none() && id.as_str() == DEFAULT_AGENT_PROFILE_ID {
            return Ok(Some(default_writer_profile()?));
        }
        profile
            .map(|mut profile| {
                migrate_profile_schema(&mut profile)?;
                Ok(profile)
            })
            .transpose()
    }

    pub async fn save_profile(
        &self,
        mut profile: AgentProfileDefinition,
        known_tools: &[tt_domain::models::agent::AgentToolSpec],
    ) -> Result<(), ApplicationError> {
        migrate_profile_schema(&mut profile)?;
        normalize_context_policy(&mut profile.context)?;
        self.resolve_definition(
            profile.clone(),
            format!("file:{}", profile.id.as_str()),
            known_tools,
            AgentProfileExternalReferencePolicy::AllowDangling,
        )
        .await?;
        self.profile_repository
            .save_profile(&profile)
            .await
            .map_err(ApplicationError::from)
    }

    pub async fn delete_profile(&self, profile_id: &str) -> Result<(), ApplicationError> {
        let id = AgentProfileId::parse(profile_id).map_err(ApplicationError::ValidationError)?;
        self.profile_repository
            .delete_profile(&id)
            .await
            .map_err(ApplicationError::from)
    }

    pub async fn repair_profile_file(
        &self,
        profile_id: &str,
        action: AgentProfileStorageRepairAction,
    ) -> Result<(), ApplicationError> {
        let id = AgentProfileId::parse(profile_id).map_err(ApplicationError::ValidationError)?;
        match action {
            AgentProfileStorageRepairAction::Delete => self
                .profile_repository
                .delete_profile(&id)
                .await
                .map_err(ApplicationError::from),
            AgentProfileStorageRepairAction::NormalizeIdentity => self
                .profile_storage_health_repository
                .normalize_profile_file_identity(&id)
                .await
                .map_err(ApplicationError::from),
        }
    }
}
