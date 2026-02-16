use std::sync::Arc;

use tauri::State;

use crate::app::AppState;
use crate::application::dto::character_dto::{
    CharacterChatDto, CharacterDto, CreateCharacterDto, CreateWithAvatarDto, DeleteCharacterDto,
    ExportCharacterContentDto, ExportCharacterContentResultDto, ExportCharacterDto,
    GetCharacterChatsDto, ImportCharacterDto, RenameCharacterDto, UpdateAvatarDto,
    UpdateCharacterDto,
};
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::errors::CommandError;

#[tauri::command]
pub async fn get_all_characters(
    shallow: bool,
    app_state: State<'_, Arc<AppState>>,
) -> Result<Vec<CharacterDto>, CommandError> {
    log_command(format!("get_all_characters (shallow: {})", shallow));

    app_state
        .character_service
        .get_all_characters(shallow)
        .await
        .map_err(map_command_error("Failed to get all characters"))
}

#[tauri::command]
pub async fn get_character(
    name: String,
    app_state: State<'_, Arc<AppState>>,
) -> Result<CharacterDto, CommandError> {
    log_command(format!("get_character {}", name));

    app_state
        .character_service
        .get_character(&name)
        .await
        .map_err(map_command_error(format!(
            "Failed to get character {}",
            name
        )))
}

#[tauri::command]
pub async fn create_character(
    dto: CreateCharacterDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<CharacterDto, CommandError> {
    log_command(format!("create_character {}", dto.name));

    app_state
        .character_service
        .create_character(dto)
        .await
        .map_err(map_command_error("Failed to create character"))
}

#[tauri::command]
pub async fn create_character_with_avatar(
    dto: CreateWithAvatarDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<CharacterDto, CommandError> {
    log_command(format!(
        "create_character_with_avatar {}",
        dto.character.name
    ));

    app_state
        .character_service
        .create_with_avatar(dto)
        .await
        .map_err(map_command_error("Failed to create character with avatar"))
}

#[tauri::command]
pub async fn update_character(
    name: String,
    dto: UpdateCharacterDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<CharacterDto, CommandError> {
    log_command(format!("update_character {}", name));

    app_state
        .character_service
        .update_character(&name, dto)
        .await
        .map_err(map_command_error("Failed to update character"))
}

#[tauri::command]
pub async fn delete_character(
    dto: DeleteCharacterDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command(format!("delete_character {}", dto.name));

    app_state
        .character_service
        .delete_character(dto)
        .await
        .map_err(map_command_error("Failed to delete character"))
}

#[tauri::command]
pub async fn rename_character(
    dto: RenameCharacterDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<CharacterDto, CommandError> {
    log_command(format!(
        "rename_character {} -> {}",
        dto.old_name, dto.new_name
    ));

    app_state
        .character_service
        .rename_character(dto)
        .await
        .map_err(map_command_error("Failed to rename character"))
}

#[tauri::command]
pub async fn import_character(
    dto: ImportCharacterDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<CharacterDto, CommandError> {
    log_command(format!("import_character from {}", dto.file_path));

    app_state
        .character_service
        .import_character(dto)
        .await
        .map_err(map_command_error("Failed to import character"))
}

#[tauri::command]
pub async fn export_character(
    dto: ExportCharacterDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command(format!(
        "export_character {} to {}",
        dto.name, dto.target_path
    ));

    app_state
        .character_service
        .export_character(dto)
        .await
        .map_err(map_command_error("Failed to export character"))
}

#[tauri::command]
pub async fn export_character_content(
    dto: ExportCharacterContentDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<ExportCharacterContentResultDto, CommandError> {
    log_command(format!(
        "export_character_content {} format {}",
        dto.name, dto.format
    ));

    app_state
        .character_service
        .export_character_content(dto)
        .await
        .map_err(map_command_error("Failed to export character content"))
}

#[tauri::command]
pub async fn update_avatar(
    dto: UpdateAvatarDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command(format!("update_avatar for {}", dto.name));

    app_state
        .character_service
        .update_avatar(dto)
        .await
        .map_err(map_command_error("Failed to update avatar"))
}

#[tauri::command]
pub async fn get_character_chats_by_id(
    dto: GetCharacterChatsDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<Vec<CharacterChatDto>, CommandError> {
    log_command(format!("get_character_chats_by_id for {}", dto.name));

    app_state
        .character_service
        .get_character_chats(dto)
        .await
        .map_err(map_command_error("Failed to get character chats"))
}

#[tauri::command]
pub async fn clear_character_cache(
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command("clear_character_cache");

    app_state
        .character_service
        .clear_cache()
        .await
        .map_err(map_command_error("Failed to clear character cache"))
}
