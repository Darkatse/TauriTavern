use std::collections::HashMap;
use std::path::Path;
use std::pin::Pin;
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
use ttsync_contract::manifest::ManifestEntryV2;
use ttsync_contract::path::SyncPath;

use crate::domain::errors::DomainError;
use crate::infrastructure::{sync_fs, sync_transfer};

pub(crate) const FEATURE_BUNDLE_V1: &str = "bundle_v1";
pub(crate) const FEATURE_ZSTD_V1: &str = "zstd_v1";

pub(crate) const BUNDLE_STREAM_BUFFER_SIZE: usize = 64 * 1024;
pub(crate) const BUNDLE_ZSTD_DECODE_BUFFER_SIZE: usize = 1024 * 1024;
pub(crate) const MAX_BUNDLE_PATH_LEN: u32 = 16 * 1024;

pub(crate) struct BundleFileProgress {
    pub path: String,
    pub size_bytes: u64,
}

#[derive(Debug)]
pub(crate) struct BundleWriteError {
    error: DomainError,
    target_changed: bool,
}

impl BundleWriteError {
    fn unchanged(error: DomainError) -> Self {
        Self {
            error,
            target_changed: false,
        }
    }

    fn from_atomic(error: sync_fs::AtomicWriteError) -> Self {
        Self {
            target_changed: error.target_changed(),
            error: error.into_error(),
        }
    }

    pub(crate) fn target_changed(&self) -> bool {
        self.target_changed
    }

    pub(crate) fn into_error(self) -> DomainError {
        self.error
    }
}

impl From<BundleWriteError> for DomainError {
    fn from(error: BundleWriteError) -> Self {
        error.error
    }
}

pub(crate) async fn read_u32_be<R>(reader: &mut R) -> Result<u32, DomainError>
where
    R: AsyncRead + Unpin,
{
    let mut buf = [0u8; 4];
    reader
        .read_exact(&mut buf)
        .await
        .map_err(|error| DomainError::InternalError(error.to_string()))?;
    Ok(u32::from_be_bytes(buf))
}

pub(crate) async fn write_u32_be<W>(writer: &mut W, value: u32) -> Result<(), DomainError>
where
    W: AsyncWrite + Unpin,
{
    writer
        .write_all(&value.to_be_bytes())
        .await
        .map_err(|error| DomainError::InternalError(error.to_string()))
}

pub(crate) async fn copy_exact<R, W>(
    reader: &mut R,
    writer: &mut W,
    mut remaining: u64,
    buffer: &mut [u8],
) -> Result<(), DomainError>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    while remaining > 0 {
        let to_read = (buffer.len() as u64).min(remaining) as usize;
        let read = reader
            .read(&mut buffer[..to_read])
            .await
            .map_err(|error| DomainError::InternalError(error.to_string()))?;
        if read == 0 {
            return Err(DomainError::InternalError(
                "Unexpected EOF in bundle stream".to_string(),
            ));
        }
        writer
            .write_all(&buffer[..read])
            .await
            .map_err(|error| DomainError::InternalError(error.to_string()))?;
        remaining -= read as u64;
    }
    Ok(())
}

