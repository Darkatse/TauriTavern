use async_trait::async_trait;

use crate::domain::errors::DomainError;
use crate::domain::models::agent::profile::{
    AgentProfileDefinition, AgentProfileId, AgentProfileSummary,
};

#[async_trait]
pub trait AgentProfileRepository: Send + Sync {
    async fn list_profiles(&self) -> Result<Vec<AgentProfileSummary>, DomainError>;

    async fn load_profile(
        &self,
        id: &AgentProfileId,
    ) -> Result<Option<AgentProfileDefinition>, DomainError>;

    async fn save_profile(&self, profile: &AgentProfileDefinition) -> Result<(), DomainError>;

    async fn delete_profile(&self, id: &AgentProfileId) -> Result<(), DomainError>;
}
