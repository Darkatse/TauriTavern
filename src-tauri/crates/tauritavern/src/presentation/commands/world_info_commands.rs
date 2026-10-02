use std::sync::Arc;
use tauri::State;

use crate::app::AppState;
use crate::presentation::commands::chunk_body::{chunk_bytes_from_request, commit_headers};
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::errors::CommandError;
use tt_application::dto::world_info_dto::{
    DeleteWorldInfoDto, GetWorldInfoDto, ImportWorldInfoDto, ImportWorldInfoResponseDto,
    NormalizeWorldInfoNameDto, NormalizeWorldInfoNameResponseDto,
};
use tt_contracts::byte_commit::CommitBegin;

#[tauri::command]
pub async fn get_world_info(
    dto: GetWorldInfoDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<tauri::ipc::Response, CommandError> {
    log_command(format!("get_world_info, name: {}", dto.name));

    let result = app_state
        .services
        .world_info_service
        .get_world_info(&dto.name)
        .await
        .map_err(map_command_error("Failed to get world info"))?;
    Ok(tauri::ipc::Response::new(result.bytes))
}

#[tauri::command]
pub async fn normalize_world_info_name(
    dto: NormalizeWorldInfoNameDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<NormalizeWorldInfoNameResponseDto, CommandError> {
    log_command(format!(
        "normalize_world_info_name, import_filename: {}",
        dto.import_filename
    ));

    let name = app_state
        .services
        .world_info_service
        .normalize_world_info_name(&dto.name, dto.import_filename)
        .map_err(map_command_error("Failed to normalize world info name"))?;

    Ok(NormalizeWorldInfoNameResponseDto { name })
}

#[tauri::command]
pub async fn begin_world_info_commit(
    app_state: State<'_, Arc<AppState>>,
) -> Result<CommitBegin, CommandError> {
    app_state
        .services
        .world_info_service
        .begin_commit()
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn append_world_info_commit_chunk(
    request: tauri::ipc::Request<'_>,
    app_state: State<'_, Arc<AppState>>,
) -> Result<u64, CommandError> {
    let (session_id, offset) = commit_headers(&request)?;
    let bytes = chunk_bytes_from_request(&request)?;
    app_state
        .services
        .world_info_service
        .append_commit(session_id, offset, &bytes)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn finish_world_info_commit(
    session_id: String,
    expected_size: u64,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    app_state
        .services
        .world_info_service
        .finish_commit(&session_id, expected_size)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn abort_world_info_commit(
    session_id: String,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    app_state
        .services
        .world_info_service
        .abort_commit(&session_id)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn delete_world_info(
    dto: DeleteWorldInfoDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command(format!("delete_world_info, name: {}", dto.name));

    app_state
        .services
        .world_info_service
        .delete_world_info(&dto.name)
        .await
        .map_err(map_command_error("Failed to delete world info"))
}

#[tauri::command]
pub async fn import_world_info(
    dto: ImportWorldInfoDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<ImportWorldInfoResponseDto, CommandError> {
    log_command(format!(
        "import_world_info, original_filename: {}",
        dto.original_filename
    ));

    let name = app_state
        .services
        .world_info_service
        .import_world_info(&dto.file_path, &dto.original_filename, dto.converted_data)
        .await
        .map_err(map_command_error("Failed to import world info"))?;

    Ok(ImportWorldInfoResponseDto { name })
}
