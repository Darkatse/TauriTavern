use std::sync::Arc;

use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter, State};

use crate::app::AppState;
use crate::application::dto::chat_completion_dto::{
    ChatCompletionGenerateRequestDto, ChatCompletionStatusRequestDto,
};
use crate::application::services::chat_completion_service::ChatCompletionService;
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::errors::CommandError;

#[tauri::command]
pub async fn get_chat_completions_status(
    dto: ChatCompletionStatusRequestDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<Value, CommandError> {
    log_command("get_chat_completions_status");

    app_state
        .chat_completion_service
        .get_status(dto)
        .await
        .map_err(map_command_error("Failed to get chat completions status"))
}

#[tauri::command]
pub async fn generate_chat_completion(
    dto: ChatCompletionGenerateRequestDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<Value, CommandError> {
    log_command("generate_chat_completion");

    app_state
        .chat_completion_service
        .generate(dto)
        .await
        .map_err(map_command_error("Failed to generate chat completion"))
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ChatCompletionStreamEvent {
    Chunk { data: String },
    Done,
    Error { message: String },
}

#[tauri::command]
pub async fn start_chat_completion_stream(
    stream_id: String,
    dto: ChatCompletionGenerateRequestDto,
    app_handle: AppHandle,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    validate_stream_id(&stream_id)?;
    log_command(format!("start_chat_completion_stream {}", stream_id));

    let service = app_state.chat_completion_service.clone();
    let cancel = service.register_stream(&stream_id).await;

    tauri::async_runtime::spawn(run_stream_generation(
        app_handle, service, stream_id, dto, cancel,
    ));

    Ok(())
}

#[tauri::command]
pub async fn cancel_chat_completion_stream(
    stream_id: String,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    validate_stream_id(&stream_id)?;
    log_command(format!("cancel_chat_completion_stream {}", stream_id));

    app_state
        .chat_completion_service
        .cancel_stream(&stream_id)
        .await;
    Ok(())
}

async fn run_stream_generation(
    app_handle: AppHandle,
    service: Arc<ChatCompletionService>,
    stream_id: String,
    dto: ChatCompletionGenerateRequestDto,
    cancel: tokio::sync::watch::Receiver<bool>,
) {
    let event_name = stream_event_name(&stream_id);
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<String>();
    let generation_task = tauri::async_runtime::spawn({
        let service = service.clone();
        async move { service.generate_stream(dto, sender, cancel).await }
    });

    while let Some(chunk) = receiver.recv().await {
        if chunk.is_empty() {
            continue;
        }

        let emit_result = app_handle.emit(
            &event_name,
            ChatCompletionStreamEvent::Chunk { data: chunk },
        );

        if emit_result.is_err() {
            generation_task.abort();
            service.complete_stream(&stream_id).await;
            return;
        }
    }

    let generation_result = match generation_task.await {
        Ok(result) => result,
        Err(error) => Err(crate::application::errors::ApplicationError::InternalError(
            format!("Streaming task join failed: {error}"),
        )),
    };

    service.complete_stream(&stream_id).await;

    match generation_result {
        Ok(()) => {
            let _ = app_handle.emit(&event_name, ChatCompletionStreamEvent::Done);
        }
        Err(error) => {
            let _ = app_handle.emit(
                &event_name,
                ChatCompletionStreamEvent::Error {
                    message: error.to_string(),
                },
            );
        }
    }
}

fn stream_event_name(stream_id: &str) -> String {
    format!("chat-completion-stream:{stream_id}")
}

fn validate_stream_id(stream_id: &str) -> Result<(), CommandError> {
    let stream_id = stream_id.trim();
    if stream_id.is_empty() || stream_id.len() > 128 {
        return Err(CommandError::BadRequest(
            "Invalid stream id length".to_string(),
        ));
    }

    if !stream_id
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        return Err(CommandError::BadRequest(
            "Invalid stream id characters".to_string(),
        ));
    }

    Ok(())
}
