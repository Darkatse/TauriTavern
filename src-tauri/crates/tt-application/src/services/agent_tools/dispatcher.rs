use std::sync::Arc;
use std::time::Instant;

use super::chat;
use super::dice;
use super::session::AgentToolSession;
use super::skill;
use super::workspace;
use super::world_info;
use crate::errors::ApplicationError;
use crate::services::skill_service::SkillService;
use tt_domain::models::agent::profile::ResolvedAgentProfile;
use tt_domain::models::agent::{
    AgentChatCommitMode, AgentToolResult, WorkspaceFileWriteMode, WorkspacePath,
};
use tt_domain::models::tool::{ToolId, ToolInvocation};
use tt_ports::repositories::agent_run_repository::AgentRunRepository;
use tt_ports::repositories::chat_repository::ChatRepository;
use tt_ports::repositories::group_chat_repository::GroupChatRepository;
use tt_ports::repositories::workspace_repository::{WorkspaceFile, WorkspaceRepository};
use tt_ports::skill_script::SkillScriptEngine;

const RUN_PROMPT_SNAPSHOT_PATH: &str = "input/prompt_snapshot.json";

#[derive(Debug, Clone)]
pub(crate) struct AgentToolDispatchOutcome {
    pub result: AgentToolResult,
    pub effect: AgentToolEffect,
    pub elapsed_ms: u128,
}

#[derive(Debug, Clone)]
pub(crate) enum AgentToolEffect {
    None,
    WorkspaceFileWritten {
        file: WorkspaceFile,
        mode: WorkspaceFileWriteMode,
    },
    WorkspaceFilePatched {
        file: WorkspaceFile,
        replacements: usize,
        old_sha256: String,
    },
    /// 一次工具调用批量写入多个工作区文件（如 skill 脚本的最终 delta）。
    /// 所有文件进入 journal / 事件；最后一次 mutation 单独供 auto-commit 使用。
    WorkspaceFilesWritten {
        files: Vec<WorkspaceFile>,
        last_text_mutation: Option<WorkspacePath>,
    },
    ChatCommitRequested {
        path: WorkspacePath,
        mode: AgentChatCommitMode,
        reason: Option<String>,
    },
    TaskReturned {
        status: tt_domain::models::agent::AgentTaskStatus,
        result_ref: WorkspacePath,
        summary: String,
    },
    HandoffAccepted {
        task_id: String,
        new_invocation_id: String,
    },
    Finish,
}

pub(crate) struct AgentToolDispatcher {
    run_repository: Arc<dyn AgentRunRepository>,
    chat_repository: Arc<dyn ChatRepository>,
    group_chat_repository: Arc<dyn GroupChatRepository>,
    workspace_repository: Arc<dyn WorkspaceRepository>,
    skill_service: Arc<SkillService>,
    skill_script_engine: Arc<dyn SkillScriptEngine>,
}

impl AgentToolDispatcher {
    pub(crate) fn new(
        run_repository: Arc<dyn AgentRunRepository>,
        chat_repository: Arc<dyn ChatRepository>,
        group_chat_repository: Arc<dyn GroupChatRepository>,
        workspace_repository: Arc<dyn WorkspaceRepository>,
        skill_service: Arc<SkillService>,
        skill_script_engine: Arc<dyn SkillScriptEngine>,
    ) -> Self {
        Self {
            run_repository,
            chat_repository,
            group_chat_repository,
            workspace_repository,
            skill_service,
            skill_script_engine,
        }
    }

    pub(crate) async fn dispatch(
        &self,
        run_id: &str,
        call: &ToolInvocation,
        session: &mut AgentToolSession,
        profile: &ResolvedAgentProfile,
    ) -> Result<AgentToolDispatchOutcome, ApplicationError> {
        self.dispatch_with_model_workspace_repository(
            run_id,
            call,
            session,
            profile,
            self.workspace_repository.as_ref(),
        )
        .await
    }

