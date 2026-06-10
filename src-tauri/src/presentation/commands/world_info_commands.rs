use serde_json::Value;
use std::sync::Arc;
use tauri::State;

use crate::app::AppState;
use crate::application::dto::world_info_dto::{
    DeleteWorldInfoDto, GetWorldInfoDto, GetWorldInfosBatchDto, GetWorldInfosBatchResponseDto,
    ImportWorldInfoDto, ImportWorldInfoResponseDto, NormalizeWorldInfoNameDto,
    NormalizeWorldInfoNameResponseDto, SaveWorldInfoDto,
};
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::errors::CommandError;

#[tauri::command]
pub async fn get_world_info(
    dto: GetWorldInfoDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<Value, CommandError> {
    log_command(format!("get_world_info, name: {}", dto.name));

    app_state
        .world_info_service
        .get_world_info(&dto.name)
        .await
        .map_err(map_command_error("Failed to get world info"))
}

#[tauri::command]
pub async fn get_world_infos_batch(
    dto: GetWorldInfosBatchDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<GetWorldInfosBatchResponseDto, CommandError> {
    log_command(format!("get_world_infos_batch, count: {}", dto.names.len()));

    let items = app_state
        .world_info_service
        .get_world_infos_batch(dto.names)
        .await
        .map_err(map_command_error("Failed to get world infos batch"))?;

    Ok(GetWorldInfosBatchResponseDto { items })
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
        .world_info_service
        .normalize_world_info_name(&dto.name, dto.import_filename)
        .map_err(map_command_error("Failed to normalize world info name"))?;

    Ok(NormalizeWorldInfoNameResponseDto { name })
}

#[tauri::command]
pub async fn save_world_info(
    dto: SaveWorldInfoDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command(format!("save_world_info, name: {}", dto.name));

    app_state
        .world_info_service
        .save_world_info(&dto.name, dto.data)
        .await
        .map_err(map_command_error("Failed to save world info"))
}

#[tauri::command]
pub async fn delete_world_info(
    dto: DeleteWorldInfoDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command(format!("delete_world_info, name: {}", dto.name));

    app_state
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
        .world_info_service
        .import_world_info(&dto.file_path, &dto.original_filename, dto.converted_data)
        .await
        .map_err(map_command_error("Failed to import world info"))?;

    Ok(ImportWorldInfoResponseDto { name })
}
