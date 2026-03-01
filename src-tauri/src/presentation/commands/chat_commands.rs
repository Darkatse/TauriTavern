use std::sync::Arc;

use tauri::State;
use tauri::ipc::Response as InvokeResponse;

use crate::app::AppState;
use crate::application::dto::chat_dto::{
    AddMessageDto, ChatDto, ChatSearchResultDto, CreateChatDto, DeleteGroupChatDto, ExportChatDto,
    ImportCharacterChatsDto, ImportChatDto, ImportGroupChatDto,
    PinnedCharacterChatDto, PinnedGroupChatDto, RenameChatDto, RenameGroupChatDto,
    SaveChatFromFileDto, SaveGroupChatFromFileDto,
};
use crate::application::errors::ApplicationError;
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::errors::CommandError;

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
pub async fn list_chat_summaries(
    character_filter: Option<String>,
    include_metadata: Option<bool>,
    app_state: State<'_, Arc<AppState>>,
) -> Result<Vec<ChatSearchResultDto>, CommandError> {
    log_command("list_chat_summaries");

    app_state
        .chat_service
        .list_chat_summaries(
            character_filter.as_deref(),
            include_metadata.unwrap_or(false),
        )
        .await
        .map_err(map_command_error("Failed to list chat summaries"))
}

#[tauri::command]
pub async fn list_group_chat_summaries(
    chat_ids: Option<Vec<String>>,
    include_metadata: Option<bool>,
    app_state: State<'_, Arc<AppState>>,
) -> Result<Vec<ChatSearchResultDto>, CommandError> {
    log_command("list_group_chat_summaries");

    app_state
        .chat_service
        .list_group_chat_summaries(chat_ids.as_deref(), include_metadata.unwrap_or(false))
        .await
        .map_err(map_command_error("Failed to list group chat summaries"))
}

#[tauri::command]
pub async fn list_recent_chat_summaries(
    character_filter: Option<String>,
    include_metadata: Option<bool>,
    max_entries: Option<usize>,
    pinned: Option<Vec<PinnedCharacterChatDto>>,
    app_state: State<'_, Arc<AppState>>,
) -> Result<Vec<ChatSearchResultDto>, CommandError> {
    log_command("list_recent_chat_summaries");
    let pinned = pinned.unwrap_or_default();
    let pinned_refs = pinned.into_iter().map(Into::into).collect::<Vec<_>>();

    app_state
        .chat_service
        .list_recent_chat_summaries(
            character_filter.as_deref(),
            include_metadata.unwrap_or(false),
            max_entries.unwrap_or(usize::MAX),
            &pinned_refs,
        )
        .await
        .map_err(map_command_error("Failed to list recent chat summaries"))
}

#[tauri::command]
pub async fn list_recent_group_chat_summaries(
    chat_ids: Option<Vec<String>>,
    include_metadata: Option<bool>,
    max_entries: Option<usize>,
    pinned: Option<Vec<PinnedGroupChatDto>>,
    app_state: State<'_, Arc<AppState>>,
) -> Result<Vec<ChatSearchResultDto>, CommandError> {
    log_command("list_recent_group_chat_summaries");
    let pinned = pinned.unwrap_or_default();
    let pinned_refs = pinned.into_iter().map(Into::into).collect::<Vec<_>>();

    app_state
        .chat_service
        .list_recent_group_chat_summaries(
            chat_ids.as_deref(),
            include_metadata.unwrap_or(false),
            max_entries.unwrap_or(usize::MAX),
            &pinned_refs,
        )
        .await
        .map_err(map_command_error(
            "Failed to list recent group chat summaries",
        ))
}

#[tauri::command]
pub async fn search_group_chats(
    query: String,
    chat_ids: Option<Vec<String>>,
    app_state: State<'_, Arc<AppState>>,
) -> Result<Vec<ChatSearchResultDto>, CommandError> {
    log_command(format!("search_group_chats {}", query));

    app_state
        .chat_service
        .search_group_chats(&query, chat_ids.as_deref())
        .await
        .map_err(map_command_error("Failed to search group chats"))
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
pub async fn list_chat_backups(
    app_state: State<'_, Arc<AppState>>,
) -> Result<Vec<ChatSearchResultDto>, CommandError> {
    log_command("list_chat_backups");

    app_state
        .chat_service
        .list_chat_backups()
        .await
        .map_err(map_command_error("Failed to list chat backups"))
}

#[tauri::command]
pub async fn get_chat_backup_raw(
    name: String,
    app_state: State<'_, Arc<AppState>>,
) -> Result<InvokeResponse, CommandError> {
    log_command(format!("get_chat_backup_raw {}", name));

    app_state
        .chat_service
        .get_chat_backup_bytes(&name)
        .await
        .map(InvokeResponse::new)
        .map_err(map_command_error("Failed to get chat backup content"))
}

#[tauri::command]
pub async fn delete_chat_backup(
    name: String,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command(format!("delete_chat_backup {}", name));

    app_state
        .chat_service
        .delete_chat_backup(&name)
        .await
        .map_err(map_command_error("Failed to delete chat backup"))
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
pub async fn get_chat_payload_path(
    character_name: String,
    file_name: String,
    allow_not_found: Option<bool>,
    app_state: State<'_, Arc<AppState>>,
) -> Result<String, CommandError> {
    log_command(format!("get_chat_payload_path {}/{}", character_name, file_name));

    let allow_not_found = allow_not_found.unwrap_or(false);
    match app_state
        .chat_service
        .get_chat_payload_path(&character_name, &file_name)
        .await
    {
        Ok(path) => Ok(path),
        Err(ApplicationError::NotFound(_)) if allow_not_found => Ok(String::new()),
        Err(error) => Err(map_command_error(format!(
            "Failed to get chat payload path {}/{}",
            character_name, file_name
        ))(error)),
    }
}

#[tauri::command]
pub async fn save_chat_payload_from_file(
    dto: SaveChatFromFileDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command(format!(
        "save_chat_payload_from_file {}/{}",
        dto.character_name, dto.file_name
    ));

    app_state
        .chat_service
        .save_chat_from_file(dto)
        .await
        .map_err(map_command_error("Failed to save chat payload from file"))
}

#[tauri::command]
pub async fn get_group_chat_path(
    id: String,
    allow_not_found: Option<bool>,
    app_state: State<'_, Arc<AppState>>,
) -> Result<String, CommandError> {
    log_command(format!("get_group_chat_path {}", id));

    let allow_not_found = allow_not_found.unwrap_or(false);
    match app_state.chat_service.get_group_chat_payload_path(&id).await {
        Ok(path) => Ok(path),
        Err(ApplicationError::NotFound(_)) if allow_not_found => Ok(String::new()),
        Err(error) => Err(map_command_error(format!(
            "Failed to get group chat payload path {}",
            id
        ))(error)),
    }
}

#[tauri::command]
pub async fn save_group_chat_from_file(
    dto: SaveGroupChatFromFileDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command(format!("save_group_chat_from_file {}", dto.id));

    app_state
        .chat_service
        .save_group_chat_from_file(dto)
        .await
        .map_err(map_command_error(
            "Failed to save group chat payload from file",
        ))
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
