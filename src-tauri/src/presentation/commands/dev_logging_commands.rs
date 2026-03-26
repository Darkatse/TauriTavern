use std::sync::Arc;

use serde::Deserialize;
use tauri::State;

use crate::infrastructure::logging::devtools::{BackendLogEntry, BackendLogStore};
use crate::infrastructure::logging::llm_api_logs::{
    LlmApiLogEntryPreview, LlmApiLogEntryRaw, LlmApiLogIndexEntry, LlmApiLogStore,
};
use crate::presentation::commands::helpers::log_command;
use crate::presentation::errors::CommandError;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrontendLogEntryDto {
    pub level: String,
    pub message: String,
    pub target: Option<String>,
}

#[tauri::command]
pub async fn devlog_append_frontend_logs(
    entries: Vec<FrontendLogEntryDto>,
) -> Result<(), CommandError> {
    log_command("devlog_append_frontend_logs");

    for entry in entries {
        let normalized_level = entry.level.trim().to_ascii_lowercase();
        let message = match entry.target.as_deref() {
            Some(target) => format!("[{target}] {}", entry.message),
            None => entry.message,
        };
        match normalized_level.as_str() {
            "debug" => tracing::debug!(target: "frontend", "{message}"),
            "warn" | "warning" => tracing::warn!(target: "frontend", "{message}"),
            "error" => tracing::error!(target: "frontend", "{message}"),
            _ => tracing::info!(target: "frontend", "{message}"),
        }
    }

    Ok(())
}

#[tauri::command]
pub async fn devlog_set_backend_log_stream_enabled(
    enabled: bool,
    backend_logs: State<'_, Arc<BackendLogStore>>,
) -> Result<(), CommandError> {
    log_command("devlog_set_backend_log_stream_enabled");
    backend_logs.set_stream_enabled(enabled);
    Ok(())
}

#[tauri::command]
pub async fn devlog_get_backend_log_tail(
    limit: Option<u32>,
    backend_logs: State<'_, Arc<BackendLogStore>>,
) -> Result<Vec<BackendLogEntry>, CommandError> {
    log_command("devlog_get_backend_log_tail");

    let limit = limit.unwrap_or(800) as usize;
    Ok(backend_logs.tail(limit))
}

#[tauri::command]
pub async fn devlog_set_llm_api_log_stream_enabled(
    enabled: bool,
    llm_api_logs: State<'_, Arc<LlmApiLogStore>>,
) -> Result<(), CommandError> {
    log_command("devlog_set_llm_api_log_stream_enabled");
    llm_api_logs.set_stream_enabled(enabled);

    Ok(())
}

#[tauri::command]
pub async fn devlog_get_llm_api_log_index(
    limit: Option<u32>,
    llm_api_logs: State<'_, Arc<LlmApiLogStore>>,
) -> Result<Vec<LlmApiLogIndexEntry>, CommandError> {
    log_command("devlog_get_llm_api_log_index");
    let limit = limit.unwrap_or(50).max(1) as usize;
    Ok(llm_api_logs.tail_index(limit))
}

#[tauri::command]
pub async fn devlog_get_llm_api_log_preview(
    id: u64,
    llm_api_logs: State<'_, Arc<LlmApiLogStore>>,
) -> Result<LlmApiLogEntryPreview, CommandError> {
    log_command(format!("devlog_get_llm_api_log_preview {}", id));

    llm_api_logs.get_preview(id).await.map_err(|error| {
        CommandError::InternalServerError(format!("Failed to read LLM API log preview: {error}"))
    })
}

#[tauri::command]
pub async fn devlog_get_llm_api_log_raw(
    id: u64,
    llm_api_logs: State<'_, Arc<LlmApiLogStore>>,
) -> Result<LlmApiLogEntryRaw, CommandError> {
    log_command(format!("devlog_get_llm_api_log_raw {}", id));

    llm_api_logs.get_raw(id).await.map_err(|error| {
        CommandError::InternalServerError(format!("Failed to read LLM API log raw: {error}"))
    })
}
