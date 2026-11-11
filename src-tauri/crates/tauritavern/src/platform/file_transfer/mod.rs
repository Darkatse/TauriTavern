//! Native file selection and delivery. Callers own content; this module owns
//! platform access, dialogs, and the lifetime of a delivered source file.

#[cfg(any(target_os = "android", target_env = "ohos"))]
mod content_uri;
#[cfg(desktop)]
mod desktop;
#[cfg(target_os = "ios")]
mod ios;

use std::path::PathBuf;

use serde::Deserialize;
use tauri::{AppHandle, WebviewWindow};
use tt_domain::errors::DomainError;

use crate::infrastructure::staging;

#[cfg(any(target_os = "android", target_env = "ohos"))]
use content_uri as native;
#[cfg(desktop)]
use desktop as native;
#[cfg(target_os = "ios")]
use ios as native;

#[cfg(target_os = "android")]
pub use content_uri::plugin;

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PickKind {
    CharacterCard,
    SkillArchive,
    DataArchive,
}

impl PickKind {
    pub fn staging_kind(self) -> &'static str {
        match self {
            Self::CharacterCard => "character-card",
            Self::SkillArchive => "skill-archive",
            Self::DataArchive => "data-archive",
        }
    }

    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            Self::CharacterCard => &["json", "png"],
            Self::SkillArchive => &["zip", "ttskill"],
            Self::DataArchive => &["zip", "tar", "gz", "tgz"],
        }
    }

    #[cfg(target_os = "android")]
    fn mime_types(self) -> &'static [&'static str] {
        match self {
            Self::CharacterCard => &["application/json", "image/png", "application/octet-stream"],
            Self::SkillArchive => &[
                "application/zip",
                "application/x-zip-compressed",
                "application/octet-stream",
            ],
            Self::DataArchive => &[
                "application/zip",
                "application/x-zip-compressed",
                "application/gzip",
                "application/x-gzip",
                "application/x-tar",
                "application/octet-stream",
            ],
        }
    }

    #[cfg(target_os = "ios")]
    fn content_types(self) -> &'static [&'static str] {
        match self {
            Self::CharacterCard => &["public.json", "public.png"],
            Self::SkillArchive => &[
                "public.zip-archive",
                "com.pkware.zip-archive",
                "public.data",
            ],
            Self::DataArchive => &[
                "public.zip-archive",
                "com.pkware.zip-archive",
                "public.tar-archive",
                "org.gnu.gnu-zip-archive",
                "com.tauritavern.client.tar-archive",
                "com.tauritavern.client.gzip-archive",
            ],
        }
    }
}

pub struct PickedFile {
    pub name: String,
    pub source: PickedSource,
}

pub enum PickedSource {
    #[cfg(desktop)]
    UserPath(PathBuf),
    #[cfg(target_os = "ios")]
    AppCopy(PathBuf),
    #[cfg(any(target_os = "android", target_env = "ohos"))]
    ContentUri(String),
}

pub enum Delivery {
    Delivered,
    Cancelled,
}

pub async fn pick(
    window: &WebviewWindow,
    kind: PickKind,
    multiple: bool,
) -> Result<Option<Vec<PickedFile>>, DomainError> {
    native::pick(window, kind, multiple).await
}

pub async fn materialize(
    app: &AppHandle,
    source: PickedSource,
    destination: PathBuf,
) -> Result<PathBuf, DomainError> {
    #[cfg(any(target_os = "android", target_env = "ohos"))]
    let app = app.clone();
    #[cfg(not(any(target_os = "android", target_env = "ohos")))]
    let _ = app;
    let target = destination.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let result = match source {
            #[cfg(desktop)]
            PickedSource::UserPath(path) => std::fs::copy(path, &target).map(|_| ()),
            #[cfg(target_os = "ios")]
            PickedSource::AppCopy(path) => std::fs::rename(path, &target),
            #[cfg(any(target_os = "android", target_env = "ohos"))]
            PickedSource::ContentUri(uri) => (|| {
                let mut input = content_uri::open_read(&app, &uri)?;
                let mut output = std::fs::File::create(&target)?;
                std::io::copy(&mut input, &mut output)?;
                Ok(())
            })(),
        };
        result.map_err(|error| {
            DomainError::file_io("Stage selected file", target.display().to_string(), error)
        })
    })
    .await
    .map_err(|error| DomainError::InternalError(format!("File staging task failed: {error}")))?;
    if let Err(error) = result {
        if let Err(cleanup_error) = staging::discard_file(&destination).await {
            tracing::warn!(%cleanup_error, "Failed to remove incomplete staged file");
        }
        return Err(error);
    }
    Ok(destination)
}

/// Takes ownership of a private source file, including on cancellation or error.
pub async fn deliver(
    window: &WebviewWindow,
    source: PathBuf,
    file_name: &str,
) -> Result<Delivery, DomainError> {
    let file_name = sanitize_file_name(file_name);
    let result = native::deliver(window, &source, &file_name).await;
    if let Err(error) = staging::discard_file(&source).await {
        tracing::warn!(%error, "Failed to remove file after delivery");
    }
    result
}

fn sanitize_file_name(name: &str) -> String {
    let name: String = name
        .chars()
        .map(|ch| {
            if ch.is_control() || matches!(ch, '/' | '\\' | '<' | '>' | ':' | '"' | '|' | '?' | '*')
            {
                '_'
            } else {
                ch
            }
        })
        .collect();
    let name = name.trim().trim_end_matches(['.', ' ']);
    if name.is_empty() {
        "download".to_string()
    } else {
        name.to_string()
    }
}

#[cfg(not(target_os = "ios"))]
fn sync_target(file: &std::fs::File) -> std::io::Result<()> {
    // A provider may expose a pipe rather than a disk file; it owns persistence.
    if file.metadata()?.is_file() {
        file.sync_all()?;
    }
    Ok(())
}

#[cfg(not(target_os = "ios"))]
async fn pick_dialog(
    dialog: tauri_plugin_dialog::FileDialogBuilder<tauri::Wry>,
    multiple: bool,
) -> Result<Option<Vec<tauri_plugin_dialog::FilePath>>, DomainError> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    if multiple {
        dialog.pick_files(move |files| {
            let _ = sender.send(files);
        });
    } else {
        dialog.pick_file(move |file| {
            let _ = sender.send(file.map(|file| vec![file]));
        });
    }
    receiver.await.map_err(|error| {
        DomainError::InternalError(format!("File picker did not complete: {error}"))
    })
}

#[cfg(not(target_os = "ios"))]
async fn save_dialog(
    dialog: tauri_plugin_dialog::FileDialogBuilder<tauri::Wry>,
) -> Result<Option<tauri_plugin_dialog::FilePath>, DomainError> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    dialog.save_file(move |file| {
        let _ = sender.send(file);
    });
    receiver.await.map_err(|error| {
        DomainError::InternalError(format!("Save dialog did not complete: {error}"))
    })
}
