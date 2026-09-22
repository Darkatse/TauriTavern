use std::sync::Arc;

use tt_domain::models::agent::profile::{AgentProfileId, AgentProfileSummary};
use tt_domain::models::tool::ToolCatalog;
use tt_ports::repositories::agent_profile_repository::AgentProfileRepository;
use tt_ports::repositories::agent_profile_storage_health_repository::{
    AgentProfileStorageHealthRepository, AgentProfileStorageIssue,
};
use tt_ports::repositories::agent_session_repository::AgentSessionRepository;
use tt_ports::repositories::preset_repository::PresetRepository;

mod constants;
mod defaults;
mod model_config;
mod output_policy;
mod preset_refs;
mod readiness;
mod resolver;
mod storage;
mod system_prompt;
mod validation;
mod workspace_policy;

#[cfg(test)]
mod tests;
#[cfg(test)]
pub(crate) use tests::TestAgentProfileRepository;

pub use model_config::{
    ensure_profile_model_configured, profile_model_configuration_error,
    profile_model_requires_configuration,
};
pub use output_policy::require_output;
pub(crate) use preset_refs::preset_exists_for_type;
pub use readiness::validate_chat_profile;
pub use system_prompt::materialize_agent_system_prompt;
pub(crate) use validation::is_retired_agent_tool;
pub use workspace_policy::{commit_policy_from_profile, workspace_roots_from_profile};

pub struct AgentProfileService {
    profile_repository: Arc<dyn AgentProfileRepository>,
    profile_storage_health_repository: Arc<dyn AgentProfileStorageHealthRepository>,
    preset_repository: Arc<dyn PresetRepository>,
    session_repository: Arc<dyn AgentSessionRepository>,
}

pub struct AgentProfileResolveInput<'a> {
    pub profile_id: Option<&'a str>,
    pub tool_catalog: &'a ToolCatalog,
}

#[derive(Debug, Clone, Default)]
pub struct AgentProfileList {
    pub profiles: Vec<AgentProfileSummary>,
    pub issues: Vec<AgentProfileStorageIssue>,
}

#[derive(Debug, Clone, Default)]
pub struct AgentProfilePresetRetargetResult {
    pub profile_ids: Vec<AgentProfileId>,
    pub session_profile_updated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AgentProfileExternalReferencePolicy {
    Strict,
    AllowDangling,
}

impl AgentProfileService {
    pub fn new(
        profile_repository: Arc<dyn AgentProfileRepository>,
        profile_storage_health_repository: Arc<dyn AgentProfileStorageHealthRepository>,
        preset_repository: Arc<dyn PresetRepository>,
        session_repository: Arc<dyn AgentSessionRepository>,
    ) -> Self {
        Self {
            profile_repository,
            profile_storage_health_repository,
            preset_repository,
            session_repository,
        }
    }
}
