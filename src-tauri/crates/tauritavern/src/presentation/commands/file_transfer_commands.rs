use std::path::Path;

use serde::Serialize;
use tauri::{AppHandle, WebviewWindow};

use crate::infrastructure::staging;
use crate::platform::file_transfer::{self, Delivery, PickKind};
use crate::presentation::errors::CommandError;

#[derive(Serialize)]
pub struct PickedFileResult {
    path: String,
    name: String,
}

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

#[tauri::command]
pub async fn pick_import_files(
    app: AppHandle,
    window: WebviewWindow,
    kind: PickKind,
    multiple: bool,
) -> Result<Option<Vec<PickedFileResult>>, CommandError> {
    if matches!(kind, PickKind::DataArchive) {
        return Err(CommandError::BadRequest(
            "Data archives use the archive import command".into(),
        ));
    }
    let Some(files) = file_transfer::pick(&window, kind, multiple).await? else {
        return Ok(None);
    };
    let mut staged = Vec::with_capacity(files.len());
    let result = async {
        for file in files {
            let extension = Path::new(&file.name)
                .extension()
                .and_then(|ext| ext.to_str())
                .unwrap_or(kind.extensions()[0]);
            let target = staging::create_staged_file(&app, kind.staging_kind(), extension).await?;
            let path = file_transfer::materialize(&app, file.source, target).await?;
            staged.push(PickedFileResult {
                path: path.to_string_lossy().into_owned(),
                name: file.name,
            });
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
