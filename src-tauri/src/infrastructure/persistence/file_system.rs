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

    // Write to file using tokio's async write function
    tokio_fs::write(path, json).await.map_err(|e| {
        logger::error(&format!("Failed to write to file {:?}: {}", path, e));
        DomainError::InternalError(format!("Failed to write to file: {}", e))
    })?;

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
