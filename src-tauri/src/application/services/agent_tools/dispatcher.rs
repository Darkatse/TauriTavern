use std::sync::Arc;
use std::time::Instant;

use serde_json::json;

use super::session::AgentToolSession;
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
    WorkspaceFileWritten {
        file: WorkspaceFile,
    },
    WorkspaceFilePatched {
        file: WorkspaceFile,
        replacements: usize,
        old_sha256: String,
    },
    Finish {
        final_path: WorkspacePath,
    },
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
        session: &mut AgentToolSession,
    ) -> Result<AgentToolDispatchOutcome, ApplicationError> {
        let started = Instant::now();
        let outcome = match call.name.as_str() {
            workspace::WORKSPACE_LIST_FILES => {
                workspace::list_files(self.workspace_repository.as_ref(), run_id, call).await?
            }
            workspace::WORKSPACE_READ_FILE => {
                workspace::read_file(self.workspace_repository.as_ref(), run_id, call, session)
                    .await?
            }
            workspace::WORKSPACE_WRITE_FILE => {
                workspace::write_file(self.workspace_repository.as_ref(), run_id, call, session)
                    .await?
            }
            workspace::WORKSPACE_APPLY_PATCH => {
                workspace::apply_patch(self.workspace_repository.as_ref(), run_id, call, session)
                    .await?
            }
            workspace::WORKSPACE_FINISH => workspace::finish(call)?,
            other => (
                AgentToolResult {
                    call_id: call.id.clone(),
                    name: call.name.clone(),
                    content: format!("Unknown or unavailable tool `{other}`."),
                    structured: json!({
                        "error": {
                            "code": "agent.tool_denied",
                            "message": format!("Unknown or unavailable tool `{other}`."),
                        }
                    }),
                    is_error: true,
                    error_code: Some("agent.tool_denied".to_string()),
                    resource_refs: Vec::new(),
                },
                AgentToolEffect::None,
            ),
        };

        Ok(AgentToolDispatchOutcome {
            result: outcome.0,
            effect: outcome.1,
            elapsed_ms: started.elapsed().as_millis(),
        })
    }
}
