use std::path::Path;

use tauri::WebviewWindow;
use tt_domain::errors::DomainError;

use super::{Delivery, PickKind, PickedFile};
use crate::platform::{ios_document_picker, ios_share_sheet};

pub(super) async fn pick(
    window: &WebviewWindow,
    kind: PickKind,
    multiple: bool,
) -> Result<Option<Vec<PickedFile>>, DomainError> {
    ios_document_picker::pick_documents(window, kind.content_types(), multiple).await
}

pub(super) async fn deliver(
    window: &WebviewWindow,
    source: &Path,
    file_name: &str,
) -> Result<Delivery, DomainError> {
    let completed = ios_share_sheet::share_file(window, source, file_name).await?;
    Ok(if completed {
        Delivery::Delivered
    } else {
        Delivery::Cancelled
    })
}
