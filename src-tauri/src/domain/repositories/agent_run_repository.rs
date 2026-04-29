use async_trait::async_trait;
use serde_json::Value;

use crate::domain::errors::DomainError;
use crate::domain::models::agent::{AgentRun, AgentRunEvent, AgentRunEventLevel};

#[derive(Debug, Clone, Copy)]
pub struct AgentRunEventReadQuery {
    pub after_seq: Option<u64>,
    pub before_seq: Option<u64>,
    pub limit: usize,
}

#[async_trait]
pub trait AgentRunRepository: Send + Sync {
    async fn create_run(&self, run: &AgentRun) -> Result<(), DomainError>;

    async fn load_run(&self, run_id: &str) -> Result<AgentRun, DomainError>;

    async fn save_run(&self, run: &AgentRun) -> Result<(), DomainError>;

    async fn append_event(
        &self,
        run_id: &str,
        level: AgentRunEventLevel,
        event_type: &str,
        payload: Value,
    ) -> Result<AgentRunEvent, DomainError>;

    async fn read_events(
        &self,
        run_id: &str,
        query: AgentRunEventReadQuery,
    ) -> Result<Vec<AgentRunEvent>, DomainError>;
}
