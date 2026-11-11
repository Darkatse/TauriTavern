use std::path::Path;

use tauri::{Manager, WebviewWindow};
use tauri_plugin_dialog::DialogExt;
use tt_domain::errors::DomainError;

use super::{Delivery, PickKind, PickedFile, PickedSource, pick_dialog, save_dialog, sync_target};

pub(super) async fn pick(
    window: &WebviewWindow,
    kind: PickKind,
    multiple: bool,
) -> Result<Option<Vec<PickedFile>>, DomainError> {
    let dialog = window
        .dialog()
        .file()
        .set_parent(window)
        .add_filter("Files", kind.extensions());
    let Some(files) = pick_dialog(dialog, multiple).await? else {
        return Ok(None);
    };
    files
        .into_iter()
        .map(|file| {
            let path = file
                .into_path()
                .map_err(|error| DomainError::InvalidData(error.to_string()))?;
            let name = path
                .file_name()
                .ok_or_else(|| DomainError::InvalidData("Selected file has no name".into()))?
                .to_string_lossy()
                .into_owned();
            Ok(PickedFile {
                name,
                source: PickedSource::UserPath(path),
            })
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Some)
}

pub(super) async fn deliver(
    window: &WebviewWindow,
    source: &Path,
    file_name: &str,
) -> Result<Delivery, DomainError> {
    let mut dialog = window
        .dialog()
        .file()
        .set_parent(window)
        .set_file_name(file_name);
    if let Ok(download_dir) = window.app_handle().path().download_dir() {
        dialog = dialog.set_directory(download_dir);
    }
    if let Some(extension) = Path::new(file_name)
        .extension()
        .and_then(|value| value.to_str())
    {
        dialog = dialog.add_filter("File", &[extension]);
    }
    let Some(target) = save_dialog(dialog).await? else {
        return Ok(Delivery::Cancelled);
    };
    let target = target
        .into_path()
        .map_err(|error| DomainError::InvalidData(error.to_string()))?;
    let source = source.to_path_buf();
    tauri::async_runtime::spawn_blocking(move || {
        let write = || -> std::io::Result<()> {
            match std::fs::rename(&source, &target) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::CrossesDevices => {
                    std::fs::copy(&source, &target)?;
                }
                Err(error) => return Err(error),
            }
            // FlushFileBuffers requires a writable handle on Windows.
            let output = std::fs::OpenOptions::new().write(true).open(&target)?;
            sync_target(&output)
        };
        write().map_err(|error| {
            DomainError::file_io(
                "Save file (target may be incomplete)",
                target.display().to_string(),
                error,
            )
        })
    })
    .await
    .map_err(|error| DomainError::InternalError(format!("File delivery task failed: {error}")))??;
    Ok(Delivery::Delivered)
}
