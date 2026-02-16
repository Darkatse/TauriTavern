use std::sync::Arc;

use tauri::State;

use crate::app::AppState;
use crate::application::dto::chat_dto::{
    AddMessageDto, ChatDto, ChatSearchResultDto, CreateChatDto, DeleteGroupChatDto, ExportChatDto,
    GetGroupChatDto, ImportCharacterChatsDto, ImportChatDto, ImportGroupChatDto, RenameChatDto,
    RenameGroupChatDto, SaveChatDto, SaveGroupChatDto,
};
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::errors::CommandError;
use serde_json::Value;

#[tauri::command]
pub async fn get_all_chats(
    app_state: State<'_, Arc<AppState>>,
) -> Result<Vec<ChatDto>, CommandError> {
    log_command("get_all_chats");

    app_state
        .chat_service
        .get_all_chats()
        .await
        .map_err(map_command_error("Failed to get all chats"))
}

#[tauri::command]
pub async fn get_chat(
    character_name: String,
    file_name: String,
    app_state: State<'_, Arc<AppState>>,
) -> Result<ChatDto, CommandError> {
    log_command(format!("get_chat {}/{}", character_name, file_name));

    app_state
        .chat_service
        .get_chat(&character_name, &file_name)
        .await
        .map_err(map_command_error(format!(
            "Failed to get chat {}/{}",
            character_name, file_name
        )))
}

#[tauri::command]
pub async fn get_character_chats(
    character_name: String,
    app_state: State<'_, Arc<AppState>>,
) -> Result<Vec<ChatDto>, CommandError> {
    log_command(format!("get_character_chats {}", character_name));

    app_state
        .chat_service
        .get_character_chats(&character_name)
        .await
        .map_err(map_command_error(format!(
            "Failed to get chats for character {}",
            character_name
        )))
}

#[tauri::command]
pub async fn create_chat(
    dto: CreateChatDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<ChatDto, CommandError> {
    log_command(format!("create_chat for character {}", dto.character_name));

    app_state
        .chat_service
        .create_chat(dto)
        .await
        .map_err(map_command_error("Failed to create chat"))
}

#[tauri::command]
pub async fn add_message(
    dto: AddMessageDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<ChatDto, CommandError> {
    log_command(format!(
        "add_message to chat {}/{}",
        dto.character_name, dto.file_name
    ));

    app_state
        .chat_service
        .add_message(dto)
        .await
        .map_err(map_command_error("Failed to add message to chat"))
}

#[tauri::command]
pub async fn rename_chat(
    dto: RenameChatDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command(format!(
        "rename_chat {}/{} -> {}/{}",
        dto.character_name, dto.old_file_name, dto.character_name, dto.new_file_name
    ));

    app_state
        .chat_service
        .rename_chat(dto)
        .await
        .map_err(map_command_error("Failed to rename chat"))
}

#[tauri::command]
pub async fn delete_chat(
    character_name: String,
    file_name: String,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command(format!("delete_chat {}/{}", character_name, file_name));

    app_state
        .chat_service
        .delete_chat(&character_name, &file_name)
        .await
        .map_err(map_command_error(format!(
            "Failed to delete chat {}/{}",
            character_name, file_name
        )))
}

#[tauri::command]
pub async fn search_chats(
    query: String,
    character_filter: Option<String>,
    app_state: State<'_, Arc<AppState>>,
) -> Result<Vec<ChatSearchResultDto>, CommandError> {
    log_command(format!("search_chats {}", query));

    app_state
        .chat_service
        .search_chats(&query, character_filter.as_deref())
        .await
        .map_err(map_command_error("Failed to search chats"))
}

#[tauri::command]
pub async fn import_chat(
    dto: ImportChatDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<ChatDto, CommandError> {
    log_command(format!(
        "import_chat for character {} from {}",
        dto.character_name, dto.file_path
    ));

    app_state
        .chat_service
        .import_chat(dto)
        .await
        .map_err(map_command_error("Failed to import chat"))
}

#[tauri::command]
pub async fn export_chat(
    dto: ExportChatDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command(format!(
        "export_chat {}/{} to {}",
        dto.character_name, dto.file_name, dto.target_path
    ));

    app_state
        .chat_service
        .export_chat(dto)
        .await
        .map_err(map_command_error("Failed to export chat"))
}

#[tauri::command]
pub async fn backup_chat(
    character_name: String,
    file_name: String,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command(format!("backup_chat {}/{}", character_name, file_name));

    app_state
        .chat_service
        .backup_chat(&character_name, &file_name)
        .await
        .map_err(map_command_error(format!(
            "Failed to backup chat {}/{}",
            character_name, file_name
        )))
}

#[tauri::command]
pub async fn clear_chat_cache(app_state: State<'_, Arc<AppState>>) -> Result<(), CommandError> {
    log_command("clear_chat_cache");

    app_state
        .chat_service
        .clear_cache()
        .await
        .map_err(map_command_error("Failed to clear chat cache"))
}

#[tauri::command]
pub async fn save_chat(
    dto: SaveChatDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command(format!(
        "save_chat {}/{}",
        dto.character_name, dto.file_name
    ));

    app_state
        .chat_service
        .save_chat(dto)
        .await
        .map_err(map_command_error("Failed to save chat"))
}

#[tauri::command]
pub async fn get_chat_payload(
    character_name: String,
    file_name: String,
    app_state: State<'_, Arc<AppState>>,
) -> Result<Vec<Value>, CommandError> {
    log_command(format!("get_chat_payload {}/{}", character_name, file_name));

    app_state
        .chat_service
        .get_chat_payload(&character_name, &file_name)
        .await
        .map_err(map_command_error(format!(
            "Failed to get chat payload {}/{}",
            character_name, file_name
        )))
}

#[tauri::command]
pub async fn get_group_chat(
    dto: GetGroupChatDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<Vec<Value>, CommandError> {
    log_command(format!("get_group_chat {}", dto.id));

    app_state
        .chat_service
        .get_group_chat(dto)
        .await
        .map_err(map_command_error("Failed to get group chat payload"))
}

#[tauri::command]
pub async fn save_group_chat(
    dto: SaveGroupChatDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command(format!("save_group_chat {}", dto.id));

    app_state
        .chat_service
        .save_group_chat(dto)
        .await
        .map_err(map_command_error("Failed to save group chat payload"))
}

#[tauri::command]
pub async fn delete_group_chat(
    dto: DeleteGroupChatDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command(format!("delete_group_chat {}", dto.id));

    app_state
        .chat_service
        .delete_group_chat(dto)
        .await
        .map_err(map_command_error("Failed to delete group chat payload"))
}

#[tauri::command]
pub async fn rename_group_chat(
    dto: RenameGroupChatDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command(format!(
        "rename_group_chat {} -> {}",
        dto.old_file_name, dto.new_file_name
    ));

    app_state
        .chat_service
        .rename_group_chat(dto)
        .await
        .map_err(map_command_error("Failed to rename group chat payload"))
}

#[tauri::command]
pub async fn import_character_chats(
    dto: ImportCharacterChatsDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<Vec<String>, CommandError> {
    log_command(format!("import_character_chats {}", dto.character_name));

    app_state
        .chat_service
        .import_character_chats(dto)
        .await
        .map_err(map_command_error("Failed to import character chats"))
}

#[tauri::command]
pub async fn import_group_chat_payload(
    dto: ImportGroupChatDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<String, CommandError> {
    log_command("import_group_chat_payload");

    app_state
        .chat_service
        .import_group_chat(dto)
        .await
        .map_err(map_command_error("Failed to import group chat payload"))
}
