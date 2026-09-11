use serde::{Serialize, de::DeserializeOwned};
use std::io;
use std::path::{Path, PathBuf};
use tokio::fs::{self as tokio_fs, create_dir_all};
use tokio::io as tokio_io;
use tt_domain::errors::DomainError;
use uuid::Uuid;

/// Represents the application data directory structure
pub struct DataDirectory {
    root: PathBuf,
    default_user: PathBuf,
    tauritavern: PathBuf,
    extension_sources: PathBuf,
    local_extension_sources: PathBuf,
    global_extension_sources: PathBuf,
    global_extensions: PathBuf,
    characters: PathBuf,
    chats: PathBuf,
    settings: PathBuf,
    user_data: PathBuf,
    default_avatar: PathBuf,
    groups: PathBuf,
    group_chats: PathBuf,
    backups: PathBuf,
}

impl DataDirectory {
    /// Create a new DataDirectory instance
    pub fn new(root: PathBuf) -> Self {
        let default_user = root.join("default-user");
        let tauritavern = root.join("_tauritavern");
        let extension_sources = tauritavern.join("extension-sources");
        let local_extension_sources = extension_sources.join("local");
        let global_extension_sources = extension_sources.join("global");
        let global_extensions = root.join("extensions").join("third-party");
        let characters = default_user.join("characters");
        let chats = default_user.join("chats");
        let settings = default_user.clone();
        let user_data = default_user.clone();
        let default_avatar = default_user
            .join("characters")
            .join("default_Seraphina.png");
        let groups = default_user.join("groups");
        let group_chats = default_user.join("group chats");
        let backups = default_user.join("backups");

        Self {
            root,
            default_user,
            tauritavern,
            extension_sources,
            local_extension_sources,
            global_extension_sources,
            global_extensions,
            characters,
            chats,
            settings,
            user_data,
            default_avatar,
            groups,
            group_chats,
            backups,
        }
    }

    /// Initialize the data directory structure
    pub async fn initialize(&self) -> Result<(), DomainError> {
        tracing::debug!("Initializing data directory at: {:?}", self.root);

        // Create main directories
        self.create_directory(&self.root).await?;
        self.create_directory(&self.default_user).await?;
        self.create_directory(&self.tauritavern).await?;
        self.create_directory(&self.extension_sources).await?;
        self.create_directory(&self.local_extension_sources).await?;
        self.create_directory(&self.global_extension_sources)
            .await?;
        self.create_directory(&self.global_extensions).await?;

        // Create default user subdirectories
        let default_user_dirs = [
            "characters",
            "chats",
            "User Avatars",
            "backgrounds",
            "thumbnails",
            "thumbnails/bg",
            "thumbnails/avatar",
            "thumbnails/persona",
            "worlds",
            "user",
            "user/images",
            "groups",
            "group chats",
            "backups",
            "NovelAI Settings",
            "KoboldAI Settings",
            "OpenAI Settings",
            "TextGen Settings",
            "themes",
            "movingUI",
            "extensions",
            "instruct",
            "context",
            "QuickReplies",
            "assets",
            "user/workflows",
            "user/files",
            "vectors",
            "sysprompt",
            "reasoning",
        ];

        for dir in default_user_dirs.iter() {
            self.create_directory(&self.default_user.join(dir)).await?;
        }

        tracing::debug!("Data directory initialized successfully");
        Ok(())
    }

    /// Create a directory if it doesn't exist
    async fn create_directory(&self, path: &Path) -> Result<(), DomainError> {
        if !path.exists() {
            tracing::info!("Creating directory: {:?}", path);
            create_dir_all(path).await.map_err(|e| {
                tracing::error!("Failed to create directory {:?}: {}", path, e);
                DomainError::InternalError(format!("Failed to create directory: {}", e))
            })?;
        }
        Ok(())
    }

    /// Get the default user directory
    pub fn default_user(&self) -> &Path {
        &self.default_user
    }

    /// Get the data root directory
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Get the extension source state root directory
    pub fn extension_sources(&self) -> &Path {
        &self.extension_sources
    }

    /// Get the global third-party extensions directory
    pub fn global_extensions(&self) -> &Path {
        &self.global_extensions
    }

