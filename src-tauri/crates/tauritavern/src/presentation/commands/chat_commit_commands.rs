use std::sync::Arc;

use serde::Deserialize;
use tauri::{ResourceId, State, Webview};

use crate::app::AppState;
use crate::presentation::commands::chunk_body::{chunk_bytes_from_request, commit_headers};
use crate::presentation::errors::CommandError;
use tt_application::dto::chat_history_dto::{ChatHistoryLocator, CurrentCommitReason};
use tt_contracts::byte_commit::CommitBegin;
use tt_ports::repositories::chat_commit_repository::ChatCommitOperation;

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
) -> Result<CommitBegin, CommandError> {
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
    app_state
        .services
        .chat_commit_service
        .begin(target, operation)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn append_chat_commit_chunk(
    request: tauri::ipc::Request<'_>,
    app_state: State<'_, Arc<AppState>>,
) -> Result<u64, CommandError> {
    let (session_id, offset) = commit_headers(&request)?;
    let bytes = chunk_bytes_from_request(&request)?;

    app_state
        .services
        .chat_commit_service
        .append(session_id, offset, &bytes)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn finish_chat_commit(
    session_id: String,
    expected_size: u64,
    commit_reason: CurrentCommitReason,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    app_state
        .services
        .chat_commit_service
        .finish(&session_id, expected_size, commit_reason)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn abort_chat_commit(
    session_id: String,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    app_state
        .services
        .chat_commit_service
        .abort(&session_id)
        .await
        .map_err(Into::into)
}
