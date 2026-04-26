use std::sync::Arc;
use std::time::Instant;

use super::workspace;
use crate::application::errors::ApplicationError;
use crate::domain::models::agent::{AgentToolCall, AgentToolResult, WorkspacePath};
use crate::domain::repositories::workspace_repository::{WorkspaceFile, WorkspaceRepository};

#[derive(Debug, Clone)]
pub struct AgentToolDispatchOutcome {
    pub result: AgentToolResult,
    pub effect: AgentToolEffect,
    pub elapsed_ms: u128,
}

#[derive(Debug, Clone)]
pub enum AgentToolEffect {
    None,
    WorkspaceFileWritten { file: WorkspaceFile },
    Finish { final_path: WorkspacePath },
}

pub struct AgentToolDispatcher {
    workspace_repository: Arc<dyn WorkspaceRepository>,
}

impl AgentToolDispatcher {
    pub fn new(workspace_repository: Arc<dyn WorkspaceRepository>) -> Self {
        Self {
            workspace_repository,
        }
    }

    pub async fn dispatch(
        &self,
        run_id: &str,
        call: &AgentToolCall,
    ) -> Result<AgentToolDispatchOutcome, ApplicationError> {
        let started = Instant::now();
        let outcome = match call.name.as_str() {
            workspace::WORKSPACE_WRITE_FILE => {
                workspace::write_file(self.workspace_repository.as_ref(), run_id, call).await?
            }
            workspace::WORKSPACE_FINISH => workspace::finish(call)?,
            other => {
                return Err(ApplicationError::PermissionDenied(format!(
                    "agent.tool_denied: unknown or unavailable tool `{other}`"
                )));
            }
        };

        Ok(AgentToolDispatchOutcome {
            result: outcome.0,
            effect: outcome.1,
            elapsed_ms: started.elapsed().as_millis(),
        })
    }
}
