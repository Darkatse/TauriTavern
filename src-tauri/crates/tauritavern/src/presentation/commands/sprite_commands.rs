use std::sync::Arc;

use tauri::State;
use tt_application::dto::sprite_dto::{
    DeleteSpriteDto, ListSpritesDto, SpriteDto, UploadSpriteDto, UploadSpritePackDto,
};

use crate::app::AppState;
use crate::presentation::errors::CommandError;

use super::helpers::{log_command, map_command_error};

#[tauri::command]
pub async fn list_sprites(
    dto: ListSpritesDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<Vec<SpriteDto>, CommandError> {
    log_command("list_sprites");
    app_state
        .services
        .sprite_service
        .list(dto)
        .await
        .map_err(map_command_error("Failed to list sprites"))
}

#[tauri::command]
pub async fn upload_sprite(
    dto: UploadSpriteDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command("upload_sprite");
    app_state
        .services
        .sprite_service
        .upload(dto)
        .await
        .map_err(map_command_error("Failed to upload sprite"))
}

#[tauri::command]
pub async fn upload_sprite_pack(
    dto: UploadSpritePackDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<usize, CommandError> {
    log_command("upload_sprite_pack");
    app_state
        .services
        .sprite_service
        .upload_pack(dto)
        .await
        .map_err(map_command_error("Failed to upload sprite pack"))
}

#[tauri::command]
pub async fn delete_sprite(
    dto: DeleteSpriteDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command("delete_sprite");
    app_state
        .services
        .sprite_service
        .delete(dto)
        .await
        .map_err(map_command_error("Failed to delete sprite"))
}
