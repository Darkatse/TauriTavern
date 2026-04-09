use crate::domain::errors::DomainError;
use crate::infrastructure::logging::logger;
use serde::{Serialize, de::DeserializeOwned};
use std::io;
use std::path::{Path, PathBuf};
use tokio::fs::{self as tokio_fs, create_dir_all, read_to_string};
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
/// This is an async function that reads a JSON file from disk and deserializes it
/// into the specified type. It uses tokio's async file I/O operations for better
/// performance and non-blocking behavior.
pub async fn read_json_file<T: DeserializeOwned>(path: &Path) -> Result<T, DomainError> {
    logger::debug(&format!("Reading JSON file: {:?}", path));

    // Use tokio's async file operations
    let contents = read_to_string(path).await.map_err(|e| {
        logger::error(&format!("Failed to read file {:?}: {}", path, e));
        if e.kind() == std::io::ErrorKind::NotFound {
            DomainError::NotFound(format!("File not found: {}", path.display()))
        } else {
            DomainError::InternalError(format!("Failed to read file: {}", e))
        }
    })?;

    serde_json::from_str(&contents).map_err(|e| {
        logger::error(&format!("Failed to parse JSON from file {:?}: {}", path, e));
        DomainError::InvalidData(format!("Invalid JSON: {}", e))
    })
}

/// Generate a unique temporary file path adjacent to `target_path`.
///
/// The returned file name is based on the target file name (or `fallback_file_name` if missing)
/// and includes a random UUID to avoid collisions under concurrent writes.
pub fn unique_temp_path(target_path: &Path, fallback_file_name: &str) -> PathBuf {
    let file_name = target_path
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback_file_name);

    target_path.with_file_name(format!("{}.{}.tmp", file_name, Uuid::new_v4()))
}

/// Replace a file using `rename`, with a copy/remove fallback for storage backends
/// where rename is unreliable (notably Android external app storage).
pub async fn replace_file_with_fallback(
    temp_path: &Path,
    target_path: &Path,
) -> Result<(), DomainError> {
    match tokio_fs::rename(temp_path, target_path).await {
        Ok(()) => Ok(()),
        Err(rename_error) => {
            logger::warn(&format!(
                "Rename failed while replacing file {:?} -> {:?}: {}. Falling back to copy/remove.",
                temp_path, target_path, rename_error
            ));

            if let Some(parent) = target_path.parent() {
                create_dir_all(parent).await.map_err(|error| {
                    DomainError::InternalError(format!(
                        "Failed to create target parent directory {:?}: {}",
                        parent, error
                    ))
                })?;
            }

            let copy_result = tokio_fs::copy(temp_path, target_path).await;
            if let Err(copy_error) = copy_result {
                return Err(DomainError::InternalError(format!(
                    "Failed to replace file {:?} -> {:?}. Rename error: {}. Copy fallback error: {}",
                    temp_path, target_path, rename_error, copy_error
                )));
            }

            match tokio_fs::remove_file(temp_path).await {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => {
                    logger::warn(&format!(
                        "Copied file {:?} -> {:?}, but failed to remove temp file: {}",
                        temp_path, target_path, error
                    ));
                }
            }

            Ok(())
        }
    }
}

/// Synchronous variant of `replace_file_with_fallback` for startup/runtime code paths
/// that cannot rely on Tokio being available yet.
pub fn replace_file_with_fallback_sync(
    temp_path: &Path,
    target_path: &Path,
) -> Result<(), DomainError> {
    match std::fs::rename(temp_path, target_path) {
        Ok(()) => Ok(()),
        Err(rename_error) => {
            logger::warn(&format!(
                "Rename failed while replacing file {:?} -> {:?}: {}. Falling back to copy/remove.",
                temp_path, target_path, rename_error
            ));

            if let Some(parent) = target_path.parent() {
                std::fs::create_dir_all(parent).map_err(|error| {
                    DomainError::InternalError(format!(
                        "Failed to create target parent directory {:?}: {}",
                        parent, error
                    ))
                })?;
            }

            std::fs::copy(temp_path, target_path).map_err(|copy_error| {
                DomainError::InternalError(format!(
                    "Failed to replace file {:?} -> {:?}. Rename error: {}. Copy fallback error: {}",
                    temp_path, target_path, rename_error, copy_error
                ))
            })?;

            match std::fs::remove_file(temp_path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => {
                    logger::warn(&format!(
                        "Copied file {:?} -> {:?}, but failed to remove temp file: {}",
                        temp_path, target_path, error
                    ));
                }
            }

            Ok(())
        }
    }
}

/// Write a JSON file
///
/// This is an async function that serializes data to JSON and writes it to a file.
/// It uses tokio's async file I/O operations for better performance and non-blocking behavior.
pub async fn write_json_file<T: Serialize + ?Sized>(
    path: &Path,
    data: &T,
) -> Result<(), DomainError> {
    logger::debug(&format!("Writing JSON file: {:?}", path));

    // Ensure the parent directory exists
    if let Some(parent) = path.parent() {
        create_dir_all(parent).await.map_err(|e| {
            logger::error(&format!(
                "Failed to create parent directory for {:?}: {}",
                path, e
            ));
            DomainError::InternalError(format!("Failed to create directory: {}", e))
        })?;
    }

    // Serialize data to JSON
    let json = serde_json::to_string_pretty(data).map_err(|e| {
        logger::error(&format!(
            "Failed to serialize to JSON for file {:?}: {}",
            path, e
        ));
        DomainError::InvalidData(format!("Failed to serialize to JSON: {}", e))
    })?;

    // Write to a unique temp file adjacent to the target, then replace the target.
    //
    // This avoids truncating the target file if the process is interrupted mid-write.
    let temp_path = unique_temp_path(path, "data.json");
    tokio_fs::write(&temp_path, json.as_bytes())
        .await
        .map_err(|e| {
            logger::error(&format!(
                "Failed to write JSON temp file {:?} -> {:?}: {}",
                temp_path, path, e
            ));
            DomainError::InternalError(format!("Failed to write file: {}", e))
        })?;
    replace_file_with_fallback(&temp_path, path).await?;

    Ok(())
}

