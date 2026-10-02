use std::sync::Arc;

use serde::Serialize;
use tauri::{Manager, ResourceId, State, Webview};
use tt_contracts::byte_commit::CommitBegin;
use tt_contracts::extension_store::{EntryKind, WriteOperation};

use super::byte_reader_commands::ByteResource;
use super::chunk_body::{chunk_bytes_from_request, commit_headers};
use super::helpers::{log_command, map_command_error};
use crate::app::AppState;
use crate::presentation::errors::CommandError;

const READ_CHUNK_BYTES: usize = 512 * 1024;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenEntryResponse {
    reader_id: ResourceId,
    mime_type: String,
}

#[tauri::command]
pub async fn open_extension_store_entry(
    namespace: String,
    key: String,
    table: Option<String>,
    kind: EntryKind,
    webview: Webview,
    app_state: State<'_, Arc<AppState>>,
) -> Result<Option<OpenEntryResponse>, CommandError> {
    log_command(format!(
        "open_extension_store_entry {}:{}/{} ({kind:?})",
        namespace,
        table.as_deref().unwrap_or("main"),
        key,
    ));
    let Some(reader) = app_state
        .services
        .extension_store_service
        .open_entry(&namespace, table.as_deref(), &key, kind)
        .await
        .map_err(map_command_error("Failed to open extension store entry"))?
    else {
        return Ok(None);
    };
    let mime_type = match kind {
        EntryKind::Json => "application/json".to_string(),
        EntryKind::Blob => mime_guess::from_path(&key)
            .first_or_octet_stream()
            .essence_str()
            .to_string(),
    };
    let reader_id = webview
        .resources_table()
        .add(ByteResource::new(reader, READ_CHUNK_BYTES));
    Ok(Some(OpenEntryResponse {
        reader_id,
        mime_type,
    }))
}

#[tauri::command]
pub async fn begin_extension_store_commit(
    namespace: String,
    key: String,
    table: Option<String>,
    operation: WriteOperation,
    app_state: State<'_, Arc<AppState>>,
) -> Result<CommitBegin, CommandError> {
    app_state
        .services
        .extension_store_service
        .begin_commit(&namespace, table.as_deref(), &key, operation)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn append_extension_store_commit_chunk(
    request: tauri::ipc::Request<'_>,
    app_state: State<'_, Arc<AppState>>,
) -> Result<u64, CommandError> {
    let (session_id, offset) = commit_headers(&request)?;
    let bytes = chunk_bytes_from_request(&request)?;
    app_state
        .services
        .extension_store_service
        .append_commit(session_id, offset, &bytes)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn finish_extension_store_commit(
    session_id: String,
    expected_size: u64,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    app_state
        .services
        .extension_store_service
        .finish_commit(&session_id, expected_size)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn abort_extension_store_commit(
    session_id: String,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    app_state
        .services
        .extension_store_service
        .abort_commit(&session_id)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn rename_extension_store_key(
    namespace: String,
    key: String,
    new_key: String,
    table: Option<String>,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command(format!(
        "rename_extension_store_key {}:{}/{} -> {}",
        namespace,
        table.as_deref().unwrap_or("main"),
        key,
        new_key,
    ));
    app_state
        .services
        .extension_store_service
        .rename_json_key(&namespace, table.as_deref(), &key, &new_key)
        .await
        .map_err(map_command_error(
            "Failed to rename extension store JSON key",
        ))
}

#[tauri::command]
pub async fn delete_extension_store_entry(
    namespace: String,
    key: String,
    table: Option<String>,
    kind: EntryKind,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command(format!(
        "delete_extension_store_entry {}:{}/{} ({kind:?})",
        namespace,
        table.as_deref().unwrap_or("main"),
        key,
    ));
    app_state
        .services
        .extension_store_service
        .delete_entry(&namespace, table.as_deref(), &key, kind)
        .await
        .map_err(map_command_error("Failed to delete extension store entry"))
}

#[tauri::command]
pub async fn list_extension_store_keys(
    namespace: String,
    table: Option<String>,
    kind: EntryKind,
    app_state: State<'_, Arc<AppState>>,
) -> Result<Vec<String>, CommandError> {
    log_command(format!(
        "list_extension_store_keys {}:{} ({kind:?})",
        namespace,
        table.as_deref().unwrap_or("main"),
    ));
    app_state
        .services
        .extension_store_service
        .list_keys(&namespace, table.as_deref(), kind)
        .await
        .map_err(map_command_error("Failed to list extension store keys"))
}

#[tauri::command]
pub async fn list_extension_store_tables(
    namespace: String,
    app_state: State<'_, Arc<AppState>>,
) -> Result<Vec<String>, CommandError> {
    log_command(format!("list_extension_store_tables {namespace}"));
    app_state
        .services
        .extension_store_service
        .list_tables(&namespace)
        .await
        .map_err(map_command_error("Failed to list extension store tables"))
}

#[tauri::command]
pub async fn delete_extension_store_table(
    namespace: String,
    table: String,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command(format!("delete_extension_store_table {namespace}:{table}"));
    app_state
        .services
        .extension_store_service
        .delete_table(&namespace, &table)
        .await
        .map_err(map_command_error("Failed to delete extension store table"))
}