    /// Get the characters directory
    pub fn characters(&self) -> &Path {
        &self.characters
    }

    /// Get the chats directory
    pub fn chats(&self) -> &Path {
        &self.chats
    }

    /// Get the settings directory
    pub fn settings(&self) -> &Path {
        &self.settings
    }

    /// Get the user data directory
    pub fn user_data(&self) -> &Path {
        &self.user_data
    }

    /// Get the default avatar path
    pub fn default_avatar(&self) -> &Path {
        &self.default_avatar
    }

    /// Get the groups directory
    pub fn groups(&self) -> &Path {
        &self.groups
    }

    /// Get the group chats directory
    pub fn group_chats(&self) -> &Path {
        &self.group_chats
    }

    /// Get the chat backups directory
    pub fn backups(&self) -> &Path {
        &self.backups
    }
}

/// Read a JSON file and deserialize it
///
/// File I/O and UTF-8 validation run together on Tokio's blocking pool.
pub async fn read_json_file<T: DeserializeOwned>(path: &Path) -> Result<T, DomainError> {
    tracing::debug!("Reading JSON file: {:?}", path);

    let read_path = path.to_owned();
    let contents = tokio::task::spawn_blocking(move || {
        let bytes = std::fs::read(&read_path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                DomainError::NotFound(format!("File not found: {}", read_path.display()))
            } else {
                DomainError::InternalError(format!(
                    "Failed to read {}: {error}",
                    read_path.display()
                ))
            }
        })?;
        String::from_utf8(bytes).map_err(|error| {
            DomainError::InvalidData(format!("Invalid UTF-8 in {}: {error}", read_path.display()))
        })
    })
    .await
    .map_err(|error| {
        DomainError::InternalError(format!(
            "JSON read task failed for {}: {error}",
            path.display()
        ))
    })??;

    serde_json::from_str(&contents).map_err(|error| {
        DomainError::InvalidData(format!("Invalid JSON in {}: {error}", path.display()))
    })
}

/// Generate a unique temporary file path adjacent to `target_path`.
///
/// The bounded, target-independent file name remains valid even when the target
/// already uses the full file-system component length.
pub fn unique_temp_path(target_path: &Path) -> PathBuf {
    target_path.with_file_name(format!(".tauritavern-{}.tmp", Uuid::new_v4().simple()))
}

async fn optional_metadata(path: &Path) -> Result<Option<std::fs::Metadata>, DomainError> {
    match tokio_fs::symlink_metadata(path).await {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(DomainError::InternalError(format!(
            "Failed to read file metadata {:?}: {}",
            path, error
        ))),
    }
}

struct CopyFileNoReplaceError {
    error: std::io::Error,
    target_created: bool,
}

async fn copy_file_no_replace(
    source_path: &Path,
    target_path: &Path,
) -> Result<(), CopyFileNoReplaceError> {
    let mut source =
        tokio_fs::File::open(source_path)
            .await
            .map_err(|error| CopyFileNoReplaceError {
                error,
                target_created: false,
            })?;
    let mut target = tokio_fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target_path)
        .await
        .map_err(|error| CopyFileNoReplaceError {
            error,
            target_created: false,
        })?;
    tokio_io::copy(&mut source, &mut target)
        .await
        .map_err(|error| CopyFileNoReplaceError {
            error,
            target_created: true,
        })?;
    Ok(())
}

