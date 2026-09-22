use async_trait::async_trait;

use tt_domain::errors::DomainError;
use tt_domain::models::agent::AgentModelMessage;
use tt_domain::models::agent::profile::AgentProfileDefinition;
use tt_domain::models::agent::session::{AgentSession, AgentSessionMessage};

#[derive(Debug, Clone)]
pub struct AgentSessionMessageReadQuery {
    /// Read forward after this sequence; mutually exclusive with before_seq.
    pub after_seq: Option<u64>,
    /// Otherwise return the latest page before this sequence, or before EOF.
    pub before_seq: Option<u64>,
    /// Positive result limit. Public API bounds are applied by the application.
    pub limit: usize,
}

#[async_trait]
pub trait AgentSessionRepository: Send + Sync {
    async fn load_session_profile(&self) -> Result<Option<AgentProfileDefinition>, DomainError>;

    async fn save_session_profile(
        &self,
        profile: &AgentProfileDefinition,
    ) -> Result<(), DomainError>;

    async fn create_session(&self, session: &AgentSession) -> Result<(), DomainError>;

    async fn load_session(&self, session_id: &str) -> Result<AgentSession, DomainError>;

    async fn append_session_message(
        &self,
        session_id: &str,
        run_id: &str,
        message: &AgentModelMessage,
    ) -> Result<AgentSessionMessage, DomainError>;

    async fn read_session_messages(
        &self,
        session_id: &str,
        query: AgentSessionMessageReadQuery,
    ) -> Result<Vec<AgentSessionMessage>, DomainError>;

    async fn session_last_seq(&self, session_id: &str) -> Result<u64, DomainError>;
}
