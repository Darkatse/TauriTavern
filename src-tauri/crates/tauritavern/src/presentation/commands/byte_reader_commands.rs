use tauri::{Manager, Resource, ResourceId, Webview};
use tokio::sync::Mutex;
use tt_ports::byte_reader::ByteReader;

use super::helpers::map_command_error;
use crate::presentation::errors::CommandError;

pub(super) struct ByteResource {
    reader: Mutex<Box<dyn ByteReader>>,
    chunk_bytes: usize,
}

impl ByteResource {
    pub(super) fn new(reader: Box<dyn ByteReader>, chunk_bytes: usize) -> Self {
        Self {
            reader: Mutex::new(reader),
            chunk_bytes,
        }
    }
}

impl Resource for ByteResource {}

#[tauri::command]
pub async fn read_bytes(
    rid: ResourceId,
    webview: Webview,
) -> Result<tauri::ipc::Response, CommandError> {
    let resource = webview
        .resources_table()
        .get::<ByteResource>(rid)
        .map_err(map_command_error("Failed to access byte reader"))?;
    let mut buffer = vec![0; resource.chunk_bytes];
    let bytes_read = resource
        .reader
        .lock()
        .await
        .read(&mut buffer)
        .await
        .map_err(map_command_error("Failed to read bytes"))?;
    buffer.truncate(bytes_read);
    Ok(tauri::ipc::Response::new(buffer))
}

/// Page reloads cannot await JS cancellation. In-flight reads retain their own Arc.
pub(crate) fn close_page_byte_readers(webview: &Webview) {
    let mut table = webview.resources_table();
    let ids: Vec<_> = table
        .names()
        .map(|(rid, _)| rid)
        .filter(|rid| table.get::<ByteResource>(*rid).is_ok())
        .collect();
    for rid in ids {
        let _ = table.close(rid);
    }
}
