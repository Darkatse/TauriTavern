use std::sync::Arc;
use tauri::State;

use crate::app::AppState;
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::errors::CommandError;
use tt_application::dto::agent_dto::{
    AgentDeleteSessionDto, AgentListSessionsResultDto, AgentPrepareSessionRunDto,
    AgentPrepareSessionRunResultDto, AgentReadSessionDto, AgentReadSessionResultDto,
    AgentRenameSessionDto, AgentSaveProfileDto, AgentSessionProfileResultDto,
    AgentSessionResultDto, AgentSessionRunHandleDto, AgentStartSessionRunDto,
};

#[tauri::command]
pub async fn load_agent_session_profile(
    app_state: State<'_, Arc<AppState>>,
) -> Result<AgentSessionProfileResultDto, CommandError> {
    log_command("load_agent_session_profile");
    app_state
        .services
        .agent_runtime_service
        .load_session_profile()
        .await
        .map_err(map_command_error("Failed to load Session profile"))
}

#[tauri::command]
pub async fn save_agent_session_profile(
    dto: AgentSaveProfileDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command("save_agent_session_profile");
    app_state
        .services
        .agent_runtime_service
        .save_session_profile(dto)
        .await
        .map_err(map_command_error("Failed to save Session profile"))
}

#[tauri::command]
pub async fn create_agent_session(
    app_state: State<'_, Arc<AppState>>,
) -> Result<AgentSessionResultDto, CommandError> {
    log_command("create_agent_session");
    app_state
        .services
        .agent_runtime_service
        .create_session()
        .await
        .map_err(map_command_error("Failed to create Agent Session"))
}

#[tauri::command]
pub async fn list_agent_sessions(
    app_state: State<'_, Arc<AppState>>,
) -> Result<AgentListSessionsResultDto, CommandError> {
    log_command("list_agent_sessions");
    app_state
        .services
        .agent_runtime_service
        .list_sessions()
        .await
        .map_err(map_command_error("Failed to list Agent Sessions"))
}

#[tauri::command]
pub async fn rename_agent_session(
    dto: AgentRenameSessionDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<AgentSessionResultDto, CommandError> {
    log_command("rename_agent_session");
    app_state
        .services
        .agent_runtime_service
        .rename_session(dto)
        .await
        .map_err(map_command_error("Failed to rename Agent Session"))
}

#[tauri::command]
pub async fn delete_agent_session(
    dto: AgentDeleteSessionDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command("delete_agent_session");
    app_state
        .services
        .agent_runtime_service
        .delete_session(dto)
        .await
        .map_err(map_command_error("Failed to delete Agent Session"))
}

#[tauri::command]
pub async fn read_agent_session(
    dto: AgentReadSessionDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<AgentReadSessionResultDto, CommandError> {
    log_command("read_agent_session");
    app_state
        .services
        .agent_runtime_service
        .read_session(dto)
        .await
        .map_err(map_command_error("Failed to read Agent Session"))
}

#[tauri::command]
pub async fn prepare_agent_session_run(
    dto: AgentPrepareSessionRunDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<AgentPrepareSessionRunResultDto, CommandError> {
    log_command("prepare_agent_session_run");
    app_state
        .services
        .agent_runtime_service
        .prepare_session_run(dto)
        .await
        .map_err(map_command_error("Failed to prepare Agent Session run"))
}

#[tauri::command]
pub async fn start_agent_session_run(
    dto: AgentStartSessionRunDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<AgentSessionRunHandleDto, CommandError> {
    log_command("start_agent_session_run");
    app_state
        .services
        .agent_runtime_service
        .start_session_run(dto)
        .await
        .map_err(map_command_error("Failed to start Agent Session run"))
}