    pub(crate) async fn dispatch_with_model_workspace_repository(
        &self,
        run_id: &str,
        call: &ToolInvocation,
        session: &mut AgentToolSession,
        profile: &ResolvedAgentProfile,
        model_workspace_repository: &dyn WorkspaceRepository,
    ) -> Result<AgentToolDispatchOutcome, ApplicationError> {
        let started = Instant::now();
        let outcome = match builtin_tool_name(&call.tool_id)? {
            chat::CHAT_SEARCH => {
                chat::search(
                    self.run_repository.as_ref(),
                    self.chat_repository.as_ref(),
                    self.group_chat_repository.as_ref(),
                    run_id,
                    call,
                    &session.frozen_macros,
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
                    &session.frozen_macros,
                )
                .await?
            }
            world_info::WORLDINFO_READ_ACTIVATED => {
                // WorldInfo activation is a hidden run input fact, not a model-visible
                // workspace file; invocation workspace policy must not gate this read.
                let prompt_snapshot = self.read_run_prompt_snapshot(run_id).await?;
                world_info::read_activated(&prompt_snapshot, call)?
            }
            dice::DICE_ROLL => dice::roll(call).await?,
            skill::SKILL_LIST => skill::list(call, session, profile).await?,
            skill::SKILL_SEARCH => {
                skill::search(self.skill_service.as_ref(), call, session, profile).await?
            }
            skill::SKILL_READ => {
                skill::read(self.skill_service.as_ref(), call, session, profile).await?
            }
            skill::SKILL_SCRIPT => {
                let prompt_snapshot = self.read_run_prompt_snapshot(run_id).await?;
                skill::script(
                    skill::ScriptContext {
                        skill_service: self.skill_service.as_ref(),
                        engine: self.skill_script_engine.as_ref(),
                        workspace_repository: model_workspace_repository,
                        run_id,
                        prompt_snapshot,
                    },
                    call,
                    session,
                    profile,
                )
                .await?
            }
            workspace::WORKSPACE_LIST_FILES => {
                workspace::list_files(model_workspace_repository, run_id, call).await?
            }
            workspace::WORKSPACE_SEARCH_FILES => {
                workspace::search_files(model_workspace_repository, run_id, call).await?
            }
            workspace::WORKSPACE_READ_FILE => {
                workspace::read_file(model_workspace_repository, run_id, call, session).await?
            }
            workspace::WORKSPACE_WRITE_FILE => {
                workspace::write_file(model_workspace_repository, run_id, call, session).await?
            }
            workspace::WORKSPACE_APPLY_PATCH => {
                workspace::apply_patch(model_workspace_repository, run_id, call, session).await?
            }
            workspace::WORKSPACE_COMMIT => {
                workspace::commit(model_workspace_repository, run_id, call, profile).await?
            }
            workspace::WORKSPACE_FINISH => workspace::finish(call)?,
            other => {
                return Err(ApplicationError::InternalError(format!(
                    "tool.dispatch_handler_missing: admitted builtin tool `builtin:{other}` has no execution handler"
                )));
            }
        };

        Ok(AgentToolDispatchOutcome {
            result: outcome.0,
            effect: outcome.1,
            elapsed_ms: started.elapsed().as_millis(),
        })
    }

    async fn read_run_prompt_snapshot(
        &self,
        run_id: &str,
    ) -> Result<serde_json::Value, ApplicationError> {
        let snapshot_path = WorkspacePath::parse(RUN_PROMPT_SNAPSHOT_PATH)?;
        let snapshot_file = self
            .workspace_repository
            .read_text(run_id, &snapshot_path)
            .await
            .map_err(ApplicationError::from)?;
        serde_json::from_str(&snapshot_file.text).map_err(|error| {
            ApplicationError::ValidationError(format!(
                "agent.invalid_prompt_snapshot_file: failed to parse prompt snapshot JSON: {error}"
            ))
        })
    }
}

fn builtin_tool_name(tool_id: &ToolId) -> Result<&str, ApplicationError> {
    if tool_id.is_builtin() {
        return Ok(tool_id.native_name());
    }
    Err(ApplicationError::InternalError(format!(
        "tool.executor_unavailable: no executor is registered for tool `{tool_id}`"
    )))
}

#[cfg(test)]
mod tests {
    use tt_domain::models::tool::{ToolId, ToolProviderId};

    use super::builtin_tool_name;

    #[test]
    fn builtin_dispatch_does_not_accept_external_tools_with_the_same_native_name() {
        let external = ToolId::new(
            &ToolProviderId::parse("mcp/registration-1").unwrap(),
            "workspace.finish",
        )
        .unwrap();

        let error = builtin_tool_name(&external).unwrap_err();
        assert!(error.to_string().contains("tool.executor_unavailable"));
    }
}