/// Move a file without replacing an existing target.
///
/// The operation uses `rename` first and falls back to copy/remove when the
/// storage backend cannot provide reliable rename semantics, which is common on
/// Android external app storage. Ambiguous post-error states fail fast.
pub async fn move_file_no_replace_with_fallback(
    source_path: &Path,
    target_path: &Path,
) -> Result<(), DomainError> {
    let Some(source_metadata) = optional_metadata(source_path).await? else {
        return Err(DomainError::NotFound(format!(
            "Source file not found: {}",
            source_path.display()
        )));
    };
    if !source_metadata.is_file() {
        return Err(DomainError::InvalidData(format!(
            "Source path is not a file: {}",
            source_path.display()
        )));
    }
    if optional_metadata(target_path).await?.is_some() {
        return Err(DomainError::InvalidData(format!(
            "Target file already exists: {}",
            target_path.display()
        )));
    }

    if let Some(parent) = target_path.parent() {
        create_dir_all(parent).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to create target parent directory {:?}: {}",
                parent, error
            ))
        })?;
    }

    match tokio_fs::rename(source_path, target_path).await {
        Ok(()) => Ok(()),
        Err(rename_error) => {
            let source_after = optional_metadata(source_path).await?;
            let target_after = optional_metadata(target_path).await?;

            match (source_after, target_after) {
                (None, Some(target_metadata)) if target_metadata.is_file() => {
                    tracing::warn!(
                        "Rename reported an error after moving file {:?} -> {:?}: {}",
                        source_path,
                        target_path,
                        rename_error
                    );
                    Ok(())
                }
                (Some(source_metadata), None) if source_metadata.is_file() => {
                    tracing::warn!(
                        "Rename failed while moving file {:?} -> {:?}: {}. Falling back to copy/remove.",
                        source_path,
                        target_path,
                        rename_error
                    );
                    if let Err(copy_error) = copy_file_no_replace(source_path, target_path).await {
                        if !copy_error.target_created {
                            return Err(DomainError::InternalError(format!(
                                "Failed to move file {:?} -> {:?}. Rename error: {}. Copy fallback error: {}",
                                source_path, target_path, rename_error, copy_error.error
                            )));
                        }

                        return Err(match tokio_fs::remove_file(target_path).await {
                            Ok(()) => DomainError::InternalError(format!(
                                "Failed to move file {:?} -> {:?}. Rename error: {}. Copy fallback error: {}",
                                source_path, target_path, rename_error, copy_error.error
                            )),
                            Err(cleanup_error)
                                if cleanup_error.kind() == std::io::ErrorKind::NotFound =>
                            {
                                DomainError::InternalError(format!(
                                    "Failed to move file {:?} -> {:?}. Rename error: {}. Copy fallback error: {}",
                                    source_path, target_path, rename_error, copy_error.error
                                ))
                            }
                            Err(cleanup_error) => DomainError::InternalError(format!(
                                "Failed to move file {:?} -> {:?}. Rename error: {}. Copy fallback error: {}. Failed to remove partial target: {}",
                                source_path,
                                target_path,
                                rename_error,
                                copy_error.error,
                                cleanup_error
                            )),
                        });
                    }

                    match tokio_fs::remove_file(source_path).await {
                        Ok(()) => Ok(()),
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                        Err(error) => Err(DomainError::InternalError(format!(
                            "Copied file {:?} -> {:?}, but failed to remove source file: {}",
                            source_path, target_path, error
                        ))),
                    }
                }
                (Some(_), Some(_)) => Err(DomainError::InternalError(format!(
                    "Failed to move file {:?} -> {:?}. Rename error: {}. Source and target both exist after failure.",
                    source_path, target_path, rename_error
                ))),
                (None, None) => Err(DomainError::InternalError(format!(
                    "Failed to move file {:?} -> {:?}. Rename error: {}. Source and target are both missing after failure.",
                    source_path, target_path, rename_error
                ))),
                (Some(_), None) => Err(DomainError::InvalidData(format!(
                    "Source path is not a file after failed move: {}",
                    source_path.display()
                ))),
                (None, Some(_)) => Err(DomainError::InvalidData(format!(
                    "Target path is not a file after failed move: {}",
                    target_path.display()
                ))),
            }
        }
    }
}

/// Atomically publish a completed temporary file on the same filesystem.
/// The caller must flush its writer first. Rename failures never fall back to
/// overwriting the target in place or guessing that the operation succeeded.
pub async fn replace_file(temp_path: &Path, target_path: &Path) -> Result<(), DomainError> {
    tokio_fs::rename(temp_path, target_path)
        .await
        .map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to replace {} with {}: {error}",
                target_path.display(),
                temp_path.display()
            ))
        })
}

/// Blocking variant for startup and blocking file writers.
pub fn replace_file_blocking(temp_path: &Path, target_path: &Path) -> Result<(), DomainError> {
    std::fs::rename(temp_path, target_path).map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to replace {} with {}: {error}",
            target_path.display(),
            temp_path.display()
        ))
    })
}

