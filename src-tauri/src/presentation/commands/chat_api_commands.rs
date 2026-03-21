use std::sync::Arc;

use serde_json::Value;
use tauri::State;

use crate::app::AppState;
use crate::application::dto::chat_dto::ChatSearchResultDto;
use crate::domain::repositories::chat_repository::{
    ChatMessageSearchHit, ChatMessageSearchQuery, FindLastMessageQuery, LocatedChatMessage,
};
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::errors::CommandError;

#[tauri::command]
pub async fn get_character_chat_summary(
    character_name: String,
    file_name: String,
    include_metadata: Option<bool>,
    app_state: State<'_, Arc<AppState>>,
) -> Result<ChatSearchResultDto, CommandError> {
    log_command(format!(
        "get_character_chat_summary {}/{}",
        character_name, file_name
    ));

    app_state
        .chat_service
        .get_character_chat_summary(
            &character_name,
            &file_name,
            include_metadata.unwrap_or(false),
        )
        .await
        .map_err(map_command_error(format!(
            "Failed to get chat summary {}/{}",
            character_name, file_name
        )))
}

#[tauri::command]
pub async fn get_group_chat_summary(
    chat_id: String,
    include_metadata: Option<bool>,
    app_state: State<'_, Arc<AppState>>,
) -> Result<ChatSearchResultDto, CommandError> {
    log_command(format!("get_group_chat_summary {}", chat_id));

    app_state
        .chat_service
        .get_group_chat_summary(&chat_id, include_metadata.unwrap_or(false))
        .await
        .map_err(map_command_error(format!(
            "Failed to get group chat summary {}",
            chat_id
        )))
}

#[tauri::command]
pub async fn get_character_chat_metadata(
    character_name: String,
    file_name: String,
    app_state: State<'_, Arc<AppState>>,
) -> Result<Value, CommandError> {
    log_command(format!(
        "get_character_chat_metadata {}/{}",
        character_name, file_name
    ));

    app_state
        .chat_service
        .get_character_chat_metadata(&character_name, &file_name)
        .await
        .map_err(map_command_error(format!(
            "Failed to get chat metadata {}/{}",
            character_name, file_name
        )))
}

#[tauri::command]
pub async fn get_group_chat_metadata(
    chat_id: String,
    app_state: State<'_, Arc<AppState>>,
) -> Result<Value, CommandError> {
    log_command(format!("get_group_chat_metadata {}", chat_id));

    app_state
        .chat_service
        .get_group_chat_metadata(&chat_id)
        .await
        .map_err(map_command_error(format!(
            "Failed to get group chat metadata {}",
            chat_id
        )))
}

#[tauri::command]
pub async fn set_character_chat_metadata_extension(
    character_name: String,
    file_name: String,
    namespace: String,
    value: Value,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command(format!(
        "set_character_chat_metadata_extension {}/{}:{}",
        character_name, file_name, namespace
    ));

    app_state
        .chat_service
        .set_character_chat_metadata_extension(&character_name, &file_name, &namespace, value)
        .await
        .map_err(map_command_error(format!(
            "Failed to set chat metadata extension {}/{}:{}",
            character_name, file_name, namespace
        )))
}

#[tauri::command]
pub async fn set_group_chat_metadata_extension(
    chat_id: String,
    namespace: String,
    value: Value,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command(format!(
        "set_group_chat_metadata_extension {}:{}",
        chat_id, namespace
    ));

    app_state
        .chat_service
        .set_group_chat_metadata_extension(&chat_id, &namespace, value)
        .await
        .map_err(map_command_error(format!(
            "Failed to set group chat metadata extension {}:{}",
            chat_id, namespace
        )))
}

#[tauri::command]
pub async fn get_character_chat_store_json(
    character_name: String,
    file_name: String,
    namespace: String,
    key: String,
    app_state: State<'_, Arc<AppState>>,
) -> Result<Value, CommandError> {
    log_command(format!(
        "get_character_chat_store_json {}/{}:{}/{}",
        character_name, file_name, namespace, key
    ));

    app_state
        .chat_service
        .get_character_chat_store_json(&character_name, &file_name, &namespace, &key)
        .await
        .map_err(map_command_error(format!(
            "Failed to get chat store json {}/{}:{}/{}",
            character_name, file_name, namespace, key
        )))
}

#[tauri::command]
pub async fn get_group_chat_store_json(
    chat_id: String,
    namespace: String,
    key: String,
    app_state: State<'_, Arc<AppState>>,
) -> Result<Value, CommandError> {
    log_command(format!(
        "get_group_chat_store_json {}:{}/{}",
        chat_id, namespace, key
    ));

    app_state
        .chat_service
        .get_group_chat_store_json(&chat_id, &namespace, &key)
        .await
        .map_err(map_command_error(format!(
            "Failed to get group chat store json {}:{}/{}",
            chat_id, namespace, key
        )))
}

#[tauri::command]
pub async fn set_character_chat_store_json(
    character_name: String,
    file_name: String,
    namespace: String,
    key: String,
    value: Value,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command(format!(
        "set_character_chat_store_json {}/{}:{}/{}",
        character_name, file_name, namespace, key
    ));

    app_state
        .chat_service
        .set_character_chat_store_json(&character_name, &file_name, &namespace, &key, value)
        .await
        .map_err(map_command_error(format!(
            "Failed to set chat store json {}/{}:{}/{}",
            character_name, file_name, namespace, key
        )))
}

