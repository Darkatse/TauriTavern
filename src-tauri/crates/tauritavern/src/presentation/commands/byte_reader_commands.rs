use std::sync::Arc;

use tauri::{Manager, Resource, ResourceId, Runtime, Webview};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use tt_domain::errors::DomainError;
use tt_ports::byte_reader::ByteReader;

use super::helpers::map_command_error;
use crate::presentation::errors::CommandError;

/// Closing cancels a pending read, so the reader is released even when its source stalls.
pub(super) struct ByteResource {
    reader: Mutex<Box<dyn ByteReader>>,
    chunk_bytes: usize,
    closed: CancellationToken,
}

impl ByteResource {
    pub(super) fn new(reader: Box<dyn ByteReader>, chunk_bytes: usize) -> Self {
        Self {
            reader: Mutex::new(reader),
            chunk_bytes,
            closed: CancellationToken::new(),
        }
    }

    async fn read(&self) -> Result<Vec<u8>, DomainError> {
        let mut buffer = vec![0; self.chunk_bytes];
        let bytes_read = tokio::select! {
            read = async { self.reader.lock().await.read(&mut buffer).await } => read?,
            () = self.closed.cancelled() => {
                return Err(DomainError::Cancelled("Byte reader closed".to_string()));
            }
        };
        buffer.truncate(bytes_read);
        Ok(buffer)
    }
}

impl Resource for ByteResource {
    fn close(self: Arc<Self>) {
        self.closed.cancel();
    }
}

#[tauri::command]
pub async fn read_bytes<R: Runtime>(
    rid: ResourceId,
    webview: Webview<R>,
) -> Result<tauri::ipc::Response, CommandError> {
    let resource = webview
        .resources_table()
        .get::<ByteResource>(rid)
        .map_err(map_command_error("Failed to access byte reader"))?;
    let bytes = resource
        .read()
        .await
        .map_err(map_command_error("Failed to read bytes"))?;
    Ok(tauri::ipc::Response::new(bytes))
}

/// Page reloads cannot await JS cancellation.
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

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    use async_trait::async_trait;
    use tokio::sync::oneshot;

    use super::*;

    struct StalledReader {
        reading: Option<oneshot::Sender<()>>,
        released: Arc<AtomicBool>,
    }

    #[async_trait]
    impl ByteReader for StalledReader {
        async fn read(&mut self, _buffer: &mut [u8]) -> Result<usize, DomainError> {
            let _ = self.reading.take().unwrap().send(());
            std::future::pending().await
        }
    }

    impl Drop for StalledReader {
        fn drop(&mut self) {
            self.released.store(true, Ordering::SeqCst);
        }
    }

    #[tokio::test]
    async fn closing_releases_a_reader_waiting_for_data() {
        let (reading, read_started) = oneshot::channel();
        let released = Arc::new(AtomicBool::new(false));
        let resource = Arc::new(ByteResource::new(
            Box::new(StalledReader {
                reading: Some(reading),
                released: released.clone(),
            }),
            16,
        ));
        let read = tokio::spawn({
            let resource = resource.clone();
            async move { resource.read().await }
        });
        read_started.await.unwrap();

        resource.close();

        let result = tokio::time::timeout(Duration::from_secs(5), read)
            .await
            .expect("closing cancels the pending read");
        assert!(matches!(result.unwrap(), Err(DomainError::Cancelled(_))));
        assert!(released.load(Ordering::SeqCst));
    }
}
