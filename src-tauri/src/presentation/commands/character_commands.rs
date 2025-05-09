use std::sync::Arc;
use tauri::State;

use crate::app::AppState;
use crate::application::services::character_service::CharacterService;
use crate::application::dto::character_dto::{
    CharacterDto, CreateCharacterDto, UpdateCharacterDto, RenameCharacterDto,
    ImportCharacterDto, ExportCharacterDto, UpdateAvatarDto, CreateWithAvatarDto,
    DeleteCharacterDto, GetCharacterChatsDto, CharacterChatDto
};
use crate::presentation::errors::CommandError;
use crate::infrastructure::logging::logger;

/// Get all characters
#[tauri::command]
pub async fn get_all_characters(
    shallow: bool,
    app_state: State<'_, Arc<AppState>>,
) -> Result<Vec<CharacterDto>, CommandError> {
    logger::debug(&format!("Command: get_all_characters (shallow: {})", shallow));

    app_state.character_service.get_all_characters(shallow).await
        .map_err(|e| {
            logger::error(&format!("Failed to get all characters: {}", e));
            e.into()
        })
}

/// Get a character by name
#[tauri::command]
pub async fn get_character(
    name: String,
    app_state: State<'_, Arc<AppState>>,
) -> Result<CharacterDto, CommandError> {
    logger::debug(&format!("Command: get_character {}", name));

    app_state.character_service.get_character(&name).await
        .map_err(|e| {
            logger::error(&format!("Failed to get character {}: {}", name, e));
            e.into()
        })
}

/// Create a new character
#[tauri::command]
pub async fn create_character(
    dto: CreateCharacterDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<CharacterDto, CommandError> {
    logger::debug(&format!("Command: create_character {}", dto.name));

    app_state.character_service.create_character(dto).await
        .map_err(|e| {
            logger::error(&format!("Failed to create character: {}", e));
            e.into()
        })
}

/// Create a character with an avatar
#[tauri::command]
pub async fn create_character_with_avatar(
    dto: CreateWithAvatarDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<CharacterDto, CommandError> {
    logger::debug(&format!("Command: create_character_with_avatar {}", dto.character.name));

    app_state.character_service.create_with_avatar(dto).await
        .map_err(|e| {
            logger::error(&format!("Failed to create character with avatar: {}", e));
            e.into()
        })
}

/// Update a character
#[tauri::command]
pub async fn update_character(
    name: String,
    dto: UpdateCharacterDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<CharacterDto, CommandError> {
    logger::debug(&format!("Command: update_character {}", name));

    app_state.character_service.update_character(&name, dto).await
        .map_err(|e| {
            logger::error(&format!("Failed to update character: {}", e));
            e.into()
        })
}

/// Delete a character
#[tauri::command]
pub async fn delete_character(
    dto: DeleteCharacterDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    logger::debug(&format!("Command: delete_character {}", dto.name));

    app_state.character_service.delete_character(dto).await
        .map_err(|e| {
            logger::error(&format!("Failed to delete character: {}", e));
            e.into()
        })
}

/// Rename a character
#[tauri::command]
pub async fn rename_character(
    dto: RenameCharacterDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<CharacterDto, CommandError> {
    logger::debug(&format!("Command: rename_character {} -> {}", dto.old_name, dto.new_name));

    app_state.character_service.rename_character(dto).await
        .map_err(|e| {
            logger::error(&format!("Failed to rename character: {}", e));
            e.into()
        })
}

/// Import a character
#[tauri::command]
pub async fn import_character(
    dto: ImportCharacterDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<CharacterDto, CommandError> {
    logger::debug(&format!("Command: import_character from {}", dto.file_path));

    app_state.character_service.import_character(dto).await
        .map_err(|e| {
            logger::error(&format!("Failed to import character: {}", e));
            e.into()
        })
}

/// Export a character
#[tauri::command]
pub async fn export_character(
    dto: ExportCharacterDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    logger::debug(&format!("Command: export_character {} to {}", dto.name, dto.target_path));

    app_state.character_service.export_character(dto).await
        .map_err(|e| {
            logger::error(&format!("Failed to export character: {}", e));
            e.into()
        })
}

/// Update a character's avatar
#[tauri::command]
pub async fn update_avatar(
    dto: UpdateAvatarDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    logger::debug(&format!("Command: update_avatar for {}", dto.name));

    app_state.character_service.update_avatar(dto).await
        .map_err(|e| {
            logger::error(&format!("Failed to update avatar: {}", e));
            e.into()
        })
}

/// Get character chats by character ID
#[tauri::command]
pub async fn get_character_chats_by_id(
    dto: GetCharacterChatsDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<Vec<CharacterChatDto>, CommandError> {
    logger::debug(&format!("Command: get_character_chats_by_id for {}", dto.name));

    app_state.character_service.get_character_chats(dto).await
        .map_err(|e| {
            logger::error(&format!("Failed to get character chats: {}", e));
            e.into()
        })
}

/// Clear the character cache
#[tauri::command]
pub async fn clear_character_cache(
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    logger::debug("Command: clear_character_cache");

    app_state.character_service.clear_cache().await
        .map_err(|e| {
            logger::error(&format!("Failed to clear character cache: {}", e));
            e.into()
        })
}