pub(crate) async fn write_bundle_to_local_files<R, F>(
    sync_root: &Path,
    transfer_entries: Vec<ManifestEntryV2>,
    reader: &mut R,
    mut on_file_written: F,
) -> Result<(), BundleWriteError>
where
    R: AsyncRead + Send + Unpin,
    F: FnMut(BundleFileProgress) -> Result<(), DomainError>,
{
    let files_total = transfer_entries.len();
    let mut files_written = 0usize;
    let mut remaining = transfer_entries
        .into_iter()
        .map(|entry| (entry.path.clone(), entry))
        .collect::<HashMap<SyncPath, ManifestEntryV2>>();

    loop {
        let path_len = read_u32_be(reader)
            .await
            .map_err(BundleWriteError::unchanged)?;
        if path_len == 0 {
            break;
        }
        if path_len > MAX_BUNDLE_PATH_LEN {
            return Err(BundleWriteError::unchanged(DomainError::InvalidData(
                format!("Bundle path too long: {} bytes", path_len),
            )));
        }

        let mut path_bytes = vec![0u8; path_len as usize];
        reader.read_exact(&mut path_bytes).await.map_err(|error| {
            BundleWriteError::unchanged(DomainError::InternalError(error.to_string()))
        })?;

        let path_text = String::from_utf8(path_bytes).map_err(|_| {
            BundleWriteError::unchanged(DomainError::InvalidData(
                "Bundle path is not UTF-8".to_string(),
            ))
        })?;
        let sync_path = SyncPath::new(path_text).map_err(|error| {
            BundleWriteError::unchanged(DomainError::InvalidData(error.to_string()))
        })?;

        let entry = remaining
            .remove(&sync_path)
            .ok_or_else(|| DomainError::NotFound(format!("Bundle file not in plan: {}", sync_path)))
            .map_err(BundleWriteError::unchanged)?;

        let full_path = sync_transfer::resolve_to_local(sync_root, &entry.path);
        let mut exact = ExactSizeReader::new(&mut *reader, entry.size_bytes);
        sync_fs::write_file_atomic(&full_path, &mut exact, entry.modified_ms)
            .await
            .map_err(BundleWriteError::from_atomic)?;

        files_written += 1;
        on_file_written(BundleFileProgress {
            path: entry.path.to_string(),
            size_bytes: entry.size_bytes,
        })
        .map_err(BundleWriteError::unchanged)?;
    }

    if !remaining.is_empty() {
        return Err(BundleWriteError::unchanged(DomainError::InvalidData(
            format!(
                "Bundle ended early: {}/{} files received",
                files_written, files_total
            ),
        )));
    }

    Ok(())
}

pub(crate) struct ExactSizeReader<R> {
    inner: R,
    remaining: u64,
}

impl<R> ExactSizeReader<R> {
    pub(crate) fn new(inner: R, size_bytes: u64) -> Self {
        Self {
            inner,
            remaining: size_bytes,
        }
    }
}

impl<R> AsyncRead for ExactSizeReader<R>
where
    R: AsyncRead + Unpin,
{
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        if self.remaining == 0 {
            return Poll::Ready(Ok(()));
        }

        let max = (self.remaining as usize).min(buf.remaining());
        if max == 0 {
            return Poll::Ready(Ok(()));
        }

        let dst = buf.initialize_unfilled_to(max);
        let mut limited = ReadBuf::new(dst);
        match Pin::new(&mut self.inner).poll_read(cx, &mut limited) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(())) => {
                let read = limited.filled().len();
                if read == 0 {
                    return Poll::Ready(Err(std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        "bundle file stream ended early",
                    )));
                }

                buf.advance(read);
                self.remaining -= read as u64;
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Err(error)) => Poll::Ready(Err(error)),
        }
    }
}

#[cfg(test)]
mod tests {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use ttsync_contract::manifest::ManifestEntryV2;
    use ttsync_contract::path::SyncPath;

    use super::{ExactSizeReader, write_bundle_to_local_files, write_u32_be};

    fn unique_temp_root() -> std::path::PathBuf {
        use rand::random;
        std::env::temp_dir().join(format!("tauritavern-sync-bundle-{}", random::<u64>()))
    }

    fn entry(path: &str, size_bytes: u64, modified_ms: u64) -> ManifestEntryV2 {
        ManifestEntryV2 {
            path: SyncPath::new(path.to_string()).expect("valid sync path"),
            size_bytes,
            modified_ms,
            content_hash: None,
        }
    }

