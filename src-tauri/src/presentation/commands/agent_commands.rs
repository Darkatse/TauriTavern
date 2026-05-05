use std::sync::Arc;

use tauri::State;

use crate::app::AppState;
use crate::application::dto::agent_dto::{
    AgentCancelRunDto, AgentListProfilesResultDto, AgentLoadProfileResultDto,
    AgentModelTurnDisplayDto, AgentProfileIdDto, AgentReadEventsDto, AgentReadEventsResultDto,
    AgentReadModelTurnDto, AgentReadWorkspaceFileDto, AgentResolveChatCommitDto, AgentRunHandleDto,
    AgentSaveProfileDto, AgentStartRunDto, AgentWorkspaceFileDto,
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
pub async fn list_agent_profiles(
    app_state: State<'_, Arc<AppState>>,
) -> Result<AgentListProfilesResultDto, CommandError> {
    log_command("list_agent_profiles");

    app_state
        .agent_profile_service
        .list_profiles()
        .await
        .map(|profiles| AgentListProfilesResultDto { profiles })
        .map_err(map_command_error("Failed to list agent profiles"))
}

#[tauri::command]
pub async fn load_agent_profile(
    dto: AgentProfileIdDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<AgentLoadProfileResultDto, CommandError> {
    log_command("load_agent_profile");

    app_state
        .agent_profile_service
        .load_profile(&dto.profile_id)
        .await
        .map(|profile| AgentLoadProfileResultDto { profile })
        .map_err(map_command_error("Failed to load agent profile"))
}

#[tauri::command]
pub async fn save_agent_profile(
    dto: AgentSaveProfileDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command("save_agent_profile");

    let known_tools = app_state.agent_runtime_service.tool_specs().to_vec();
    app_state
        .agent_profile_service
        .save_profile(dto.profile, &known_tools)
        .await
        .map_err(map_command_error("Failed to save agent profile"))
}

#[tauri::command]
pub async fn delete_agent_profile(
    dto: AgentProfileIdDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command("delete_agent_profile");

    app_state
        .agent_profile_service
        .delete_profile(&dto.profile_id)
        .await
        .map_err(map_command_error("Failed to delete agent profile"))
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
pub async fn read_agent_model_turn(
    dto: AgentReadModelTurnDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<AgentModelTurnDisplayDto, CommandError> {
    log_command("read_agent_model_turn");

    app_state
        .agent_runtime_service
        .read_model_turn(dto)
        .await
        .map_err(map_command_error("Failed to read agent model turn"))
}

#[tauri::command]
pub async fn resolve_agent_chat_commit(
    dto: AgentResolveChatCommitDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command("resolve_agent_chat_commit");

    app_state
        .agent_runtime_service
        .resolve_chat_commit(dto)
        .await
        .map_err(map_command_error("Failed to resolve agent chat commit"))
}
