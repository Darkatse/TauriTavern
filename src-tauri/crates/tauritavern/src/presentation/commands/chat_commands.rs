use std::sync::Arc;

use tauri::State;

use crate::app::AppState;
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::errors::CommandError;
use tt_application::dto::chat_dto::{
    AddMessageDto, ChatDto, ChatSearchResultDto, CreateChatDto, ExportChatDto,
    ImportCharacterChatsDto, ImportChatDto, PinnedCharacterChatDto, RenameChatDto,
    RestoreCharacterChatBackupDto,
};
use tt_application::dto::chat_history_dto::ChatHistoryLocator;
use tt_application::errors::ApplicationError;
use tt_ports::repositories::chat_repository::{
    ChatPayloadChunk, ChatPayloadCursor, ChatPayloadTail,
};

#[tauri::command]
pub async fn get_all_chats(
    app_state: State<'_, Arc<AppState>>,
) -> Result<Vec<ChatDto>, CommandError> {
    log_command("get_all_chats");

    app_state
        .services
        .chat_service
        .get_all_chats()
        .await
        .map_err(map_command_error("Failed to get all chats"))
}

#[tauri::command]
pub async fn chat_history_generation_started(
    locator: ChatHistoryLocator,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    app_state
        .services
        .chat_history_coordinator
        .generation_started(locator)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn chat_history_generation_finished(
    locator: ChatHistoryLocator,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    app_state
        .services
        .chat_history_coordinator
        .generation_finished(locator)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn get_chat(
    character_name: String,
    file_name: String,
    app_state: State<'_, Arc<AppState>>,
) -> Result<ChatDto, CommandError> {
    log_command(format!("get_chat {}/{}", character_name, file_name));

    app_state
        .services
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
        .services
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
        .services
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
        .services
        .chat_service
        .add_message(dto)
        .await
        .map_err(map_command_error("Failed to add message to chat"))
}

#[tauri::command]
pub async fn rename_chat(
    dto: RenameChatDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<String, CommandError> {
    log_command(format!(
        "rename_chat {}/{} -> {}/{}",
        dto.character_name, dto.old_file_name, dto.character_name, dto.new_file_name
    ));

    app_state
        .services
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
        .services
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
        .services
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
        .services
        .chat_service
        .list_chat_summaries(
            character_filter.as_deref(),
            include_metadata.unwrap_or(false),
        )
        .await
        .map_err(map_command_error("Failed to list chat summaries"))
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
        .services
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
pub async fn import_chat(
    dto: ImportChatDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<ChatDto, CommandError> {
    log_command(format!(
        "import_chat for character {} from {}",
        dto.character_name, dto.file_path
    ));

    app_state
        .services
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
        .services
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
        .services
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
        .services
        .chat_service
        .list_chat_backups()
        .await
        .map_err(map_command_error("Failed to list chat backups"))
}

#[tauri::command]
pub async fn materialize_chat_backup(
    name: String,
    app_state: State<'_, Arc<AppState>>,
) -> Result<String, CommandError> {
    log_command(format!("materialize_chat_backup {}", name));

    app_state
        .services
        .chat_service
        .materialize_chat_backup(&name)
        .await
        .map_err(map_command_error("Failed to materialize chat backup"))
}

#[tauri::command]
pub async fn discard_chat_backup_materialization(
    path: String,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command("discard_chat_backup_materialization");

    app_state
        .services
        .chat_service
        .discard_chat_backup_materialization(&path)
        .await
        .map_err(map_command_error(
            "Failed to discard chat backup materialization",
        ))
}

#[tauri::command]
pub async fn restore_character_chat_backup(
    dto: RestoreCharacterChatBackupDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<Vec<String>, CommandError> {
    log_command(format!(
        "restore_character_chat_backup {} for {}",
        dto.backup_name, dto.character_name
    ));

    app_state
        .services
        .chat_service
        .restore_character_chat_backup(dto)
        .await
        .map_err(map_command_error("Failed to restore character chat backup"))
}

#[tauri::command]
pub async fn delete_chat_backup(
    name: String,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command(format!("delete_chat_backup {}", name));

    app_state
        .services
        .chat_service
        .delete_chat_backup(&name)
        .await
        .map_err(map_command_error("Failed to delete chat backup"))
}

#[tauri::command]
pub async fn clear_chat_cache(app_state: State<'_, Arc<AppState>>) -> Result<(), CommandError> {
    log_command("clear_chat_cache");

    app_state
        .services
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
    log_command(format!(
        "get_chat_payload_path {}/{}",
        character_name, file_name
    ));

    let allow_not_found = allow_not_found.unwrap_or(false);
    match app_state
        .services
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
pub async fn get_chat_payload_tail(
    character_name: String,
    file_name: String,
    max_lines: usize,
    allow_not_found: Option<bool>,
    app_state: State<'_, Arc<AppState>>,
) -> Result<ChatPayloadTail, CommandError> {
    log_command(format!(
        "get_chat_payload_tail {}/{}",
        character_name, file_name
    ));

    let allow_not_found = allow_not_found.unwrap_or(false);
    match app_state
        .services
        .chat_service
        .get_chat_payload_tail_lines(&character_name, &file_name, max_lines)
        .await
    {
        Ok(result) => Ok(result),
        Err(ApplicationError::NotFound(_)) if allow_not_found => Ok(ChatPayloadTail {
            header: String::new(),
            lines: Vec::new(),
            cursor: ChatPayloadCursor {
                offset: 0,
                size: 0,
                modified_millis: 0,
            },
            has_more_before: false,
        }),
        Err(error) => Err(map_command_error(format!(
            "Failed to get chat payload tail {}/{}",
            character_name, file_name
        ))(error)),
    }
}

#[tauri::command]
pub async fn get_chat_payload_before(
    character_name: String,
    file_name: String,
    cursor: ChatPayloadCursor,
    max_lines: usize,
    app_state: State<'_, Arc<AppState>>,
) -> Result<ChatPayloadChunk, CommandError> {
    log_command(format!(
        "get_chat_payload_before {}/{}",
        character_name, file_name
    ));

    app_state
        .services
        .chat_service
        .get_chat_payload_before_lines(&character_name, &file_name, cursor, max_lines)
        .await
        .map_err(map_command_error(format!(
            "Failed to get chat payload before {}/{}",
            character_name, file_name
        )))
}

#[tauri::command]
pub async fn get_chat_payload_before_pages(
    character_name: String,
    file_name: String,
    cursor: ChatPayloadCursor,
    max_lines: usize,
    max_pages: usize,
    app_state: State<'_, Arc<AppState>>,
) -> Result<Vec<ChatPayloadChunk>, CommandError> {
    log_command(format!(
        "get_chat_payload_before_pages {}/{}",
        character_name, file_name
    ));

    app_state
        .services
        .chat_service
        .get_chat_payload_before_pages_lines(
            &character_name,
            &file_name,
            cursor,
            max_lines,
            max_pages,
        )
        .await
        .map_err(map_command_error(format!(
            "Failed to get chat payload before pages {}/{}",
            character_name, file_name
        )))
}

#[tauri::command]
pub async fn import_character_chats(
    dto: ImportCharacterChatsDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<Vec<String>, CommandError> {
    log_command(format!("import_character_chats {}", dto.character_name));

    app_state
        .services
        .chat_service
        .import_character_chats(dto)
        .await
        .map_err(map_command_error("Failed to import character chats"))
}
