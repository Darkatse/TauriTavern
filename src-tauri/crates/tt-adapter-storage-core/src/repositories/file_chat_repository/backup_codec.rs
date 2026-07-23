use std::io::{self, BufReader as StdBufReader, BufWriter, Write};
use std::path::Path;
use std::pin::Pin;
use std::time::SystemTime;

use async_compression::tokio::bufread::ZstdDecoder;
use tokio::fs;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt, BufReader};
use tt_domain::errors::DomainError;

const ZSTD_COMPRESSION_LEVEL: i32 = 1;
const ZSTD_FRAME_HEADER_MAX_SIZE: usize = 18;
const ZSTD_SUFFIX: &str = ".zst";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum BackupFormat {
    RawJsonl,
    Zstd,
}

impl BackupFormat {
    pub(super) fn from_compression_enabled(enabled: bool) -> Self {
        if enabled { Self::Zstd } else { Self::RawJsonl }
    }

    pub(super) fn physical_file_name(self, logical_file_name: &str) -> String {
        match self {
            Self::RawJsonl => logical_file_name.to_string(),
            Self::Zstd => format!("{logical_file_name}{ZSTD_SUFFIX}"),
        }
    }

    pub(super) fn parse_physical_file_name(file_name: &str) -> Option<(Self, String)> {
        if let Some(logical_file_name) = file_name.strip_suffix(ZSTD_SUFFIX)
            && logical_file_name.ends_with(".jsonl")
        {
            return Some((Self::Zstd, logical_file_name.to_string()));
        }

        file_name
            .ends_with(".jsonl")
            .then(|| (Self::RawJsonl, file_name.to_string()))
    }
}

pub(super) type DecodedBackupReader = Pin<Box<dyn AsyncRead + Send>>;

pub(super) async fn open_decoded_backup(
    path: &Path,
    format: BackupFormat,
) -> Result<DecodedBackupReader, DomainError> {
    let file = fs::File::open(path).await.map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to open chat backup {}: {}",
            path.display(),
            error
        ))
    })?;

    Ok(match format {
        BackupFormat::RawJsonl => Box::pin(file),
        BackupFormat::Zstd => Box::pin(ZstdDecoder::new(BufReader::new(file))),
    })
}

pub(super) async fn read_zstd_frame_content_size(path: &Path) -> Result<Option<u64>, DomainError> {
    let mut file = match fs::File::open(path).await {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(DomainError::InternalError(format!(
                "Failed to open chat backup {}: {error}",
                path.display()
            )));
        }
    };
    let mut header = [0; ZSTD_FRAME_HEADER_MAX_SIZE];
    let mut header_len = 0;
    while header_len < header.len() {
        let read = file
            .read(&mut header[header_len..])
            .await
            .map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to read Zstandard frame header {}: {error}",
                    path.display()
                ))
            })?;
        if read == 0 {
            break;
        }
        header_len += read;
    }

    let content_size =
        zstd::zstd_safe::get_frame_content_size(&header[..header_len]).map_err(|error| {
            DomainError::InvalidData(format!(
                "Invalid Zstandard frame header {}: {error}",
                path.display()
            ))
        })?;
    Ok(content_size)
}

pub(super) async fn copy_decoded_backup_reader(
    mut source: DecodedBackupReader,
    source_path: &Path,
    target_path: &Path,
) -> Result<u64, DomainError> {
    let result = async {
        let mut target = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(target_path)
            .await
            .map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to create decoded chat backup {}: {}",
                    target_path.display(),
                    error
                ))
            })?;
        let copied = tokio::io::copy(&mut source, &mut target)
            .await
            .map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to decode chat backup {}: {}",
                    source_path.display(),
                    error
                ))
            })?;
        target.flush().await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to flush decoded chat backup {}: {}",
                target_path.display(),
                error
            ))
        })?;
        Ok::<_, DomainError>(copied)
    }
    .await;

    if result.is_err() {
        let _ = fs::remove_file(target_path).await;
    }
    result
}

pub(super) async fn compress_backup(
    source_path: &Path,
    target_path: &Path,
    expected_source_len: u64,
) -> Result<u64, DomainError> {
    let source_path = source_path.to_path_buf();
    let target_path = target_path.to_path_buf();
    let task_target_path = target_path.clone();
    let result = tokio::task::spawn_blocking(move || {
        compress_backup_blocking(&source_path, &task_target_path, expected_source_len)
    })
    .await
    .map_err(|error| {
        DomainError::InternalError(format!("Chat backup compression task failed: {error}"))
    })
    .and_then(|result| {
        result.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to compress chat backup {}: {}",
                target_path.display(),
                error
            ))
        })
    });

    if result.is_err() {
        let _ = fs::remove_file(&target_path).await;
    }
    result
}

