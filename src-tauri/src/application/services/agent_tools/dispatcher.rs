use std::sync::Arc;
use std::time::Instant;

use serde_json::json;

use super::chat;
use super::session::AgentToolSession;
use super::skill;
use super::workspace;
use super::world_info;
use crate::application::errors::ApplicationError;
use crate::application::services::skill_service::SkillService;
use crate::domain::models::agent::{AgentToolCall, AgentToolResult, WorkspacePath};
use crate::domain::repositories::agent_run_repository::AgentRunRepository;
use crate::domain::repositories::chat_repository::ChatRepository;
use crate::domain::repositories::group_chat_repository::GroupChatRepository;
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
    run_repository: Arc<dyn AgentRunRepository>,
    chat_repository: Arc<dyn ChatRepository>,
    group_chat_repository: Arc<dyn GroupChatRepository>,
    workspace_repository: Arc<dyn WorkspaceRepository>,
    skill_service: Arc<SkillService>,
}

impl AgentToolDispatcher {
    pub fn new(
        run_repository: Arc<dyn AgentRunRepository>,
        chat_repository: Arc<dyn ChatRepository>,
        group_chat_repository: Arc<dyn GroupChatRepository>,
        workspace_repository: Arc<dyn WorkspaceRepository>,
        skill_service: Arc<SkillService>,
    ) -> Self {
        Self {
            run_repository,
            chat_repository,
            group_chat_repository,
            workspace_repository,
            skill_service,
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
            chat::CHAT_SEARCH => {
                chat::search(
                    self.run_repository.as_ref(),
                    self.chat_repository.as_ref(),
                    self.group_chat_repository.as_ref(),
                    run_id,
                    call,
                )
                .await?
            }
            chat::CHAT_READ_MESSAGES => {
                chat::read_messages(
                    self.run_repository.as_ref(),
                    self.chat_repository.as_ref(),
                    self.group_chat_repository.as_ref(),
                    run_id,
                    call,
                )
                .await?
            }
            world_info::WORLDINFO_READ_ACTIVATED => {
                world_info::read_activated(self.workspace_repository.as_ref(), run_id, call).await?
            }
            skill::SKILL_LIST => skill::list(self.skill_service.as_ref(), call).await?,
            skill::SKILL_READ => skill::read(self.skill_service.as_ref(), call).await?,
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
