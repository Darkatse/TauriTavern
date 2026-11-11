use std::sync::Arc;

use serde::Serialize;
#[cfg(mobile)]
use tauri::Manager;
use tauri::{State, WebviewWindow};

use crate::app::AppState;
use crate::platform::file_transfer::{self, PickKind};
use crate::presentation::commands::file_transfer_commands::DeliveryResult;
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::errors::CommandError;
use tt_application::dto::data_archive_dto::DataArchiveJobStatus;

#[derive(Serialize)]
pub struct ImportArchiveResult {
    cancelled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    job_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    file_name: Option<String>,
}

#[tauri::command]
pub async fn import_data_archive_from_picker(
    app_state: State<'_, Arc<AppState>>,
    window: WebviewWindow,
) -> Result<ImportArchiveResult, CommandError> {
    log_command("import_data_archive_from_picker");
    let Some(files) = file_transfer::pick(&window, PickKind::DataArchive, false).await? else {
        return Ok(ImportArchiveResult {
            cancelled: true,
            job_id: None,
            file_name: None,
        });
    };
    let picked = files
        .into_iter()
        .next()
        .ok_or_else(|| CommandError::BadRequest("No archive was selected".into()))?;
    let service = app_state.services.data_archive_service.clone();

    #[cfg(desktop)]
    let (archive_path, temporary) = {
        let file_transfer::PickedSource::UserPath(path) = picked.source;
        (path, false)
    };
    #[cfg(mobile)]
    let (archive_path, temporary) = {
        let staging_service = service.clone();
        let target = tauri::async_runtime::spawn_blocking(move || {
            staging_service.prepare_incoming_import_archive_path()
        })
        .await
        .map_err(|error| {
            CommandError::InternalServerError(format!("Import staging task failed: {error}"))
        })??;
        (
            file_transfer::materialize(window.app_handle(), picked.source, target).await?,
            true,
        )
    };

    let job_id = tauri::async_runtime::spawn_blocking(move || {
        let result = service.start_import(&archive_path, temporary);
        // A temporary source is moved into the job workspace before returning.
        if temporary && result.is_err()
            && let Err(error) = std::fs::remove_file(&archive_path)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!(%error, path = %archive_path.display(), "Failed to discard imported archive");
        }
        result
    }).await.map_err(|error| CommandError::InternalServerError(format!("Import startup task failed: {error}")))??;
    Ok(ImportArchiveResult {
        cancelled: false,
        job_id: Some(job_id),
        file_name: Some(picked.name),
    })
}

#[tauri::command]
pub fn start_export_data_archive(
    app_state: State<'_, Arc<AppState>>,
) -> Result<String, CommandError> {
    log_command("start_export_data_archive");
    app_state
        .services
        .data_archive_service
        .start_export()
        .map_err(map_command_error("Failed to start data archive export"))
}

#[tauri::command]
pub fn get_data_archive_job_status(
    app_state: State<'_, Arc<AppState>>,
    job_id: String,
) -> Result<DataArchiveJobStatus, CommandError> {
    app_state
        .services
        .data_archive_service
        .get_status(&job_id)
        .map_err(map_command_error("Failed to get data archive job status"))
}

#[tauri::command]
pub fn cancel_data_archive_job(
    app_state: State<'_, Arc<AppState>>,
    job_id: String,
) -> Result<(), CommandError> {
    app_state
        .services
        .data_archive_service
        .cancel(&job_id)
        .map_err(map_command_error("Failed to cancel data archive job"))
}

#[tauri::command]
pub async fn save_export_data_archive(
    app_state: State<'_, Arc<AppState>>,
    window: WebviewWindow,
    job_id: String,
) -> Result<DeliveryResult, CommandError> {
    log_command(format!("save_export_data_archive {job_id}"));
    let service = &app_state.services.data_archive_service;
    let artifact = service.claim_export_artifact(&job_id)?;
    let delivery =
        file_transfer::deliver(&window, artifact.archive_path, &artifact.file_name).await;
    service.finish_export_delivery(&job_id)?;
    Ok(delivery?.into())
}

#[tauri::command]
pub async fn export_user_backup_archive(
    app_state: State<'_, Arc<AppState>>,
    window: WebviewWindow,
    handle: String,
    include_secrets: bool,
) -> Result<DeliveryResult, CommandError> {
    log_command(format!(
        "export_user_backup_archive {handle} include_secrets={include_secrets}"
    ));
    let archive = app_state
        .services
        .data_archive_service
        .export_user_backup(handle, include_secrets)
        .await?;
    Ok(
        file_transfer::deliver(&window, archive.archive_path.into(), &archive.file_name)
            .await?
            .into(),
    )
}
