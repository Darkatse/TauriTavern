use std::sync::Arc;

use tauri::State;

use crate::app::AppState;
use crate::application::dto::chat_dto::{
    ChatSearchResultDto, DeleteGroupChatDto, ImportGroupChatDto, PatchGroupChatWindowedDto,
    PinnedGroupChatDto, RenameGroupChatDto, SaveGroupChatFromFileDto, SaveGroupChatWindowedDto,
};
use crate::application::errors::ApplicationError;
use crate::domain::repositories::chat_types::{
    ChatPayloadChunk, ChatPayloadCursor, ChatPayloadTail,
};
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::errors::CommandError;

#[tauri::command]
pub async fn list_group_chat_summaries(
    chat_ids: Option<Vec<String>>,
    include_metadata: Option<bool>,
    app_state: State<'_, Arc<AppState>>,
) -> Result<Vec<ChatSearchResultDto>, CommandError> {
    log_command("list_group_chat_summaries");

    app_state
        .group_chat_service
        .list_group_chat_summaries(chat_ids.as_deref(), include_metadata.unwrap_or(false))
        .await
        .map_err(map_command_error("Failed to list group chat summaries"))
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
        .group_chat_service
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
        .group_chat_service
        .search_group_chats(&query, chat_ids.as_deref())
        .await
        .map_err(map_command_error("Failed to search group chats"))
}

#[tauri::command]
pub async fn get_group_chat_path(
    id: String,
    allow_not_found: Option<bool>,
    app_state: State<'_, Arc<AppState>>,
) -> Result<String, CommandError> {
    log_command(format!("get_group_chat_path {}", id));

    let allow_not_found = allow_not_found.unwrap_or(false);
    match app_state
        .group_chat_service
        .get_group_chat_payload_path(&id)
        .await
    {
        Ok(path) => Ok(path),
        Err(ApplicationError::NotFound(_)) if allow_not_found => Ok(String::new()),
        Err(error) => Err(map_command_error(format!(
            "Failed to get group chat payload path {}",
            id
        ))(error)),
    }
}

#[tauri::command]
pub async fn get_group_chat_payload_tail(
    id: String,
    max_lines: usize,
    allow_not_found: Option<bool>,
    app_state: State<'_, Arc<AppState>>,
) -> Result<ChatPayloadTail, CommandError> {
    log_command(format!("get_group_chat_payload_tail {}", id));

    let allow_not_found = allow_not_found.unwrap_or(false);
    match app_state
        .group_chat_service
        .get_group_chat_payload_tail_lines(&id, max_lines)
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
            "Failed to get group chat payload tail {}",
            id
        ))(error)),
    }
}

#[tauri::command]
pub async fn get_group_chat_payload_before(
    id: String,
    cursor: ChatPayloadCursor,
    max_lines: usize,
    app_state: State<'_, Arc<AppState>>,
) -> Result<ChatPayloadChunk, CommandError> {
    log_command(format!("get_group_chat_payload_before {}", id));

    app_state
        .group_chat_service
        .get_group_chat_payload_before_lines(&id, cursor, max_lines)
        .await
        .map_err(map_command_error(format!(
            "Failed to get group chat payload before {}",
            id
        )))
}

#[tauri::command]
pub async fn get_group_chat_payload_before_pages(
    id: String,
    cursor: ChatPayloadCursor,
    max_lines: usize,
    max_pages: usize,
    app_state: State<'_, Arc<AppState>>,
) -> Result<Vec<ChatPayloadChunk>, CommandError> {
    log_command(format!("get_group_chat_payload_before_pages {}", id));

    app_state
        .group_chat_service
        .get_group_chat_payload_before_pages_lines(&id, cursor, max_lines, max_pages)
        .await
        .map_err(map_command_error(format!(
            "Failed to get group chat payload before pages {}",
            id
        )))
}

#[tauri::command]
pub async fn save_group_chat_payload_windowed(
    dto: SaveGroupChatWindowedDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<ChatPayloadCursor, CommandError> {
    log_command(format!("save_group_chat_payload_windowed {}", dto.id));

    app_state
        .group_chat_service
        .save_group_chat_payload_windowed(
            &dto.id,
            dto.cursor,
            dto.header,
            dto.lines,
            dto.force.unwrap_or(false),
        )
        .await
        .map_err(map_command_error(
            "Failed to save windowed group chat payload",
        ))
}

#[tauri::command]
pub async fn patch_group_chat_payload_windowed(
    dto: PatchGroupChatWindowedDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<ChatPayloadCursor, CommandError> {
    log_command(format!("patch_group_chat_payload_windowed {}", dto.id));

    app_state
        .group_chat_service
        .patch_group_chat_payload_windowed(
            &dto.id,
            dto.cursor,
            dto.header,
            dto.patch,
            dto.force.unwrap_or(false),
        )
        .await
        .map_err(map_command_error(
            "Failed to patch windowed group chat payload",
        ))
}

#[tauri::command]
pub async fn save_group_chat_from_file(
    dto: SaveGroupChatFromFileDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command(format!("save_group_chat_from_file {}", dto.id));

    app_state
        .group_chat_service
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
        .group_chat_service
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
        .group_chat_service
        .rename_group_chat(dto)
        .await
        .map_err(map_command_error("Failed to rename group chat payload"))
}

#[tauri::command]
pub async fn import_group_chat_payload(
    dto: ImportGroupChatDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<String, CommandError> {
    log_command("import_group_chat_payload");

    app_state
        .group_chat_service
        .import_group_chat(dto)
        .await
        .map_err(map_command_error("Failed to import group chat payload"))
}
