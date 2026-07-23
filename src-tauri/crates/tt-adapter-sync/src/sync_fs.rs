use std::path::{Path, PathBuf};

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use ttsync_contract::path::SyncPath;
use ttsync_core::dataset::prune_boundary_for_path;
use ttsync_core::error::SyncError;

use crate::sync_transfer;

#[derive(Debug)]
pub(crate) struct FileMutationError {
    error: SyncError,
    target_changed: bool,
}

impl FileMutationError {
    fn unchanged(error: SyncError) -> Self {
        Self {
            error,
            target_changed: false,
        }
    }

    fn changed(error: SyncError) -> Self {
        Self {
            error,
            target_changed: true,
        }
    }

    pub(crate) fn target_changed(&self) -> bool {
        self.target_changed
    }

    pub(crate) fn into_error(self) -> SyncError {
        self.error
    }
}

pub(crate) async fn write_file_atomic(
    path: &Path,
    data: &mut (dyn AsyncRead + Send + Unpin),
    modified_ms: u64,
) -> Result<(), FileMutationError> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|error| {
            FileMutationError::unchanged(SyncError::Internal(error.to_string()))
        })?;
    }

    let tmp_path = download_tmp_path(path);
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&tmp_path)
        .await
        .map_err(|error| FileMutationError::unchanged(SyncError::Internal(error.to_string())))?;

    copy_to_file(data, &mut file)
        .await
        .map_err(FileMutationError::unchanged)?;

    file.flush()
        .await
        .map_err(|error| FileMutationError::unchanged(SyncError::Internal(error.to_string())))?;
    drop(file);

    set_file_modified_ms(&tmp_path, modified_ms).map_err(FileMutationError::unchanged)?;
    rename_with_retry(&tmp_path, path).await?;

    Ok(())
}

pub(crate) async fn delete_sync_file(
    sync_root: &Path,
    path: &SyncPath,
) -> Result<(), FileMutationError> {
    let boundary = prune_boundary_for_path(path.as_str()).map_err(FileMutationError::unchanged)?;
    let full_path = sync_transfer::resolve_to_local(sync_root, path);
    let boundary = boundary.map(|boundary| sync_root.join(boundary));

    if let Some(boundary) = boundary.as_deref() {
        let Some(parent) = full_path.parent() else {
            return Err(FileMutationError::unchanged(SyncError::InvalidData(
                format!("sync delete path has no parent: {path}"),
            )));
        };
        if !parent.starts_with(boundary) {
            return Err(FileMutationError::unchanged(SyncError::InvalidData(
                format!(
                    "sync delete path {path} is outside prune boundary {}",
                    boundary.display()
                ),
            )));
        }
    }

    tokio::fs::remove_file(&full_path).await.map_err(|error| {
        FileMutationError::unchanged(SyncError::Io(format!("remove sync file {path}: {error}")))
    })?;

    if let Some(boundary) = boundary.as_deref() {
        prune_fileless_ancestors(&full_path, boundary)
            .await
            .map_err(|error| {
                FileMutationError::changed(SyncError::Io(format!(
                    "prune parents after deleting {path}: {error}"
                )))
            })?;
    }

    Ok(())
}

async fn prune_fileless_ancestors(file_path: &Path, boundary: &Path) -> std::io::Result<()> {
    let mut current = file_path
        .parent()
        .expect("validated sync delete path must have a parent");

    while current != boundary {
        match tokio::fs::remove_dir(current).await {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::DirectoryNotEmpty => {
                if !remove_fileless_tree(current).await? {
                    return Ok(());
                }
            }
            Err(error) => return Err(error),
        }

        current = current
            .parent()
            .expect("validated prune boundary must be an ancestor");
    }

    Ok(())
}

async fn remove_fileless_tree(root: &Path) -> std::io::Result<bool> {
    let mut pending = vec![root.to_path_buf()];
    let mut directories = Vec::new();

    while let Some(directory) = pending.pop() {
        let mut entries = tokio::fs::read_dir(&directory).await?;
        directories.push(directory);

        while let Some(entry) = entries.next_entry().await? {
            if !entry.file_type().await?.is_dir() {
                return Ok(false);
            }
            pending.push(entry.path());
        }
    }

    for directory in directories.into_iter().rev() {
        match tokio::fs::remove_dir(directory).await {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::DirectoryNotEmpty => {
                return Ok(false);
            }
            Err(error) => return Err(error),
        }
    }

    Ok(true)
}

async fn copy_to_file(
    data: &mut (dyn AsyncRead + Send + Unpin),
    file: &mut tokio::fs::File,
) -> Result<(), SyncError> {
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let read = data
            .read(&mut buffer)
            .await
            .map_err(|error| SyncError::Internal(error.to_string()))?;
        if read == 0 {
            return Ok(());
        }
        file.write_all(&buffer[..read])
            .await
            .map_err(|error| SyncError::Internal(error.to_string()))?;
    }
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

async fn rename_with_retry(from: &Path, to: &Path) -> Result<(), FileMutationError> {
    match tokio::fs::rename(from, to).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            match tokio::fs::remove_file(to).await {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(FileMutationError::unchanged(SyncError::Internal(
                        error.to_string(),
                    )));
                }
            }

            tokio::fs::rename(from, to)
                .await
                .map_err(|error| FileMutationError::changed(SyncError::Internal(error.to_string())))
        }
        Err(error) => Err(FileMutationError::unchanged(SyncError::Internal(
            error.to_string(),
        ))),
    }
}

fn set_file_modified_ms(path: &Path, modified_ms: u64) -> Result<(), SyncError> {
    let secs = (modified_ms / 1000) as i64;
    let nanos = ((modified_ms % 1000) * 1_000_000) as u32;
    let mtime = filetime::FileTime::from_unix_time(secs, nanos);

    filetime::set_file_mtime(path, mtime).map_err(|error| SyncError::Internal(error.to_string()))
}