pub(super) async fn decompress_backup(
    source_path: &Path,
    target_path: &Path,
) -> Result<u64, DomainError> {
    let source_path = source_path.to_path_buf();
    let target_path = target_path.to_path_buf();
    let task_source_path = source_path.clone();
    let task_target_path = target_path.clone();
    let result = tokio::task::spawn_blocking(move || {
        decompress_backup_blocking(&task_source_path, &task_target_path)
    })
    .await
    .map_err(|error| {
        DomainError::InternalError(format!("Chat backup decompression task failed: {error}"))
    })
    .and_then(|result| {
        result.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to decompress chat backup {}: {}",
                source_path.display(),
                error
            ))
        })
    });

    if result.is_err() {
        let _ = fs::remove_file(&target_path).await;
    }
    result
}

pub(super) async fn set_backup_modified(
    path: &Path,
    modified: SystemTime,
) -> Result<(), DomainError> {
    let path = path.to_path_buf();
    let task_path = path.clone();
    tokio::task::spawn_blocking(move || {
        let file = std::fs::OpenOptions::new().write(true).open(&task_path)?;
        file.set_times(std::fs::FileTimes::new().set_modified(modified))
    })
    .await
    .map_err(|error| {
        DomainError::InternalError(format!("Chat backup timestamp task failed: {error}"))
    })?
    .map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to preserve chat backup timestamp {}: {}",
            path.display(),
            error
        ))
    })
}

fn compress_backup_blocking(
    source_path: &Path,
    target_path: &Path,
    expected_source_len: u64,
) -> io::Result<u64> {
    let source = std::fs::File::open(source_path)?;
    let target = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(target_path)?;
    let mut source = StdBufReader::with_capacity(zstd::zstd_safe::CCtx::in_size(), source);
    let target = BufWriter::with_capacity(zstd::zstd_safe::CCtx::out_size(), target);
    let mut encoder = zstd::stream::write::Encoder::new(target, ZSTD_COMPRESSION_LEVEL)?;
    encoder.include_checksum(true)?;
    encoder.include_contentsize(true)?;
    encoder.set_pledged_src_size(Some(expected_source_len))?;

    let copied = io::copy(&mut source, &mut encoder)?;
    if copied != expected_source_len {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            format!(
                "chat backup source length changed: expected {expected_source_len}, copied {copied}"
            ),
        ));
    }

    let mut target = encoder.finish()?;
    target.flush()?;
    drop(target);
    std::fs::metadata(target_path).map(|metadata| metadata.len())
}

fn decompress_backup_blocking(source_path: &Path, target_path: &Path) -> io::Result<u64> {
    let source = std::fs::File::open(source_path)?;
    let target = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(target_path)?;
    let mut target = BufWriter::with_capacity(zstd::zstd_safe::DCtx::out_size(), target);
    zstd::stream::copy_decode(source, &mut target)?;
    target.flush()?;
    drop(target);
    std::fs::metadata(target_path).map(|metadata| metadata.len())
}

pub(super) fn materialization_file_name() -> String {
    format!(
        "backup-materialization-{}.jsonl",
        uuid::Uuid::new_v4().simple()
    )
}

pub(super) fn is_materialization_path(path: &Path, staging_dir: &Path) -> bool {
    if path.parent() != Some(staging_dir) {
        return false;
    }
    let Some(file_name) = path.file_name().and_then(|value| value.to_str()) else {
        return false;
    };
    let Some(identifier) = file_name
        .strip_prefix("backup-materialization-")
        .and_then(|value| value.strip_suffix(".jsonl"))
    else {
        return false;
    };
    identifier.len() == 32 && uuid::Uuid::parse_str(identifier).is_ok()
}

pub(super) fn restore_staging_file_name() -> String {
    format!("backup-restore-{}.partial", uuid::Uuid::new_v4().simple())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backup_format_maps_logical_and_physical_names() {
        let logical = "chat_alice_20260722-120000.jsonl";
        assert_eq!(BackupFormat::RawJsonl.physical_file_name(logical), logical);
        assert_eq!(
            BackupFormat::Zstd.physical_file_name(logical),
            format!("{logical}.zst")
        );
        assert_eq!(
            BackupFormat::parse_physical_file_name(&format!("{logical}.zst")),
            Some((BackupFormat::Zstd, logical.to_string()))
        );
    }
}
