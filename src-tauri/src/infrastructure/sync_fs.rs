use std::path::{Path, PathBuf};

use tokio::io::{AsyncRead, AsyncWriteExt};

use crate::domain::errors::DomainError;

pub(crate) async fn write_file_atomic(
    path: &Path,
    data: &mut (dyn AsyncRead + Send + Unpin),
    modified_ms: u64,
) -> Result<(), DomainError> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|error| DomainError::InternalError(error.to_string()))?;
    }

    let tmp_path = download_tmp_path(path);
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&tmp_path)
        .await
        .map_err(|error| DomainError::InternalError(error.to_string()))?;

    tokio::io::copy(data, &mut file)
        .await
        .map_err(|error| DomainError::InternalError(error.to_string()))?;

    file.flush()
        .await
        .map_err(|error| DomainError::InternalError(error.to_string()))?;
    drop(file);

    rename_with_retry(&tmp_path, path).await?;
    set_file_modified_ms(path, modified_ms)?;

    Ok(())
}

fn download_tmp_path(path: &Path) -> PathBuf {
    match path.extension() {
        Some(ext) if !ext.is_empty() => {
            let mut tmp_ext = ext.to_os_string();
            tmp_ext.push(".ttsync.tmp");
            path.with_extension(tmp_ext)
        }
        _ => path.with_extension("ttsync.tmp"),
    }
}

async fn rename_with_retry(from: &Path, to: &Path) -> Result<(), DomainError> {
    match tokio::fs::rename(from, to).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            match tokio::fs::remove_file(to).await {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(DomainError::InternalError(error.to_string())),
            }

            tokio::fs::rename(from, to)
                .await
                .map_err(|error| DomainError::InternalError(error.to_string()))
        }
        Err(error) => Err(DomainError::InternalError(error.to_string())),
    }
}

fn set_file_modified_ms(path: &Path, modified_ms: u64) -> Result<(), DomainError> {
    let secs = (modified_ms / 1000) as i64;
    let nanos = ((modified_ms % 1000) * 1_000_000) as u32;
    let mtime = filetime::FileTime::from_unix_time(secs, nanos);

    filetime::set_file_mtime(path, mtime)
        .map_err(|error| DomainError::InternalError(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::{download_tmp_path, write_file_atomic};

    fn unique_temp_root() -> std::path::PathBuf {
        use rand::random;
        std::env::temp_dir().join(format!("tauritavern-sync-fs-{}", random::<u64>()))
    }

    #[test]
    fn download_tmp_path_preserves_original_extension() {
        let pack = std::path::Path::new("pack-abc.pack");
        let idx = std::path::Path::new("pack-abc.idx");
        let rev = std::path::Path::new("pack-abc.rev");

        assert_ne!(download_tmp_path(pack), download_tmp_path(idx));
        assert_ne!(download_tmp_path(pack), download_tmp_path(rev));
        assert_ne!(download_tmp_path(idx), download_tmp_path(rev));

        assert_eq!(
            download_tmp_path(pack).file_name().unwrap(),
            std::ffi::OsStr::new("pack-abc.pack.ttsync.tmp")
        );
        assert_eq!(
            download_tmp_path(idx).file_name().unwrap(),
            std::ffi::OsStr::new("pack-abc.idx.ttsync.tmp")
        );
        assert_eq!(
            download_tmp_path(rev).file_name().unwrap(),
            std::ffi::OsStr::new("pack-abc.rev.ttsync.tmp")
        );
    }

    #[test]
    fn download_tmp_path_avoids_stem_collisions_for_lock_files() {
        let config = std::path::Path::new("config");
        let config_lock = std::path::Path::new("config.lock");

        assert_ne!(download_tmp_path(config), download_tmp_path(config_lock));
        assert_eq!(
            download_tmp_path(config).file_name().unwrap(),
            std::ffi::OsStr::new("config.ttsync.tmp")
        );
        assert_eq!(
            download_tmp_path(config_lock).file_name().unwrap(),
            std::ffi::OsStr::new("config.lock.ttsync.tmp")
        );
    }

    #[tokio::test]
    async fn write_file_atomic_overwrites_and_preserves_mtime() {
        let root = unique_temp_root();
        let _ = tokio::fs::remove_dir_all(&root).await;
        tokio::fs::create_dir_all(&root)
            .await
            .expect("create temp root");

        let source_path = root.join("source.bin");
        tokio::fs::write(&source_path, b"new")
            .await
            .expect("write source");

        let dest_path = root.join("dest.bin");
        tokio::fs::write(&dest_path, b"old")
            .await
            .expect("write existing dest");

        let modified_ms = 1_710_000_000_123u64;
        let mut source = tokio::fs::File::open(&source_path)
            .await
            .expect("open source");

        write_file_atomic(&dest_path, &mut source, modified_ms)
            .await
            .expect("atomic write");

        let bytes = tokio::fs::read(&dest_path).await.expect("read dest");
        assert_eq!(&bytes, b"new");

        let metadata = tokio::fs::metadata(&dest_path).await.expect("metadata");
        let actual = filetime::FileTime::from_last_modification_time(&metadata);

        let expected_secs = (modified_ms / 1000) as i64;
        let expected_nanos = ((modified_ms % 1000) * 1_000_000) as u32;
        assert_eq!(actual.unix_seconds(), expected_secs);
        assert_eq!(actual.nanoseconds(), expected_nanos);

        tokio::fs::remove_dir_all(&root)
            .await
            .expect("remove temp root");
    }
}
