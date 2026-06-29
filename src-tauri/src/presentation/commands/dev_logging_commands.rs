use std::sync::Arc;

use tauri::State;

use crate::app::dev_observability::DevObservabilityHub;
use crate::application::dto::dev_observability_dto::{
    BackendLogEntryDto, DevBundleVersionDto, FrontendLogEntryDto, FrontendLogEntrySnapshotDto,
    LlmApiLogIndexEntryDto, LlmApiLogPreviewDto, LlmApiLogRawDto,
};
use crate::presentation::commands::bridge::{VersionInfo, get_client_version};
use crate::presentation::commands::helpers::log_command;
use crate::presentation::errors::CommandError;

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
    observability: State<'_, Arc<DevObservabilityHub>>,
) -> Result<(), CommandError> {
    log_command("devlog_set_backend_log_stream_enabled");
    observability.set_backend_stream_enabled(enabled);
    Ok(())
}

#[tauri::command]
pub async fn devlog_get_backend_log_tail(
    limit: Option<u32>,
    observability: State<'_, Arc<DevObservabilityHub>>,
) -> Result<Vec<BackendLogEntryDto>, CommandError> {
    log_command("devlog_get_backend_log_tail");

    let limit = limit.unwrap_or(800) as usize;
    Ok(observability.tail_backend_logs(limit))
}

#[tauri::command]
pub async fn devlog_set_llm_api_log_stream_enabled(
    enabled: bool,
    observability: State<'_, Arc<DevObservabilityHub>>,
) -> Result<(), CommandError> {
    log_command("devlog_set_llm_api_log_stream_enabled");
    observability.set_llm_api_stream_enabled(enabled);

    Ok(())
}

#[tauri::command]
pub async fn devlog_get_llm_api_log_index(
    limit: Option<u32>,
    observability: State<'_, Arc<DevObservabilityHub>>,
) -> Result<Vec<LlmApiLogIndexEntryDto>, CommandError> {
    log_command("devlog_get_llm_api_log_index");
    let limit = limit.unwrap_or(50).max(1) as usize;
    Ok(observability.tail_llm_api_index(limit))
}

#[tauri::command]
pub async fn devlog_get_llm_api_log_preview(
    id: u64,
    observability: State<'_, Arc<DevObservabilityHub>>,
) -> Result<LlmApiLogPreviewDto, CommandError> {
    log_command(format!("devlog_get_llm_api_log_preview {}", id));

    observability
        .get_llm_api_preview(id)
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn devlog_get_llm_api_log_raw(
    id: u64,
    observability: State<'_, Arc<DevObservabilityHub>>,
) -> Result<LlmApiLogRawDto, CommandError> {
    log_command(format!("devlog_get_llm_api_log_raw {}", id));

    observability
        .get_llm_api_raw(id)
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn devlog_export_bundle(
    frontend_entries: Vec<FrontendLogEntrySnapshotDto>,
    observability: State<'_, Arc<DevObservabilityHub>>,
) -> Result<String, CommandError> {
    log_command("devlog_export_bundle");

    let output_path = observability
        .export_bundle(
            frontend_entries,
            dev_bundle_version_dto(get_client_version()?),
        )
        .await?;

    Ok(output_path.to_string_lossy().to_string())
}

fn dev_bundle_version_dto(version: VersionInfo) -> DevBundleVersionDto {
    DevBundleVersionDto {
        agent: version.agent,
        pkg_version: version.pkg_version,
        tauri_version: version.tauri_version,
        git_revision: version.git_revision,
        git_branch: version.git_branch,
    }
}