/// Sync a completed temporary file before atomically publishing it.
/// Pass the original writing handle, after flushing any buffered writer. Reopening
/// the path can miss earlier writeback errors on Linux. Sync failures leave the target intact.
/// ponytail: file data is synced; add platform-specific directory sync if callers
/// require crash durability of the subsequent rename as well.
pub async fn persist_file(
    file: tokio_fs::File,
    temp_path: &Path,
    target_path: &Path,
) -> Result<(), DomainError> {
    let file = file.into_std().await;
    let temp_path = temp_path.to_owned();
    let target_path = target_path.to_owned();
    tokio::task::spawn_blocking(move || persist_file_blocking(file, &temp_path, &target_path))
        .await
        .map_err(|error| DomainError::InternalError(format!("File publish task failed: {error}")))?
}

/// Blocking variant of [`persist_file`], consuming the original writing handle.
pub fn persist_file_blocking(
    file: std::fs::File,
    temp_path: &Path,
    target_path: &Path,
) -> Result<(), DomainError> {
    file.sync_all().map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to sync staged file {} for {}: {error}",
            temp_path.display(),
            target_path.display()
        ))
    })?;
    drop(file);
    replace_file_blocking(temp_path, target_path)
}

/// Write and sync a JSON file during startup, before async services exist.
pub fn persist_json_file_blocking<T: Serialize + ?Sized>(
    path: &Path,
    data: &T,
) -> Result<(), DomainError> {
    use std::io::Write;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            DomainError::InternalError(format!("Failed to create {}: {error}", parent.display()))
        })?;
    }
    let json = serde_json::to_vec_pretty(data).map_err(|error| {
        DomainError::InvalidData(format!(
            "Failed to serialize JSON for {}: {error}",
            path.display()
        ))
    })?;
    let temp_path = unique_temp_path(path);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp_path)
        .map_err(|error| {
            DomainError::InternalError(format!("Failed to create {}: {error}", temp_path.display()))
        })?;
    let result = (|| {
        file.write_all(&json).map_err(|error| {
            DomainError::InternalError(format!("Failed to write {}: {error}", temp_path.display()))
        })?;
        persist_file_blocking(file, &temp_path, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp_path);
    }
    result
}

/// Write JSON with atomic replacement, without forcing a disk sync.
pub async fn write_json_file<T: Serialize + ?Sized>(
    path: &Path,
    data: &T,
) -> Result<(), DomainError> {
    write_json_file_impl(path, data, false).await
}

/// Write JSON and sync the completed file before publishing it.
pub async fn persist_json_file<T: Serialize + ?Sized>(
    path: &Path,
    data: &T,
) -> Result<(), DomainError> {
    write_json_file_impl(path, data, true).await
}

async fn write_json_file_impl<T: Serialize + ?Sized>(
    path: &Path,
    data: &T,
    sync_to_disk: bool,
) -> Result<(), DomainError> {
    use tokio::io::AsyncWriteExt;

    if let Some(parent) = path.parent() {
        create_dir_all(parent).await.map_err(|error| {
            DomainError::InternalError(format!("Failed to create {}: {error}", parent.display()))
        })?;
    }
    let json = serde_json::to_vec_pretty(data).map_err(|error| {
        DomainError::InvalidData(format!(
            "Failed to serialize JSON for {}: {error}",
            path.display()
        ))
    })?;
    let temp_path = unique_temp_path(path);
    let mut file = tokio_fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp_path)
        .await
        .map_err(|error| {
            DomainError::InternalError(format!("Failed to create {}: {error}", temp_path.display()))
        })?;
    let result = async {
        let written = async {
            file.write_all(&json).await?;
            file.flush().await
        }
        .await;
        written.map_err(|error: io::Error| {
            DomainError::InternalError(format!("Failed to write {}: {error}", temp_path.display()))
        })?;
        if sync_to_disk {
            persist_file(file, &temp_path, path).await
        } else {
            drop(file);
            replace_file(&temp_path, path).await
        }
    }
    .await;
    if result.is_err() {
        let _ = tokio_fs::remove_file(&temp_path).await;
    }
    result
}

