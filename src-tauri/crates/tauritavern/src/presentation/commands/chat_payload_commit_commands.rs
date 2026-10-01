use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::{ResourceId, State, Webview};

use crate::app::AppState;
use crate::presentation::commands::chunk_body::chunk_bytes_from_request;
use crate::presentation::errors::CommandError;
use tt_application::dto::chat_history_dto::{ChatHistoryLocator, CurrentCommitReason};
use tt_ports::repositories::chat_payload_commit_repository::ChatCommitOperation;

const HEADER_OFFSET: &str = "offset";
const HEADER_SESSION_ID: &str = "session-id";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BeginChatCommitResult {
    session_id: String,
    max_frame_bytes: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FinishChatCommitResult {
    accepted_size: u64,
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

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ChatCommitOperationDto {
    Payload {
        force: bool,
        #[serde(rename = "coldSourceId")]
        cold_source_id: Option<ResourceId>,
    },
    Metadata,
    MetadataExtension {
        namespace: String,
    },
}

#[tauri::command]
pub async fn begin_chat_commit(
    target: ChatHistoryLocator,
    operation: ChatCommitOperationDto,
    webview: Webview,
    app_state: State<'_, Arc<AppState>>,
) -> Result<BeginChatCommitResult, CommandError> {
    let operation = match operation {
        ChatCommitOperationDto::Payload {
            force,
            cold_source_id,
        } => ChatCommitOperation::Payload {
            force,
            cold_source: cold_source_id
                .map(|id| super::chat_swipe_commands::commit_source(&webview, id))
                .transpose()?,
        },
        ChatCommitOperationDto::Metadata => ChatCommitOperation::Metadata,
        ChatCommitOperationDto::MetadataExtension { namespace } => {
            ChatCommitOperation::MetadataExtension { namespace }
        }
    };
    let session = app_state
        .services
        .chat_payload_commit_service
        .begin(target, operation)
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
    let committed = app_state
        .services
        .chat_payload_commit_service
        .finish(&session_id, expected_size, commit_reason)
        .await?;

    Ok(FinishChatCommitResult {
        accepted_size: committed.accepted_size,
        size: committed.size,
    })
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