/// List files in a directory with a specific extension
///
/// This is an async function that lists all files in a directory with a specific extension.
/// It uses tokio's async file I/O operations for better performance and non-blocking behavior.
pub async fn list_files_with_extension(
    dir: &Path,
    extension: &str,
) -> Result<Vec<PathBuf>, DomainError> {
    logger::debug(&format!(
        "Listing files with extension '{}' in directory: {:?}",
        extension, dir
    ));

    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut entries = tokio_fs::read_dir(dir).await.map_err(|e| {
        logger::error(&format!("Failed to read directory {:?}: {}", dir, e));
        DomainError::InternalError(format!("Failed to read directory: {}", e))
    })?;

    let mut files = Vec::new();

    // Process each entry in the directory
    while let Some(entry) = entries.next_entry().await.map_err(|e| {
        logger::error(&format!("Failed to read directory entry: {}", e));
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
    logger::debug(&format!("Deleting file: {:?}", path));

    if !path.exists() {
        return Ok(());
    }

    tokio_fs::remove_file(path).await.map_err(|e| {
        logger::error(&format!("Failed to delete file {:?}: {}", path, e));
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

    #[test]
    fn unique_temp_path_is_unique_and_adjacent() {
        let root = unique_temp_root();
        let target = root.join("settings.json");

        let a = unique_temp_path(&target, "fallback.json");
        let b = unique_temp_path(&target, "fallback.json");

        assert_ne!(a, b);
        assert_eq!(a.parent(), target.parent());
        assert_eq!(b.parent(), target.parent());

        let a_name = a.file_name().and_then(|value| value.to_str()).unwrap_or("");
        assert!(a_name.starts_with("settings.json."));
        assert!(a_name.ends_with(".tmp"));
    }

    #[tokio::test]
    async fn replace_file_with_fallback_overwrites_existing_file() {
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

        replace_file_with_fallback(&temp, &target)
            .await
            .expect("replace file");

        let bytes = tokio_fs::read(&target).await.expect("read target");
        assert_eq!(&bytes, b"new");
        assert!(!temp.exists(), "temp file should be moved/removed");

        tokio_fs::remove_dir_all(&root)
            .await
            .expect("remove temp root");
    }

    #[test]
    fn replace_file_with_fallback_sync_overwrites_existing_file() {
        let root = unique_temp_root();
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("create temp root");

        let target = root.join("target.txt");
        std::fs::write(&target, b"old").expect("write existing target");

        let temp = root.join("temp.txt");
        std::fs::write(&temp, b"new").expect("write temp file");

        replace_file_with_fallback_sync(&temp, &target).expect("replace file");

        let bytes = std::fs::read(&target).expect("read target");
        assert_eq!(&bytes, b"new");
        assert!(!temp.exists(), "temp file should be moved/removed");

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
        write_json_file(&path, &json!({ "version": 1u32, "payload": "old" }))
            .await
            .expect("write initial json");

        // Keep the original file handle open across the rewrite. If the write is implemented
        // as temp+rename replacement, the open handle continues to point at the old inode.
        let mut old_handle = std::fs::File::open(&path).expect("open old handle");

        write_json_file(&path, &json!({ "version": 2u32, "payload": "new" }))
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

    #[tokio::test]
    async fn list_files_with_extension_filters_non_matching_entries() {
        let root = unique_temp_root();
        let _ = tokio_fs::remove_dir_all(&root).await;
        tokio_fs::create_dir_all(&root)
            .await
            .expect("create temp root");

        tokio_fs::write(root.join("a.json"), b"{}")
            .await
            .expect("write a.json");
        tokio_fs::write(root.join("b.txt"), b"ok")
            .await
            .expect("write b.txt");
        tokio_fs::write(root.join("c.json"), b"{}")
            .await
            .expect("write c.json");

        tokio_fs::create_dir_all(root.join("sub"))
            .await
            .expect("create subdir");
        tokio_fs::write(root.join("sub").join("d.json"), b"{}")
            .await
            .expect("write sub/d.json");

        let mut results = list_files_with_extension(&root, "json")
            .await
            .expect("list json files");

        results.sort();

        let expected = vec![root.join("a.json"), root.join("c.json")];
        assert_eq!(results, expected);

        tokio_fs::remove_dir_all(&root)
            .await
            .expect("remove temp root");
    }

    #[tokio::test]
    async fn delete_file_is_idempotent() {
        let root = unique_temp_root();
        let _ = tokio_fs::remove_dir_all(&root).await;
        tokio_fs::create_dir_all(&root)
            .await
            .expect("create temp root");

        let path = root.join("to-delete.txt");
        tokio_fs::write(&path, b"hello").await.expect("write file");

        delete_file(&path).await.expect("delete file");
        assert!(!path.exists());

        delete_file(&path).await.expect("delete file again");
        assert!(!path.exists());

        tokio_fs::remove_dir_all(&root)
            .await
            .expect("remove temp root");
    }
}