/// List files in a directory with a specific extension
///
/// This is an async function that lists all files in a directory with a specific extension.
/// It uses tokio's async file I/O operations for better performance and non-blocking behavior.
pub async fn list_files_with_extension(
    dir: &Path,
    extension: &str,
) -> Result<Vec<PathBuf>, DomainError> {
    tracing::debug!(
        "Listing files with extension '{}' in directory: {:?}",
        extension,
        dir
    );

    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut entries = tokio_fs::read_dir(dir).await.map_err(|e| {
        tracing::error!("Failed to read directory {:?}: {}", dir, e);
        DomainError::InternalError(format!("Failed to read directory: {}", e))
    })?;

    let mut files = Vec::new();

    // Process each entry in the directory
    while let Some(entry) = entries.next_entry().await.map_err(|e| {
        tracing::error!("Failed to read directory entry: {}", e);
        DomainError::InternalError(format!("Failed to read directory entry: {}", e))
    })? {
        let path = entry.path();

        // Check if it's a file with the specified extension
        if path.is_file() && path.extension().is_some_and(|ext| ext == extension) {
            files.push(path);
        }
    }

    Ok(files)
}

/// Delete a file
///
/// This is an async function that deletes a file from the filesystem.
/// It uses tokio's async file I/O operations for better performance and non-blocking behavior.
pub async fn delete_file(path: &Path) -> Result<(), DomainError> {
    tracing::debug!("Deleting file: {:?}", path);

    if !path.exists() {
        return Ok(());
    }

    tokio_fs::remove_file(path).await.map_err(|e| {
        tracing::error!("Failed to delete file {:?}: {}", path, e);
        DomainError::InternalError(format!("Failed to delete file: {}", e))
    })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use serde_json::{Value, json};
    use std::io::Read;

    fn unique_temp_root() -> PathBuf {
        use rand::random;
        std::env::temp_dir().join(format!("tauritavern-file-system-{}", random::<u64>()))
    }

    #[tokio::test]
    async fn replace_file_overwrites_existing_file() {
        let root = unique_temp_root();
        let _ = tokio_fs::remove_dir_all(&root).await;
        tokio_fs::create_dir_all(&root)
            .await
            .expect("create temp root");

        let target = root.join("target.txt");
        tokio_fs::write(&target, b"old")
            .await
            .expect("write existing target");

        let temp = root.join("temp.txt");
        tokio_fs::write(&temp, b"new")
            .await
            .expect("write temp file");

        replace_file(&temp, &target).await.expect("replace file");

        let bytes = tokio_fs::read(&target).await.expect("read target");
        assert_eq!(&bytes, b"new");
        assert!(!temp.exists(), "temp file should be moved/removed");

        tokio_fs::remove_dir_all(&root)
            .await
            .expect("remove temp root");
    }

    #[tokio::test]
    async fn persist_file_keeps_existing_target_when_stage_is_missing() {
        use tokio::io::AsyncWriteExt;

        let root = unique_temp_root();
        tokio_fs::create_dir_all(&root).await.unwrap();
        let temp = root.join("missing.txt");
        let target = root.join("target.txt");
        tokio_fs::write(&target, b"old").await.unwrap();
        let mut file = tokio_fs::File::create(&temp).await.unwrap();
        file.write_all(b"new").await.unwrap();
        file.flush().await.unwrap();
        tokio_fs::remove_file(&temp).await.unwrap();

        persist_file(file, &temp, &target)
            .await
            .expect_err("missing stage must fail");
        assert_eq!(tokio_fs::read(&target).await.unwrap(), b"old");

        tokio_fs::remove_dir_all(root).await.unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn persist_file_can_publish_read_only_stage() {
        use std::os::unix::fs::PermissionsExt;
        use tokio::io::AsyncWriteExt;

        let root = unique_temp_root();
        tokio_fs::create_dir_all(&root).await.unwrap();
        let temp = root.join("stage.txt");
        let target = root.join("target.txt");
        let mut file = tokio_fs::File::create(&temp).await.unwrap();
        file.write_all(b"new").await.unwrap();
        file.flush().await.unwrap();
        file.set_permissions(std::fs::Permissions::from_mode(0o400))
            .await
            .unwrap();

        persist_file(file, &temp, &target).await.unwrap();
        assert_eq!(tokio_fs::read(&target).await.unwrap(), b"new");
        assert!(!temp.exists());

        tokio_fs::remove_dir_all(root).await.unwrap();
    }

    #[tokio::test]
    async fn replace_file_rejects_directory_target_after_failed_rename() {
        let root = unique_temp_root();
        let _ = tokio_fs::remove_dir_all(&root).await;
        tokio_fs::create_dir_all(&root)
            .await
            .expect("create temp root");

        let temp = root.join("temp.txt");
        let target = root.join("target");
        tokio_fs::write(&temp, b"new")
            .await
            .expect("write temp file");
        tokio_fs::create_dir_all(&target)
            .await
            .expect("create target directory");

        replace_file(&temp, &target)
            .await
            .expect_err("directory target should fail");

        assert!(temp.exists(), "temp file should remain for diagnosis");
        assert!(target.is_dir(), "target directory should remain intact");

        tokio_fs::remove_dir_all(&root)
            .await
            .expect("remove temp root");
    }

    #[tokio::test]
    async fn move_file_no_replace_with_fallback_moves_file() {
        let root = unique_temp_root();
        let _ = tokio_fs::remove_dir_all(&root).await;
        tokio_fs::create_dir_all(&root)
            .await
            .expect("create temp root");

        let source = root.join("source.txt");
        let target = root.join("nested").join("target.txt");
        tokio_fs::write(&source, b"payload")
            .await
            .expect("write source file");

        move_file_no_replace_with_fallback(&source, &target)
            .await
            .expect("move file");

        let bytes = tokio_fs::read(&target).await.expect("read target");
        assert_eq!(&bytes, b"payload");
        assert!(!source.exists(), "source file should be moved/removed");

        tokio_fs::remove_dir_all(&root)
            .await
            .expect("remove temp root");
    }

    #[tokio::test]
    async fn move_file_no_replace_with_fallback_rejects_existing_target() {
        let root = unique_temp_root();
        let _ = tokio_fs::remove_dir_all(&root).await;
        tokio_fs::create_dir_all(&root)
            .await
            .expect("create temp root");

        let source = root.join("source.txt");
        let target = root.join("target.txt");
        tokio_fs::write(&source, b"source")
            .await
            .expect("write source file");
        tokio_fs::write(&target, b"target")
            .await
            .expect("write target file");

        let error = move_file_no_replace_with_fallback(&source, &target)
            .await
            .expect_err("existing target should fail");

        assert!(
            matches!(error, DomainError::InvalidData(message) if message.contains("Target file already exists"))
        );
        assert_eq!(
            tokio_fs::read(&source).await.expect("read source"),
            b"source"
        );
        assert_eq!(
            tokio_fs::read(&target).await.expect("read target"),
            b"target"
        );

        tokio_fs::remove_dir_all(&root)
            .await
            .expect("remove temp root");
    }

    #[tokio::test]
    async fn copy_file_no_replace_keeps_existing_target() {
        let root = unique_temp_root();
        let _ = tokio_fs::remove_dir_all(&root).await;
        tokio_fs::create_dir_all(&root)
            .await
            .expect("create temp root");

        let source = root.join("source.txt");
        let target = root.join("target.txt");
        tokio_fs::write(&source, b"source")
            .await
            .expect("write source file");
        tokio_fs::write(&target, b"target")
            .await
            .expect("write target file");

        let error = copy_file_no_replace(&source, &target)
            .await
            .expect_err("existing target should fail");

        assert!(!error.target_created);
        assert_eq!(
            tokio_fs::read(&source).await.expect("read source"),
            b"source"
        );
        assert_eq!(
            tokio_fs::read(&target).await.expect("read target"),
            b"target"
        );

        tokio_fs::remove_dir_all(&root)
            .await
            .expect("remove temp root");
    }

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    #[test]
    fn replace_file_blocking_rejects_directory_target_after_failed_rename() {
        let root = unique_temp_root();
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("create temp root");

        let temp = root.join("temp.txt");
        let target = root.join("target");
        std::fs::write(&temp, b"new").expect("write temp file");
        std::fs::create_dir_all(&target).expect("create target directory");

        replace_file_blocking(&temp, &target).expect_err("directory target should fail");

        assert!(temp.exists(), "temp file should remain for diagnosis");
        assert!(target.is_dir(), "target directory should remain intact");

        std::fs::remove_dir_all(&root).expect("remove temp root");
    }

    #[tokio::test]
    async fn write_json_file_creates_parent_directory_and_round_trips() {
        #[derive(Debug, Serialize, Deserialize, PartialEq)]
        struct Sample {
            version: u32,
            name: String,
        }

        let root = unique_temp_root();
        let _ = tokio_fs::remove_dir_all(&root).await;

        let path = root.join("a").join("b").join("c").join("settings.json");
        let sample = Sample {
            version: 1,
            name: "demo".to_string(),
        };

        write_json_file(&path, &sample)
            .await
            .expect("write json file");

        let loaded: Sample = read_json_file(&path).await.expect("read json file");
        assert_eq!(loaded, sample);

        tokio_fs::remove_dir_all(&root)
            .await
            .expect("remove temp root");
    }

    #[tokio::test]
    async fn write_json_file_replaces_target_entry_so_open_handles_keep_old_bytes() {
        let root = unique_temp_root();
        let _ = tokio_fs::remove_dir_all(&root).await;
        tokio_fs::create_dir_all(&root)
            .await
            .expect("create temp root");

        let path = root.join("settings.json");
        persist_json_file(&path, &json!({ "version": 1u32, "payload": "old" }))
            .await
            .expect("write initial json");

        // Keep the original file handle open across the rewrite. If the write is implemented
        // as temp+rename replacement, the open handle continues to point at the old inode.
        let mut old_handle = std::fs::File::open(&path).expect("open old handle");

        persist_json_file(&path, &json!({ "version": 2u32, "payload": "new" }))
            .await
            .expect("write updated json");

        let mut old_contents = String::new();
        old_handle
            .read_to_string(&mut old_contents)
            .expect("read from old handle");

        let on_disk_contents = tokio_fs::read_to_string(&path)
            .await
            .expect("read via path");

        let old_json: Value = serde_json::from_str(&old_contents).expect("parse old json");
        let new_json: Value = serde_json::from_str(&on_disk_contents).expect("parse new json");

        assert_eq!(old_json.get("version"), Some(&json!(1u32)));
        assert_eq!(new_json.get("version"), Some(&json!(2u32)));

        tokio_fs::remove_dir_all(&root)
            .await
            .expect("remove temp root");
    }

    #[tokio::test]
    async fn write_json_file_does_not_modify_target_on_serialization_error() {
        struct FailingSerialize;

        impl Serialize for FailingSerialize {
            fn serialize<S>(&self, _serializer: S) -> Result<S::Ok, S::Error>
            where
                S: serde::Serializer,
            {
                Err(serde::ser::Error::custom("intentional serialization error"))
            }
        }

        let root = unique_temp_root();
        let _ = tokio_fs::remove_dir_all(&root).await;
        tokio_fs::create_dir_all(&root)
            .await
            .expect("create temp root");

        let path = root.join("settings.json");
        write_json_file(&path, &json!({ "ok": true }))
            .await
            .expect("write initial json");

        let before: Value = read_json_file(&path).await.expect("read initial json");

        let err = write_json_file(&path, &FailingSerialize)
            .await
            .expect_err("expected serialization failure");

        match err {
            DomainError::InvalidData(_) => {}
            other => panic!("expected InvalidData, got {:?}", other),
        }

        let after: Value = read_json_file(&path).await.expect("read json after error");
        assert_eq!(after, before);

        tokio_fs::remove_dir_all(&root)
            .await
            .expect("remove temp root");
    }

    #[tokio::test]
    async fn read_json_file_returns_invalid_data_for_malformed_json() {
        let root = unique_temp_root();
        let _ = tokio_fs::remove_dir_all(&root).await;
        tokio_fs::create_dir_all(&root)
            .await
            .expect("create temp root");

        let path = root.join("bad.json");
        tokio_fs::write(&path, b"{")
            .await
            .expect("write malformed json");

        let err = read_json_file::<Value>(&path)
            .await
            .expect_err("expected invalid json error");

        match err {
            DomainError::InvalidData(_) => {}
            other => panic!("expected InvalidData, got {:?}", other),
        }

        tokio_fs::remove_dir_all(&root)
            .await
            .expect("remove temp root");
    }
}
