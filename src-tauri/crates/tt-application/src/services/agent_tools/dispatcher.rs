use std::sync::Arc;
use std::time::Instant;

use serde_json::{Map, Value};
use tokio::sync::watch;

use super::chat;
use super::dice;
use super::session::AgentToolSession;
use super::workspace;
use super::world_info;
use crate::errors::ApplicationError;
use crate::services::agent_workspace_scope::{
    ChatSnapshot, ScopedWorkspaceFs, WorkspaceAccessPolicy,
};
use crate::services::skill_service::SkillService;
use tt_domain::models::agent::profile::ResolvedAgentProfile;
use tt_domain::models::agent::{
    AgentChatCommitMode, AgentToolResult, WorkspaceFileWriteMode, WorkspacePath,
};
use tt_domain::models::tool::{ToolId, ToolInvocation};
use tt_ports::repositories::agent_run_repository::AgentRunRepository;
use tt_ports::repositories::chat_repository::ChatRepository;
use tt_ports::repositories::group_chat_repository::GroupChatRepository;
use tt_ports::workspace_fs::{WorkspaceFile, WorkspaceFs};
use tt_ports::workspace_shell::WorkspaceShell;

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
    /// Direct filesystem operations need only a publication candidate, not a text delta.
    AutoCommitCandidateUpdated {
        path: Option<WorkspacePath>,
    },
    ChatCommitRequested {
        path: WorkspacePath,
        mode: AgentChatCommitMode,
        reason: String,
        /// The run ends once this commit is confirmed.
        finish: bool,
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
    /// Set by the runtime on a confirmed commit with `finish: true`; no tool returns it.
    Finish,
}

pub(crate) struct AgentToolDispatcher {
    run_repository: Arc<dyn AgentRunRepository>,
    chat_repository: Arc<dyn ChatRepository>,
    group_chat_repository: Arc<dyn GroupChatRepository>,
    skill_service: Arc<SkillService>,
    workspace_shell: Arc<dyn WorkspaceShell>,
}

impl AgentToolDispatcher {
    pub(crate) fn new(
        run_repository: Arc<dyn AgentRunRepository>,
        chat_repository: Arc<dyn ChatRepository>,
        group_chat_repository: Arc<dyn GroupChatRepository>,
        skill_service: Arc<SkillService>,
        workspace_shell: Arc<dyn WorkspaceShell>,
    ) -> Self {
        Self {
            run_repository,
            chat_repository,
            group_chat_repository,
            skill_service,
            workspace_shell,
        }
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "tool dispatch carries the current invocation context"
    )]
    pub(crate) async fn dispatch(
        &self,
        run_id: &str,
        call: &ToolInvocation,
        args: &Map<String, Value>,
        session: &mut AgentToolSession,
        profile: &ResolvedAgentProfile,
        raw_files: Arc<dyn WorkspaceFs>,
        cancel: watch::Receiver<bool>,
        auto_commit_candidate: Option<WorkspacePath>,
    ) -> Result<AgentToolDispatchOutcome, ApplicationError> {
        let started = Instant::now();
        let chat = session
            .chat
            .get_or_try_init(|| async {
                self.run_repository.load_run(run_id).await.map(|run| {
                    ChatSnapshot::for_run(
                        &run,
                        self.chat_repository.clone(),
                        self.group_chat_repository.clone(),
                    )
                })
            })
            .await?
            .clone();
        // Chat tools are offered only in Chat runs, which always have a chat.
        let run_chat = || {
            chat.as_deref().ok_or_else(|| {
                ApplicationError::InternalError(
                    "agent.chat_missing: chat tools read the chat of a Chat run".to_string(),
                )
            })
        };
        let workspace = ScopedWorkspaceFs::new(
            raw_files.clone(),
            WorkspaceAccessPolicy::from_profile(profile),
        )
        .with_skills(
            self.skill_service.file_repository(),
            session.effective_skills.clone(),
            session.runtime_context.frozen_macros.clone(),
        )
        .with_chat(chat.clone());
        let outcome = match builtin_tool_name(&call.tool_id)? {
            chat::CHAT_SEARCH => chat::search(run_chat()?, call, args).await?,
            chat::CHAT_READ_MESSAGES => chat::read_messages(run_chat()?, call, args).await?,
            world_info::WORLDINFO_READ_ACTIVATED => {
                // WorldInfo activation is a hidden run input fact, not a model-visible
                // workspace file; invocation workspace policy must not gate this read.
                let prompt_snapshot = Self::read_run_prompt_snapshot(raw_files.as_ref()).await?;
                world_info::read_activated(&prompt_snapshot, call, args)?
            }
            dice::DICE_ROLL => dice::roll(call, args).await?,
            workspace::WORKSPACE_LIST_FILES => {
                workspace::list_files(&workspace, call, args).await?
            }
            workspace::WORKSPACE_SEARCH_FILES => {
                workspace::search_files(&workspace, call, args).await?
            }
            workspace::WORKSPACE_READ_FILE => {
                workspace::read_file(&workspace, call, args, session).await?
            }
            workspace::WORKSPACE_WRITE_FILE => {
                workspace::write_file(&workspace, call, args, session).await?
            }
            workspace::WORKSPACE_APPLY_PATCH => {
                workspace::apply_patch(&workspace, call, args, session).await?
            }
            workspace::WORKSPACE_SHELL => {
                let workspace = match &profile.output {
                    Some(output) => workspace.track_text_mutations(
                        WorkspacePath::parse(&output.message_body_path)?,
                        auto_commit_candidate,
                    ),
                    None => workspace,
                };
                workspace::shell(
                    self.workspace_shell.as_ref(),
                    Arc::new(workspace),
                    session.runtime_context.clone(),
                    call,
                    args,
                    cancel,
                )
                .await?
            }
            workspace::WORKSPACE_COMMIT => {
                workspace::commit(&workspace, call, args, profile).await?
            }
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
        files: &dyn WorkspaceFs,
    ) -> Result<serde_json::Value, ApplicationError> {
        let snapshot_path = WorkspacePath::parse(RUN_PROMPT_SNAPSHOT_PATH)?;
        let snapshot_file = files
            .read_text(&snapshot_path)
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
            "workspace.commit",
        )
        .unwrap();

        let error = builtin_tool_name(&external).unwrap_err();
        assert!(error.to_string().contains("tool.executor_unavailable"));
    }
}