#[tauri::command]
pub async fn set_group_chat_store_json(
    chat_id: String,
    namespace: String,
    key: String,
    value: Value,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command(format!(
        "set_group_chat_store_json {}:{}/{}",
        chat_id, namespace, key
    ));

    app_state
        .chat_service
        .set_group_chat_store_json(&chat_id, &namespace, &key, value)
        .await
        .map_err(map_command_error(format!(
            "Failed to set group chat store json {}:{}/{}",
            chat_id, namespace, key
        )))
}

#[tauri::command]
pub async fn delete_character_chat_store_json(
    character_name: String,
    file_name: String,
    namespace: String,
    key: String,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command(format!(
        "delete_character_chat_store_json {}/{}:{}/{}",
        character_name, file_name, namespace, key
    ));

    app_state
        .chat_service
        .delete_character_chat_store_json(&character_name, &file_name, &namespace, &key)
        .await
        .map_err(map_command_error(format!(
            "Failed to delete chat store json {}/{}:{}/{}",
            character_name, file_name, namespace, key
        )))
}

#[tauri::command]
pub async fn delete_group_chat_store_json(
    chat_id: String,
    namespace: String,
    key: String,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command(format!(
        "delete_group_chat_store_json {}:{}/{}",
        chat_id, namespace, key
    ));

    app_state
        .chat_service
        .delete_group_chat_store_json(&chat_id, &namespace, &key)
        .await
        .map_err(map_command_error(format!(
            "Failed to delete group chat store json {}:{}/{}",
            chat_id, namespace, key
        )))
}

#[tauri::command]
pub async fn list_character_chat_store_keys(
    character_name: String,
    file_name: String,
    namespace: String,
    app_state: State<'_, Arc<AppState>>,
) -> Result<Vec<String>, CommandError> {
    log_command(format!(
        "list_character_chat_store_keys {}/{}:{}",
        character_name, file_name, namespace
    ));

    app_state
        .chat_service
        .list_character_chat_store_keys(&character_name, &file_name, &namespace)
        .await
        .map_err(map_command_error(format!(
            "Failed to list chat store keys {}/{}:{}",
            character_name, file_name, namespace
        )))
}

#[tauri::command]
pub async fn list_group_chat_store_keys(
    chat_id: String,
    namespace: String,
    app_state: State<'_, Arc<AppState>>,
) -> Result<Vec<String>, CommandError> {
    log_command(format!(
        "list_group_chat_store_keys {}:{}",
        chat_id, namespace
    ));

    app_state
        .chat_service
        .list_group_chat_store_keys(&chat_id, &namespace)
        .await
        .map_err(map_command_error(format!(
            "Failed to list group chat store keys {}:{}",
            chat_id, namespace
        )))
}

#[tauri::command]
pub async fn find_last_character_chat_message(
    character_name: String,
    file_name: String,
    query: FindLastMessageQuery,
    app_state: State<'_, Arc<AppState>>,
) -> Result<Option<LocatedChatMessage>, CommandError> {
    log_command(format!(
        "find_last_character_chat_message {}/{}",
        character_name, file_name
    ));

    app_state
        .chat_service
        .find_last_character_chat_message(&character_name, &file_name, query)
        .await
        .map_err(map_command_error(format!(
            "Failed to locate last chat message {}/{}",
            character_name, file_name
        )))
}

#[tauri::command]
pub async fn find_last_group_chat_message(
    chat_id: String,
    query: FindLastMessageQuery,
    app_state: State<'_, Arc<AppState>>,
) -> Result<Option<LocatedChatMessage>, CommandError> {
    log_command(format!("find_last_group_chat_message {}", chat_id));

    app_state
        .chat_service
        .find_last_group_chat_message(&chat_id, query)
        .await
        .map_err(map_command_error(format!(
            "Failed to locate last group chat message {}",
            chat_id
        )))
}

#[tauri::command]
pub async fn search_character_chat_messages(
    character_name: String,
    file_name: String,
    query: ChatMessageSearchQuery,
    app_state: State<'_, Arc<AppState>>,
) -> Result<Vec<ChatMessageSearchHit>, CommandError> {
    log_command(format!(
        "search_character_chat_messages {}/{}",
        character_name, file_name
    ));

    app_state
        .chat_service
        .search_character_chat_messages(&character_name, &file_name, query)
        .await
        .map_err(map_command_error(format!(
            "Failed to search chat messages {}/{}",
            character_name, file_name
        )))
}

#[tauri::command]
pub async fn search_group_chat_messages(
    chat_id: String,
    query: ChatMessageSearchQuery,
    app_state: State<'_, Arc<AppState>>,
) -> Result<Vec<ChatMessageSearchHit>, CommandError> {
    log_command(format!("search_group_chat_messages {}", chat_id));

    app_state
        .chat_service
        .search_group_chat_messages(&chat_id, query)
        .await
        .map_err(map_command_error(format!(
            "Failed to search group chat messages {}",
            chat_id
        )))
}
