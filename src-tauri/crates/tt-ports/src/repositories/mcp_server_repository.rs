use async_trait::async_trait;

use tt_domain::{
    errors::DomainError,
    models::mcp::{McpRegistrationId, McpServerRegistration},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpRegistrationStorageIssue {
    pub file_name: String,
    pub message: String,
}

#[derive(Debug, Clone, Default)]
pub struct McpRegistrationScan {
    pub registrations: Vec<McpServerRegistration>,
    pub issues: Vec<McpRegistrationStorageIssue>,
}

#[async_trait]
pub trait McpServerRepository: Send + Sync {
    async fn scan(&self) -> Result<McpRegistrationScan, DomainError>;

    async fn load(
        &self,
        id: &McpRegistrationId,
    ) -> Result<Option<McpServerRegistration>, DomainError>;

    async fn save(&self, registration: &McpServerRegistration) -> Result<(), DomainError>;

    async fn remove(&self, id: &McpRegistrationId) -> Result<(), DomainError>;
}
