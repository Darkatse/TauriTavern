use std::sync::Arc;
use tauri::State;

use crate::application::services::chat_service::ChatService;
use crate::application::dto::chat_dto::{
    ChatDto, ChatSearchResultDto,
    CreateChatDto, AddMessageDto, RenameChatDto, ImportChatDto, ExportChatDto
};
use crate::presentation::errors::CommandError;
use crate::infrastructure::logging::logger;

/// Get all chats
#[tauri::command]
pub async fn get_all_chats(
    app_state: State<'_, Arc<crate::app::AppState>>,
) -> Result<Vec<ChatDto>, CommandError> {
    logger::debug("Command: get_all_chats");

    app_state.chat_service.get_all_chats().await
        .map_err(|e| {
            logger::error(&format!("Failed to get all chats: {}", e));
            e.into()
        })
}

/// Get a chat by character name and file name
#[tauri::command]
pub async fn get_chat(
    character_name: String,
    file_name: String,
    app_state: State<'_, Arc<crate::app::AppState>>,
) -> Result<ChatDto, CommandError> {
    logger::debug(&format!("Command: get_chat {}/{}", character_name, file_name));

    app_state.chat_service.get_chat(&character_name, &file_name).await
        .map_err(|e| {
            logger::error(&format!("Failed to get chat {}/{}: {}", character_name, file_name, e));
            e.into()
        })
}

/// Get all chats for a character
#[tauri::command]
pub async fn get_character_chats(
    character_name: String,
    app_state: State<'_, Arc<crate::app::AppState>>,
) -> Result<Vec<ChatDto>, CommandError> {
    logger::debug(&format!("Command: get_character_chats {}", character_name));

    app_state.chat_service.get_character_chats(&character_name).await
        .map_err(|e| {
            logger::error(&format!("Failed to get chats for character {}: {}", character_name, e));
            e.into()
        })
}

/// Create a new chat
#[tauri::command]
pub async fn create_chat(
    dto: CreateChatDto,
    app_state: State<'_, Arc<crate::app::AppState>>,
) -> Result<ChatDto, CommandError> {
    logger::debug(&format!("Command: create_chat for character {}", dto.character_name));

    app_state.chat_service.create_chat(dto).await
        .map_err(|e| {
            logger::error(&format!("Failed to create chat: {}", e));
            e.into()
        })
}

/// Add a message to a chat
#[tauri::command]
pub async fn add_message(
    dto: AddMessageDto,
    app_state: State<'_, Arc<crate::app::AppState>>,
) -> Result<ChatDto, CommandError> {
    logger::debug(&format!("Command: add_message to chat {}/{}", dto.character_name, dto.file_name));

    app_state.chat_service.add_message(dto).await
        .map_err(|e| {
            logger::error(&format!("Failed to add message to chat: {}", e));
            e.into()
        })
}

/// Rename a chat
#[tauri::command]
pub async fn rename_chat(
    dto: RenameChatDto,
    app_state: State<'_, Arc<crate::app::AppState>>,
) -> Result<(), CommandError> {
    logger::debug(&format!("Command: rename_chat {}/{} -> {}/{}",
        dto.character_name, dto.old_file_name, dto.character_name, dto.new_file_name));

    app_state.chat_service.rename_chat(dto).await
        .map_err(|e| {
            logger::error(&format!("Failed to rename chat: {}", e));
            e.into()
        })
}

/// Delete a chat
#[tauri::command]
pub async fn delete_chat(
    character_name: String,
    file_name: String,
    app_state: State<'_, Arc<crate::app::AppState>>,
) -> Result<(), CommandError> {
    logger::debug(&format!("Command: delete_chat {}/{}", character_name, file_name));

    app_state.chat_service.delete_chat(&character_name, &file_name).await
        .map_err(|e| {
            logger::error(&format!("Failed to delete chat {}/{}: {}", character_name, file_name, e));
            e.into()
        })
}

/// Search for chats
#[tauri::command]
pub async fn search_chats(
    query: String,
    character_filter: Option<String>,
    app_state: State<'_, Arc<crate::app::AppState>>,
) -> Result<Vec<ChatSearchResultDto>, CommandError> {
    logger::debug(&format!("Command: search_chats {}", query));

    let character_filter_ref = character_filter.as_deref();

    app_state.chat_service.search_chats(&query, character_filter_ref).await
        .map_err(|e| {
            logger::error(&format!("Failed to search chats: {}", e));
            e.into()
        })
}

/// Import a chat
#[tauri::command]
pub async fn import_chat(
    dto: ImportChatDto,
    app_state: State<'_, Arc<crate::app::AppState>>,
) -> Result<ChatDto, CommandError> {
    logger::debug(&format!("Command: import_chat for character {} from {}", dto.character_name, dto.file_path));

    app_state.chat_service.import_chat(dto).await
        .map_err(|e| {
            logger::error(&format!("Failed to import chat: {}", e));
            e.into()
        })
}

/// Export a chat
#[tauri::command]
pub async fn export_chat(
    dto: ExportChatDto,
    app_state: State<'_, Arc<crate::app::AppState>>,
) -> Result<(), CommandError> {
    logger::debug(&format!("Command: export_chat {}/{} to {}", dto.character_name, dto.file_name, dto.target_path));

    app_state.chat_service.export_chat(dto).await
        .map_err(|e| {
            logger::error(&format!("Failed to export chat: {}", e));
            e.into()
        })
}

/// Backup a chat
#[tauri::command]
pub async fn backup_chat(
    character_name: String,
    file_name: String,
    app_state: State<'_, Arc<crate::app::AppState>>,
) -> Result<(), CommandError> {
    logger::debug(&format!("Command: backup_chat {}/{}", character_name, file_name));

    app_state.chat_service.backup_chat(&character_name, &file_name).await
        .map_err(|e| {
            logger::error(&format!("Failed to backup chat {}/{}: {}", character_name, file_name, e));
            e.into()
        })
}

/// Clear the chat cache
#[tauri::command]
pub async fn clear_chat_cache(
    app_state: State<'_, Arc<crate::app::AppState>>,
) -> Result<(), CommandError> {
    logger::debug("Command: clear_chat_cache");

    app_state.chat_service.clear_cache().await
        .map_err(|e| {
            logger::error(&format!("Failed to clear chat cache: {}", e));
            e.into()
        })
}
