use std::pin::Pin;
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};

use crate::domain::errors::DomainError;

pub(crate) const BUNDLE_CONTENT_TYPE: &str = "application/x-ttsync-bundle";
pub(crate) const FEATURE_BUNDLE_V1: &str = "bundle_v1";
pub(crate) const FEATURE_ZSTD_V1: &str = "zstd_v1";

pub(crate) const MAX_BUNDLE_PATH_LEN: u32 = 16 * 1024;

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
) -> Result<(), DomainError>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut buffer = vec![0u8; 64 * 1024];
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

    use super::ExactSizeReader;

    #[tokio::test]
    async fn exact_size_reader_errors_on_short_stream() {
        let (mut reader, mut writer) = tokio::io::duplex(64);
        tokio::spawn(async move {
            writer.write_all(b"abc").await.expect("write");
            drop(writer);
        });

        let mut exact = ExactSizeReader::new(&mut reader, 4);
        let mut buffer = Vec::new();
        let error = exact.read_to_end(&mut buffer).await.expect_err("must error");
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
}
