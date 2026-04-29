use std::sync::Arc;

use tauri::State;

use crate::app::AppState;
use crate::application::dto::agent_dto::{
    AgentCancelRunDto, AgentCommitDraftDto, AgentCommitResultDto, AgentFinalizeCommitDto,
    AgentPrepareCommitDto, AgentReadEventsDto, AgentReadEventsResultDto, AgentReadWorkspaceFileDto,
    AgentRunHandleDto, AgentStartRunDto, AgentWorkspaceFileDto,
};
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::errors::CommandError;

#[tauri::command]
pub async fn start_agent_run(
    dto: AgentStartRunDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<AgentRunHandleDto, CommandError> {
    log_command("start_agent_run");

    app_state
        .agent_runtime_service
        .start_run(dto)
        .await
        .map_err(map_command_error("Failed to start agent run"))
}

#[tauri::command]
pub async fn cancel_agent_run(
    dto: AgentCancelRunDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<AgentRunHandleDto, CommandError> {
    log_command("cancel_agent_run");

    app_state
        .agent_runtime_service
        .cancel_run(dto)
        .await
        .map_err(map_command_error("Failed to cancel agent run"))
}

#[tauri::command]
pub async fn read_agent_run_events(
    dto: AgentReadEventsDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<AgentReadEventsResultDto, CommandError> {
    log_command("read_agent_run_events");

    app_state
        .agent_runtime_service
        .read_events(dto)
        .await
        .map_err(map_command_error("Failed to read agent run events"))
}

#[tauri::command]
pub async fn read_agent_workspace_file(
    dto: AgentReadWorkspaceFileDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<AgentWorkspaceFileDto, CommandError> {
    log_command("read_agent_workspace_file");

    app_state
        .agent_runtime_service
        .read_workspace_file(dto)
        .await
        .map_err(map_command_error("Failed to read agent workspace file"))
}

#[tauri::command]
pub async fn prepare_agent_run_commit(
    dto: AgentPrepareCommitDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<AgentCommitDraftDto, CommandError> {
    log_command("prepare_agent_run_commit");

    app_state
        .agent_runtime_service
        .prepare_commit(dto)
        .await
        .map_err(map_command_error("Failed to prepare agent run commit"))
}

#[tauri::command]
pub async fn finalize_agent_run_commit(
    dto: AgentFinalizeCommitDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<AgentCommitResultDto, CommandError> {
    log_command("finalize_agent_run_commit");

    app_state
        .agent_runtime_service
        .finalize_commit(dto)
        .await
        .map_err(map_command_error("Failed to finalize agent run commit"))
}
