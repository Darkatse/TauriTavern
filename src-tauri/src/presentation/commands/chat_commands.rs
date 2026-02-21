use std::sync::Arc;

use tauri::http::HeaderMap;
use tauri::ipc::{InvokeBody, Request as InvokeRequest, Response as InvokeResponse};
use tauri::State;

use crate::app::AppState;
use crate::application::dto::chat_dto::{
    AddMessageDto, ChatDto, ChatSearchResultDto, CreateChatDto, DeleteGroupChatDto, ExportChatDto,
    GetGroupChatDto, ImportCharacterChatsDto, ImportChatDto, ImportGroupChatDto, RenameChatDto,
    RenameGroupChatDto, SaveChatDto, SaveChatFromFileDto, SaveGroupChatDto,
    SaveGroupChatFromFileDto,
};
use crate::application::errors::ApplicationError;
use crate::infrastructure::logging::logger;
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::errors::CommandError;
use serde_json::Value;

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn maybe_percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    if !bytes.contains(&b'%') {
        return value.to_string();
    }

    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return value.to_string();
            }

            let hi = hex_value(bytes[index + 1]);
            let lo = hex_value(bytes[index + 2]);
            let (Some(hi), Some(lo)) = (hi, lo) else {
                return value.to_string();
            };

            decoded.push((hi << 4) | lo);
            index += 3;
            continue;
        }

        decoded.push(bytes[index]);
        index += 1;
    }

    String::from_utf8(decoded).unwrap_or_else(|_| value.to_string())
}

fn header_required(headers: &HeaderMap, key: &str) -> Result<String, CommandError> {
    headers
        .get(key)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(maybe_percent_decode)
        .ok_or_else(|| CommandError::BadRequest(format!("Missing header: {}", key)))
}

fn header_bool(headers: &HeaderMap, key: &str) -> bool {
    headers
        .get(key)
        .and_then(|value| value.to_str().ok())
        .map(|value| {
            let normalized = value.trim().to_ascii_lowercase();
            normalized == "1" || normalized == "true" || normalized == "yes" || normalized == "on"
        })
        .unwrap_or(false)
}

fn bytes_from_request_body(body: &InvokeBody) -> Result<Vec<u8>, CommandError> {
    match body {
        InvokeBody::Raw(bytes) => Ok(bytes.clone()),
        InvokeBody::Json(Value::Array(values)) => values
            .iter()
            .map(|value| {
                value
                    .as_u64()
                    .filter(|number| *number <= u8::MAX as u64)
                    .map(|number| number as u8)
                    .ok_or_else(|| {
                        CommandError::BadRequest(
                            "Invalid raw byte payload; expected an array of numbers".to_string(),
                        )
                    })
            })
            .collect(),
        InvokeBody::Json(Value::String(text)) => Ok(text.as_bytes().to_vec()),
        _ => Err(CommandError::BadRequest(
            "Invalid raw payload body".to_string(),
        )),
    }
}

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
    allow_not_found: Option<bool>,
    app_state: State<'_, Arc<AppState>>,
) -> Result<Vec<Value>, CommandError> {
    log_command(format!("get_chat_payload {}/{}", character_name, file_name));

    match app_state
        .chat_service
        .get_chat_payload(&character_name, &file_name)
        .await
    {
        Ok(payload) => Ok(payload),
        Err(ApplicationError::NotFound(_)) if allow_not_found.unwrap_or(false) => Ok(Vec::new()),
        Err(error) => {
            let context = format!(
                "Failed to get chat payload {}/{}",
                character_name, file_name
            );
            logger::error(&format!("{}: {}", context, error));
            Err(error.into())
        }
    }
}

#[tauri::command]
pub async fn get_chat_payload_raw(
    character_name: String,
    file_name: String,
    allow_not_found: Option<bool>,
    app_state: State<'_, Arc<AppState>>,
) -> Result<InvokeResponse, CommandError> {
    log_command(format!(
        "get_chat_payload_raw {}/{}",
        character_name, file_name
    ));

    match app_state
        .chat_service
        .get_chat_payload_bytes(&character_name, &file_name)
        .await
    {
        Ok(payload) => Ok(InvokeResponse::new(payload)),
        Err(ApplicationError::NotFound(_)) if allow_not_found.unwrap_or(false) => {
            Ok(InvokeResponse::new(Vec::<u8>::new()))
        }
        Err(error) => {
            let context = format!(
                "Failed to get raw chat payload {}/{}",
                character_name, file_name
            );
            logger::error(&format!("{}: {}", context, error));
            Err(error.into())
        }
    }
}

#[tauri::command]
pub async fn get_chat_payload_path(
    character_name: String,
    file_name: String,
    app_state: State<'_, Arc<AppState>>,
) -> Result<String, CommandError> {
    log_command(format!(
        "get_chat_payload_path {}/{}",
        character_name, file_name
    ));

    app_state
        .chat_service
        .get_chat_payload_path(&character_name, &file_name)
        .await
        .map_err(map_command_error("Failed to get chat payload path"))
}

