use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{RwLock, watch};

use crate::application::services::agent_model_gateway::AgentModelGateway;
use crate::application::services::agent_tools::{AgentToolDispatcher, BuiltinAgentToolRegistry};
use crate::application::services::skill_service::SkillService;
use crate::domain::repositories::agent_run_repository::AgentRunRepository;
use crate::domain::repositories::chat_repository::ChatRepository;
use crate::domain::repositories::checkpoint_repository::CheckpointRepository;
use crate::domain::repositories::group_chat_repository::GroupChatRepository;
use crate::domain::repositories::workspace_repository::WorkspaceRepository;

mod artifacts;
mod commit;
mod executor;
mod ids;
mod journal;
mod lifecycle;
mod loop_runner;
mod model_response_store;
mod model_turn;
mod prompt_snapshot;
mod tool_execution;

#[cfg(test)]
mod tests;

pub(super) type AgentCancelReceiver = watch::Receiver<bool>;
pub(super) const MAX_AGENT_TOOL_ROUNDS: usize = 80;

pub struct AgentRuntimeService {
    run_repository: Arc<dyn AgentRunRepository>,
    workspace_repository: Arc<dyn WorkspaceRepository>,
    checkpoint_repository: Arc<dyn CheckpointRepository>,
    model_gateway: Arc<dyn AgentModelGateway>,
    tool_registry: BuiltinAgentToolRegistry,
    tool_dispatcher: AgentToolDispatcher,
    active_runs: RwLock<HashMap<String, watch::Sender<bool>>>,
}

impl AgentRuntimeService {
    pub fn new(
        run_repository: Arc<dyn AgentRunRepository>,
        workspace_repository: Arc<dyn WorkspaceRepository>,
        checkpoint_repository: Arc<dyn CheckpointRepository>,
        chat_repository: Arc<dyn ChatRepository>,
        group_chat_repository: Arc<dyn GroupChatRepository>,
        skill_service: Arc<SkillService>,
        model_gateway: Arc<dyn AgentModelGateway>,
    ) -> Self {
        let tool_registry = BuiltinAgentToolRegistry::phase2c();
        let tool_dispatcher = AgentToolDispatcher::new(
            run_repository.clone(),
            chat_repository,
            group_chat_repository,
            workspace_repository.clone(),
            skill_service,
        );
        Self {
            run_repository,
            workspace_repository,
            checkpoint_repository,
            model_gateway,
            tool_registry,
            tool_dispatcher,
            active_runs: RwLock::new(HashMap::new()),
        }
    }
}
