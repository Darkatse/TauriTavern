use std::fs::File;
use std::path::Path;

use tauri::{AppHandle, Manager, WebviewWindow};
use tauri_plugin_dialog::{DialogExt, FileAccessMode, FilePath};
use tauri_plugin_fs::{FsExt, OpenOptions};
use tt_domain::errors::DomainError;

use super::{Delivery, PickKind, PickedFile, PickedSource, pick_dialog, save_dialog, sync_target};

pub(super) async fn pick(
    window: &WebviewWindow,
    kind: PickKind,
    multiple: bool,
) -> Result<Option<Vec<PickedFile>>, DomainError> {
    #[cfg(target_os = "android")]
    let filters = kind.mime_types();
    #[cfg(target_env = "ohos")]
    let filters = kind.extensions();
    let dialog = window
        .dialog()
        .file()
        .set_file_access_mode(FileAccessMode::Scoped)
        .add_filter("Files", filters);
    let Some(files) = pick_dialog(dialog, multiple).await? else {
        return Ok(None);
    };
    let mut picked = Vec::with_capacity(files.len());
    for file in files {
        let FilePath::Url(uri) = file else {
            return Err(DomainError::InvalidData(
                "Native picker did not return a file URI".into(),
            ));
        };
        #[cfg(target_os = "android")]
        let name = display_name(window.app_handle(), uri.as_str()).await?;
        #[cfg(target_env = "ohos")]
        let name =
            percent_encoding::percent_decode_str(uri.path().rsplit('/').next().unwrap_or_default())
                .decode_utf8()
                .map_err(|error| {
                    DomainError::InvalidData(format!("Invalid selected file name: {error}"))
                })?
                .into_owned();
        picked.push(PickedFile {
            name,
            source: PickedSource::ContentUri(uri.to_string()),
        });
    }
    Ok(Some(picked))
}

pub(super) fn open_read(app: &AppHandle, uri: &str) -> std::io::Result<File> {
    let uri = uri
        .parse()
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidInput, error))?;
    let mut options = OpenOptions::new();
    options.read(true);
    app.fs().open(FilePath::Url(uri), options)
}

pub(super) async fn deliver(
    window: &WebviewWindow,
    source: &Path,
    file_name: &str,
) -> Result<Delivery, DomainError> {
    // A document provider may create the target before the save dialog returns.
    let input = tokio::fs::File::open(source)
        .await
        .map_err(|error| {
            DomainError::file_io("Open export source", source.display().to_string(), error)
        })?
        .into_std()
        .await;
    let dialog = window
        .dialog()
        .file()
        .set_file_access_mode(FileAccessMode::Scoped)
        .set_file_name(file_name);
    #[cfg(target_os = "android")]
    let dialog = dialog.add_filter(
        "File",
        &[mime_guess::from_path(file_name)
            .first_or_octet_stream()
            .essence_str()],
    );
    #[cfg(target_env = "ohos")]
    let dialog = match Path::new(file_name)
        .extension()
        .and_then(|value| value.to_str())
    {
        Some(extension) => dialog.add_filter("File", &[extension]),
        None => dialog,
    };
    let Some(target) = save_dialog(dialog).await? else {
        return Ok(Delivery::Cancelled);
    };
    let app = window.app_handle().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let write = || -> std::io::Result<()> {
            let mut input = input;
            let mut options = OpenOptions::new();
            options.write(true).truncate(true).create(true);
            let mut output = app.fs().open(target, options)?;
            std::io::copy(&mut input, &mut output)?;
            sync_target(&output)
        };
        write().map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to save file; the selected target may be incomplete: {error}"
            ))
        })
    })
    .await
    .map_err(|error| DomainError::InternalError(format!("File delivery task failed: {error}")))??;
    Ok(Delivery::Delivered)
}

#[cfg(target_os = "android")]
struct ContentUriPlugin<R: tauri::Runtime> {
    handle: Result<tauri::plugin::PluginHandle<R>, String>,
}

#[cfg(target_os = "android")]
async fn display_name(app: &AppHandle, uri: &str) -> Result<String, DomainError> {
    #[derive(serde::Deserialize)]
    struct DisplayName {
        name: String,
    }

    let plugin = app.state::<ContentUriPlugin<tauri::Wry>>();
    let handle = plugin.handle.as_ref().map_err(|error| {
        DomainError::InternalError(format!("Content URI plugin is unavailable: {error}"))
    })?;
    handle
        .run_mobile_plugin_async::<DisplayName>("displayName", serde_json::json!({ "uri": uri }))
        .await
        .map(|result| result.name)
        .map_err(|error| {
            DomainError::InternalError(format!("Failed to read selected file name: {error}"))
        })
}

#[cfg(target_os = "android")]
pub fn plugin<R: tauri::Runtime>() -> tauri::plugin::TauriPlugin<R> {
    tauri::plugin::Builder::new("content-uri")
        .setup(|app, api| {
            let handle = api
                .register_android_plugin("com.tauritavern.client", "ContentUriPlugin")
                .map_err(|error| error.to_string());
            app.manage(ContentUriPlugin { handle });
            Ok(())
        })
        .build()
}