#[tauri::command]
pub async fn get_chat_payload_text(
    character_name: String,
    file_name: String,
    allow_not_found: Option<bool>,
    app_state: State<'_, Arc<AppState>>,
) -> Result<String, CommandError> {
    log_command(format!(
        "get_chat_payload_text {}/{}",
        character_name, file_name
    ));

    match app_state
        .chat_service
        .get_chat_payload_text(&character_name, &file_name)
        .await
    {
        Ok(text) => Ok(text),
        Err(ApplicationError::NotFound(_)) if allow_not_found.unwrap_or(false) => Ok(String::new()),
        Err(error) => {
            let context = format!(
                "Failed to get chat payload text {}/{}",
                character_name, file_name
            );
            logger::error(&format!("{}: {}", context, error));
            Err(error.into())
        }
    }
}

#[tauri::command]
pub async fn save_chat_payload_raw(
    request: InvokeRequest<'_>,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command("save_chat_payload_raw");

    let character_name = header_required(request.headers(), "x-character-name")?;
    let file_name = header_required(request.headers(), "x-file-name")?;
    let force = header_bool(request.headers(), "x-force");
    let payload = bytes_from_request_body(request.body())?;

    app_state
        .chat_service
        .save_chat_payload_bytes(&character_name, &file_name, &payload, force)
        .await
        .map_err(map_command_error("Failed to save raw chat payload"))
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
pub async fn get_group_chat(
    dto: GetGroupChatDto,
    allow_not_found: Option<bool>,
    app_state: State<'_, Arc<AppState>>,
) -> Result<Vec<Value>, CommandError> {
    let chat_id = dto.id.clone();
    log_command(format!("get_group_chat {}", chat_id));

    match app_state.chat_service.get_group_chat(dto).await {
        Ok(payload) => Ok(payload),
        Err(ApplicationError::NotFound(_)) if allow_not_found.unwrap_or(false) => Ok(Vec::new()),
        Err(error) => {
            let context = format!("Failed to get group chat payload {}", chat_id);
            logger::error(&format!("{}: {}", context, error));
            Err(error.into())
        }
    }
}

#[tauri::command]
pub async fn get_group_chat_raw(
    dto: GetGroupChatDto,
    allow_not_found: Option<bool>,
    app_state: State<'_, Arc<AppState>>,
) -> Result<InvokeResponse, CommandError> {
    let chat_id = dto.id.clone();
    log_command(format!("get_group_chat_raw {}", chat_id));

    match app_state
        .chat_service
        .get_group_chat_payload_bytes(&chat_id)
        .await
    {
        Ok(payload) => Ok(InvokeResponse::new(payload)),
        Err(ApplicationError::NotFound(_)) if allow_not_found.unwrap_or(false) => {
            Ok(InvokeResponse::new(Vec::<u8>::new()))
        }
        Err(error) => {
            let context = format!("Failed to get raw group chat payload {}", chat_id);
            logger::error(&format!("{}: {}", context, error));
            Err(error.into())
        }
    }
}

#[tauri::command]
pub async fn get_group_chat_path(
    dto: GetGroupChatDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<String, CommandError> {
    log_command(format!("get_group_chat_path {}", dto.id));

    app_state
        .chat_service
        .get_group_chat_payload_path(&dto.id)
        .await
        .map_err(map_command_error("Failed to get group chat payload path"))
}

#[tauri::command]
pub async fn get_group_chat_text(
    dto: GetGroupChatDto,
    allow_not_found: Option<bool>,
    app_state: State<'_, Arc<AppState>>,
) -> Result<String, CommandError> {
    let chat_id = dto.id.clone();
    log_command(format!("get_group_chat_text {}", chat_id));

    match app_state.chat_service.get_group_chat_text(&chat_id).await {
        Ok(text) => Ok(text),
        Err(ApplicationError::NotFound(_)) if allow_not_found.unwrap_or(false) => Ok(String::new()),
        Err(error) => {
            let context = format!("Failed to get group chat payload text {}", chat_id);
            logger::error(&format!("{}: {}", context, error));
            Err(error.into())
        }
    }
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
pub async fn save_group_chat_raw(
    request: InvokeRequest<'_>,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command("save_group_chat_raw");

    let chat_id = header_required(request.headers(), "x-chat-id")?;
    let force = header_bool(request.headers(), "x-force");
    let payload = bytes_from_request_body(request.body())?;

    app_state
        .chat_service
        .save_group_chat_payload_bytes(&chat_id, &payload, force)
        .await
        .map_err(map_command_error("Failed to save raw group chat payload"))
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

#[cfg(test)]
mod tests {
    use super::maybe_percent_decode;

    #[test]
    fn maybe_percent_decode_handles_uri_encoded_utf8() {
        let encoded = "%E8%A7%92%E8%89%B2%201";
        assert_eq!(maybe_percent_decode(encoded), "角色 1");
    }

    #[test]
    fn maybe_percent_decode_keeps_plain_ascii() {
        let value = "alice-session";
        assert_eq!(maybe_percent_decode(value), "alice-session");
    }

    #[test]
    fn maybe_percent_decode_falls_back_on_invalid_sequences() {
        let value = "bad%zzvalue";
        assert_eq!(maybe_percent_decode(value), "bad%zzvalue");
    }
}
