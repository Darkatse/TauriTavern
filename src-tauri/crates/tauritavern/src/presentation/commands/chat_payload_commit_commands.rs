use std::sync::Arc;

use serde::Serialize;
use tauri::State;

use crate::app::AppState;
use crate::presentation::commands::chunk_body::chunk_bytes_from_request;
use crate::presentation::errors::CommandError;
use tt_application::dto::chat_history_dto::{ChatHistoryLocator, CurrentCommitReason};

const HEADER_OFFSET: &str = "offset";
const HEADER_SESSION_ID: &str = "session-id";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BeginChatCommitResult {
    session_id: String,
    max_frame_bytes: u64,
}

#[derive(Debug, Serialize)]
pub struct FinishChatCommitResult {
    size: u64,
}

fn required_header(request: &tauri::ipc::Request<'_>, name: &str) -> Result<String, CommandError> {
    request
        .headers()
        .get(name)
        .ok_or_else(|| CommandError::BadRequest(format!("Missing chat commit header: {name}")))?
        .to_str()
        .map(str::to_string)
        .map_err(|_| CommandError::BadRequest(format!("Invalid chat commit header: {name}")))
}

#[tauri::command]
pub async fn begin_chat_commit(
    target: ChatHistoryLocator,
    force: bool,
    app_state: State<'_, Arc<AppState>>,
) -> Result<BeginChatCommitResult, CommandError> {
    let session = app_state
        .services
        .chat_payload_commit_service
        .begin(target, force)
        .await?;

    Ok(BeginChatCommitResult {
        session_id: session.session_id,
        max_frame_bytes: session.max_frame_bytes,
    })
}

#[tauri::command]
pub async fn append_chat_commit_chunk(
    request: tauri::ipc::Request<'_>,
    app_state: State<'_, Arc<AppState>>,
) -> Result<u64, CommandError> {
    let session_id = required_header(&request, HEADER_SESSION_ID)?;
    let offset = required_header(&request, HEADER_OFFSET)?
        .parse::<u64>()
        .map_err(|_| CommandError::BadRequest("Chat commit offset is invalid".to_string()))?;
    let bytes = chunk_bytes_from_request(&request)?;

    app_state
        .services
        .chat_payload_commit_service
        .append(&session_id, offset, &bytes)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn finish_chat_commit(
    session_id: String,
    expected_size: u64,
    commit_reason: CurrentCommitReason,
    app_state: State<'_, Arc<AppState>>,
) -> Result<FinishChatCommitResult, CommandError> {
    let size = app_state
        .services
        .chat_payload_commit_service
        .finish(&session_id, expected_size, commit_reason)
        .await?;

    Ok(FinishChatCommitResult { size })
}

#[tauri::command]
pub async fn abort_chat_commit(
    session_id: String,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    app_state
        .services
        .chat_payload_commit_service
        .abort(&session_id)
        .await
        .map_err(Into::into)
}