#[cfg(test)]
mod tests {
    use ttsync_contract::path::SyncPath;
    use ttsync_core::error::SyncError;

    use super::{delete_sync_file, download_tmp_path, write_file_atomic};

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

    #[tokio::test]
    async fn delete_sync_file_prunes_fileless_git_tree_at_dataset_boundary() {
        let root = unique_temp_root();
        let extension = root.join("extensions/third-party/example");
        let git = extension.join(".git");

        for directory in [
            git.join("objects/info"),
            git.join("objects/pack"),
            git.join("refs/heads"),
            git.join("refs/tags"),
        ] {
            tokio::fs::create_dir_all(directory)
                .await
                .expect("create git directory");
        }
        tokio::fs::write(extension.join("manifest.json"), b"{}")
            .await
            .expect("write manifest");
        tokio::fs::write(git.join("HEAD"), b"ref: refs/heads/main\n")
            .await
            .expect("write HEAD");
        tokio::fs::write(git.join("config"), b"[core]\n")
            .await
            .expect("write config");

        let head = SyncPath::new("extensions/third-party/example/.git/HEAD".to_string()).unwrap();
        delete_sync_file(&root, &head).await.unwrap();

        assert!(git.join("config").exists());
        assert!(git.join("objects/info").exists());
        assert!(git.join("objects/pack").exists());
        assert!(git.join("refs/heads").exists());
        assert!(git.join("refs/tags").exists());

        let config =
            SyncPath::new("extensions/third-party/example/.git/config".to_string()).unwrap();
        delete_sync_file(&root, &config).await.unwrap();

        assert!(!git.exists());
        assert!(extension.join("manifest.json").exists());

        let manifest =
            SyncPath::new("extensions/third-party/example/manifest.json".to_string()).unwrap();
        delete_sync_file(&root, &manifest).await.unwrap();

        assert!(!extension.exists());
        assert!(root.join("extensions/third-party").exists());

        tokio::fs::remove_dir_all(root)
            .await
            .expect("remove temp root");
    }

    #[tokio::test]
    async fn delete_sync_file_does_not_prune_file_only_dataset_parent() {
        let root = unique_temp_root();
        let default_user = root.join("default-user");
        tokio::fs::create_dir_all(&default_user)
            .await
            .expect("create default user");
        tokio::fs::write(default_user.join("settings.json"), b"{}")
            .await
            .expect("write settings");

        let settings = SyncPath::new("default-user/settings.json".to_string()).unwrap();
        delete_sync_file(&root, &settings).await.unwrap();

        assert!(default_user.exists());
        assert!(!default_user.join("settings.json").exists());

        tokio::fs::remove_dir_all(root)
            .await
            .expect("remove temp root");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn delete_sync_file_does_not_follow_symlinks_in_candidate_tree() {
        use std::os::unix::fs::symlink;

        let root = unique_temp_root();
        let git = root.join("extensions/third-party/example/.git");
        let outside = root.join("outside");
        tokio::fs::create_dir_all(&git)
            .await
            .expect("create git directory");
        tokio::fs::create_dir_all(&outside)
            .await
            .expect("create outside directory");
        tokio::fs::write(git.join("HEAD"), b"ref: refs/heads/main\n")
            .await
            .expect("write HEAD");
        tokio::fs::write(outside.join("keep.txt"), b"keep")
            .await
            .expect("write outside file");
        symlink(&outside, git.join("linked-directory")).expect("create directory symlink");

        let head = SyncPath::new("extensions/third-party/example/.git/HEAD".to_string()).unwrap();
        delete_sync_file(&root, &head).await.unwrap();

        assert!(git.join("linked-directory").symlink_metadata().is_ok());
        assert!(outside.join("keep.txt").exists());
        tokio::fs::remove_dir_all(root)
            .await
            .expect("remove temp root");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn cleanup_failure_after_delete_reports_target_changed() {
        use std::os::unix::fs::PermissionsExt;

        let root = unique_temp_root();
        let extension = root.join("extensions/third-party/example");
        let child = extension.join("child");
        let file = child.join("index.js");
        tokio::fs::create_dir_all(&child)
            .await
            .expect("create child directory");
        tokio::fs::write(&file, b"export {};")
            .await
            .expect("write file");
        tokio::fs::set_permissions(&extension, std::fs::Permissions::from_mode(0o555))
            .await
            .expect("make extension read-only");

        let path =
            SyncPath::new("extensions/third-party/example/child/index.js".to_string()).unwrap();
        let result = delete_sync_file(&root, &path).await;

        tokio::fs::set_permissions(&extension, std::fs::Permissions::from_mode(0o755))
            .await
            .expect("restore extension permissions");
        let error = result.expect_err("directory cleanup must fail");
        assert!(error.target_changed());
        assert!(!file.exists());
        assert!(child.exists());

        tokio::fs::remove_dir_all(root)
            .await
            .expect("remove temp root");
    }

    #[tokio::test]
    async fn delete_sync_file_rejects_unknown_path_before_mutation() {
        let root = unique_temp_root();
        let file = root.join("outside/file.txt");
        tokio::fs::create_dir_all(file.parent().unwrap())
            .await
            .expect("create parent");
        tokio::fs::write(&file, b"keep").await.expect("write file");

        let path = SyncPath::new("outside/file.txt".to_string()).unwrap();
        let error = delete_sync_file(&root, &path).await.unwrap_err();

        assert!(!error.target_changed());
        assert!(matches!(error.into_error(), SyncError::InvalidData(_)));
        assert!(file.exists());

        tokio::fs::remove_dir_all(root)
            .await
            .expect("remove temp root");
    }
}
