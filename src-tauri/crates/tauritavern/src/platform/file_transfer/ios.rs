use std::path::Path;

use tauri::{Manager, WebviewWindow};
use tt_domain::errors::DomainError;

use super::{Delivery, PickKind, PickedFile};
use crate::infrastructure::staging;
use crate::platform::{ios_document_picker, ios_share_sheet};

pub(super) async fn pick(
    window: &WebviewWindow,
    kind: PickKind,
    multiple: bool,
) -> Result<Option<Vec<PickedFile>>, DomainError> {
    ios_document_picker::pick_documents(window, kind.content_types(), multiple).await
}

/// Share Sheet receivers name a file by its file name, so the source is handed
/// over under the delivery name from a host directory of its own.
pub(super) async fn deliver(
    window: &WebviewWindow,
    source: &Path,
    file_name: &str,
) -> Result<Delivery, DomainError> {
    let directory = staging::create_host_directory(window.app_handle()).await?;
    let result = share_as(window, source, &directory.join(file_name)).await;
    if let Err(error) = staging::discard(&directory).await {
        tracing::warn!(%error, "Failed to remove shared file");
    }
    result
}

async fn share_as(
    window: &WebviewWindow,
    source: &Path,
    shared: &Path,
) -> Result<Delivery, DomainError> {
    tokio::fs::rename(source, shared).await.map_err(|error| {
        DomainError::file_io("Name shared file", shared.display().to_string(), error)
    })?;
    let completed = ios_share_sheet::share_file(window, shared).await?;
    Ok(if completed {
        Delivery::Delivered
    } else {
        Delivery::Cancelled
    })
}
