use std::path::Path;

use serde::Serialize;
use tauri::{AppHandle, WebviewWindow};

use crate::infrastructure::staging;
#[cfg(target_os = "android")]
use crate::platform::file_transfer::PickedSource;
use crate::platform::file_transfer::{self, Delivery, PickKind, PickedFile};
use crate::presentation::commands::file_staging_commands::StagedFileResult;
use crate::presentation::errors::CommandError;

#[derive(Serialize)]
pub struct DeliveryResult {
    delivered: bool,
}

impl From<Delivery> for DeliveryResult {
    fn from(delivery: Delivery) -> Self {
        Self {
            delivered: matches!(delivery, Delivery::Delivered),
        }
    }
}

async fn stage_selected_file(
    app: &AppHandle,
    kind: PickKind,
    extension: &str,
    file: PickedFile,
) -> Result<StagedFileResult, CommandError> {
    let target = staging::create_staged_file(app, kind.staging_kind(), extension).await?;
    let path = file_transfer::materialize(app, file.source, target).await?;
    Ok(StagedFileResult {
        path: path.to_string_lossy().into_owned(),
        name: file.name,
    })
}

fn picked_file_extension(kind: PickKind, name: &str) -> Result<String, CommandError> {
    Path::new(name)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .filter(|extension| kind.extensions().contains(&extension.as_str()))
        .ok_or_else(|| CommandError::BadRequest(format!("Unsupported file type: {name}")))
}

#[tauri::command]
pub async fn pick_import_files(
    app: AppHandle,
    window: WebviewWindow,
    kind: PickKind,
    multiple: bool,
) -> Result<Option<Vec<StagedFileResult>>, CommandError> {
    if matches!(kind, PickKind::DataArchive) {
        return Err(CommandError::BadRequest(
            "Data archives use the archive import command".into(),
        ));
    }
    let Some(files) = file_transfer::pick(&window, kind, multiple).await? else {
        return Ok(None);
    };
    let extensions = files
        .iter()
        .map(|file| picked_file_extension(kind, &file.name))
        .collect::<Result<Vec<_>, _>>()?;
    let mut staged = Vec::with_capacity(files.len());
    let result = async {
        for (file, extension) in files.into_iter().zip(extensions) {
            staged.push(stage_selected_file(&app, kind, &extension, file).await?);
        }
        Ok::<_, CommandError>(())
    }
    .await;
    if let Err(error) = result {
        for file in &staged {
            if let Err(cleanup_error) = staging::discard_file(Path::new(&file.path)).await {
                tracing::warn!(%cleanup_error, "Failed to discard selected file");
            }
        }
        return Err(error);
    }
    Ok(Some(staged))
}

/// The share target forwards only PNG cards, so the file is staged as PNG.
#[cfg(target_os = "android")]
#[tauri::command]
pub async fn stage_shared_file(
    app: AppHandle,
    uri: String,
    name: String,
) -> Result<StagedFileResult, CommandError> {
    if !uri.starts_with("content://") {
        return Err(CommandError::BadRequest(
            "Shared files must be content URIs".into(),
        ));
    }
    let file = PickedFile {
        name,
        source: PickedSource::ContentUri(uri),
    };
    stage_selected_file(&app, PickKind::CharacterCard, "png", file).await
}

#[tauri::command]
pub async fn deliver_staged_file(
    app: AppHandle,
    window: WebviewWindow,
    path: String,
    file_name: String,
) -> Result<DeliveryResult, CommandError> {
    let path = staging::validate_staged_path(&app, &path).await?;
    Ok(file_transfer::deliver(&window, path, &file_name)
        .await?
        .into())
}