    async fn write_bundle_frame<W>(writer: &mut W, path: &str, content: &[u8])
    where
        W: tokio::io::AsyncWrite + Unpin,
    {
        write_u32_be(writer, path.len() as u32)
            .await
            .expect("write path len");
        writer.write_all(path.as_bytes()).await.expect("write path");
        writer.write_all(content).await.expect("write content");
    }

    #[tokio::test]
    async fn exact_size_reader_errors_on_short_stream() {
        let (mut reader, mut writer) = tokio::io::duplex(64);
        tokio::spawn(async move {
            writer.write_all(b"abc").await.expect("write");
            drop(writer);
        });

        let mut exact = ExactSizeReader::new(&mut reader, 4);
        let mut buffer = Vec::new();
        let error = exact
            .read_to_end(&mut buffer)
            .await
            .expect_err("must error");
        assert_eq!(error.kind(), std::io::ErrorKind::UnexpectedEof);
    }

    #[tokio::test]
    async fn exact_size_reader_stops_at_exact_length_and_preserves_rest() {
        let (mut reader, mut writer) = tokio::io::duplex(64);
        tokio::spawn(async move {
            writer.write_all(b"abcdEXTRA").await.expect("write");
            drop(writer);
        });

        let mut exact = ExactSizeReader::new(&mut reader, 4);
        let mut buffer = Vec::new();
        exact.read_to_end(&mut buffer).await.expect("read exact");
        assert_eq!(&buffer, b"abcd");

        let mut rest = Vec::new();
        reader.read_to_end(&mut rest).await.expect("read rest");
        assert_eq!(&rest, b"EXTRA");
    }

    #[tokio::test]
    async fn write_bundle_to_local_files_writes_plan_entries() {
        let root = unique_temp_root();
        let _ = tokio::fs::remove_dir_all(&root).await;
        let modified_ms = 1_710_000_000_123u64;
        let path = "default-user/chats/alice.jsonl";

        let (mut reader, mut writer) = tokio::io::duplex(256);
        tokio::spawn(async move {
            write_bundle_frame(&mut writer, path, b"chat").await;
            write_u32_be(&mut writer, 0).await.expect("write end frame");
        });

        let mut progress_paths = Vec::new();
        write_bundle_to_local_files(
            &root,
            vec![entry(path, 4, modified_ms)],
            &mut reader,
            |progress| {
                progress_paths.push(progress.path);
                Ok(())
            },
        )
        .await
        .expect("write bundle");

        assert_eq!(progress_paths, vec![path.to_string()]);
        let bytes = tokio::fs::read(root.join("default-user/chats/alice.jsonl"))
            .await
            .expect("read written file");
        assert_eq!(&bytes, b"chat");

        let metadata = tokio::fs::metadata(root.join("default-user/chats/alice.jsonl"))
            .await
            .expect("metadata");
        let actual = filetime::FileTime::from_last_modification_time(&metadata);
        assert_eq!(actual.unix_seconds(), (modified_ms / 1000) as i64);
        assert_eq!(
            actual.nanoseconds(),
            ((modified_ms % 1000) * 1_000_000) as u32
        );

        tokio::fs::remove_dir_all(root)
            .await
            .expect("remove temp root");
    }

    #[tokio::test]
    async fn write_bundle_to_local_files_errors_on_missing_plan_entry() {
        let root = unique_temp_root();
        let _ = tokio::fs::remove_dir_all(&root).await;

        let (mut reader, mut writer) = tokio::io::duplex(64);
        tokio::spawn(async move {
            write_u32_be(&mut writer, 0).await.expect("write end frame");
        });

        let error = write_bundle_to_local_files(
            &root,
            vec![entry("default-user/chats/missing.jsonl", 1, 1)],
            &mut reader,
            |_| Ok(()),
        )
        .await
        .expect_err("missing entry must error");

        assert!(matches!(
            error.into_error(),
            crate::domain::errors::DomainError::InvalidData(_)
        ));

        let _ = tokio::fs::remove_dir_all(root).await;
    }
}
