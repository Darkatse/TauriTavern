use std::sync::Arc;

use chrono::TimeZone;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

use crate::infrastructure::logging::dev_bundle::{DevLogBundleInput, export_dev_log_bundle};
use crate::infrastructure::logging::devtools::{BackendLogEntry, BackendLogStore};
use crate::infrastructure::logging::llm_api_logs::{
    LlmApiLogEntryPreview, LlmApiLogEntryRaw, LlmApiLogIndexEntry, LlmApiLogStore,
};
use crate::infrastructure::paths::RuntimePaths;
use crate::presentation::commands::bridge::get_client_version;
use crate::presentation::commands::helpers::log_command;
use crate::presentation::errors::CommandError;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrontendLogEntryDto {
    pub level: String,
    pub message: String,
    pub target: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrontendLogEntrySnapshotDto {
    pub id: u64,
    pub timestamp_ms: i64,
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DevLogBundleMeta {
    exported_at: String,
    os: String,
    arch: String,
    runtime_paths: DevLogBundleRuntimePaths,
    version: crate::presentation::commands::bridge::VersionInfo,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DevLogBundleRuntimePaths {
    data_root: String,
    log_root: String,
}

fn format_log_timestamp(ms: i64) -> String {
    let Some(ts) = chrono::Utc.timestamp_millis_opt(ms).single() else {
        return format!("Invalid({ms})");
    };

    ts.to_rfc3339()
}

fn format_backend_tail(entries: &[BackendLogEntry]) -> String {
    let mut out = String::new();
    for entry in entries {
        let line = format!(
            "[{}] [{}] [{}] {}\n",
            format_log_timestamp(entry.timestamp_ms),
            entry.level,
            entry.target,
            entry.message
        );
        out.push_str(&line);
    }

    out
}

fn format_frontend_jsonl(entries: &[FrontendLogEntrySnapshotDto]) -> Result<String, CommandError> {
    let mut out = String::new();

    for entry in entries {
        let line = serde_json::to_string(entry).map_err(|error| {
            CommandError::InternalServerError(format!(
                "Failed to serialize frontend log entry: {}",
                error
            ))
        })?;
        out.push_str(&line);
        out.push('\n');
    }

    Ok(out)
}

fn bundle_readme() -> String {
    [
        "TauriTavern dev bundle (for bug reports)",
        "",
        "- frontend/logs.jsonl: preview only (truncated/summarized).",
        "- backend/*.log: full backend file logs (may include forwarded frontend logs).",
        "- llm-api/*: LLM API request/response raw logs (may contain prompts/responses).",
        "- settings/*: app settings snapshot (secrets are not included).",
        "",
        "Review files before sharing.",
        "",
    ]
    .join("\n")
}

#[tauri::command]
pub async fn devlog_export_bundle(
    app: AppHandle,
    frontend_entries: Vec<FrontendLogEntrySnapshotDto>,
    backend_logs: State<'_, Arc<BackendLogStore>>,
    runtime_paths: State<'_, RuntimePaths>,
) -> Result<String, CommandError> {
    log_command("devlog_export_bundle");

    let backend_tail = backend_logs.tail(800);

    let meta = DevLogBundleMeta {
        exported_at: chrono::Utc::now().to_rfc3339(),
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        runtime_paths: DevLogBundleRuntimePaths {
            data_root: runtime_paths.data_root.to_string_lossy().to_string(),
            log_root: runtime_paths.log_root.to_string_lossy().to_string(),
        },
        version: get_client_version()?,
    };

    let meta_json = serde_json::to_string_pretty(&meta).map_err(|error| {
        CommandError::InternalServerError(format!(
            "Failed to serialize dev bundle metadata: {}",
            error
        ))
    })?;

    let frontend_logs_jsonl = format_frontend_jsonl(&frontend_entries)?;
    let backend_logs_tail_text = format_backend_tail(&backend_tail);
    let readme_text = bundle_readme();

    let app_handle = app.clone();
    let runtime_paths = runtime_paths.inner().clone();

    let output_path = tauri::async_runtime::spawn_blocking(move || {
        export_dev_log_bundle(
            &app_handle,
            &runtime_paths,
            DevLogBundleInput {
                meta_json,
                readme_text,
                frontend_logs_jsonl,
                backend_logs_tail_text,
            },
        )
    })
    .await
    .map_err(|error| {
        CommandError::InternalServerError(format!("Export bundle task join error: {}", error))
    })??;

    Ok(output_path.to_string_lossy().to_string())
}
