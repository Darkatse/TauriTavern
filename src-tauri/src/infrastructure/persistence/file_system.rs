use crate::domain::errors::DomainError;
use crate::infrastructure::logging::logger;
use serde::{de::DeserializeOwned, Serialize};
use std::path::{Path, PathBuf};
use tokio::fs::{self as tokio_fs, create_dir_all, read_to_string};

/// Represents the application data directory structure
pub struct DataDirectory {
    root: PathBuf,
    default_user: PathBuf,
    characters: PathBuf,
    chats: PathBuf,
    settings: PathBuf,
    user_data: PathBuf,
    default_avatar: PathBuf,
    groups: PathBuf,
    group_chats: PathBuf,
}

impl DataDirectory {
    /// Create a new DataDirectory instance
    pub fn new(root: PathBuf) -> Self {
        let default_user = root.join("default-user");
        let characters = default_user.join("characters");
        let chats = default_user.join("chats");
        let settings = default_user.clone();
        let user_data = default_user.clone();
        let default_avatar = default_user
            .join("characters")
            .join("default_Seraphina.png");
        let groups = default_user.join("groups");
        let group_chats = default_user.join("group chats");

        Self {
            root,
            default_user,
            characters,
            chats,
            settings,
            user_data,
            default_avatar,
            groups,
            group_chats,
        }
    }

    /// Initialize the data directory structure
    pub async fn initialize(&self) -> Result<(), DomainError> {
        tracing::info!("Initializing data directory at: {:?}", self.root);

        // Create main directories
        self.create_directory(&self.root).await?;
        self.create_directory(&self.default_user).await?;

        // Create default user subdirectories
        let default_user_dirs = [
            "characters",
            "chats",
            "User Avatars",
            "backgrounds",
            "thumbnails",
            "thumbnails/bg",
            "thumbnails/avatar",
            "worlds",
            "user",
            "user/images",
            "groups",
            "group chats",
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

        tracing::info!("Data directory initialized successfully");
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

/// Write a JSON file
///
/// This is an async function that serializes data to JSON and writes it to a file.
/// It uses tokio's async file I/O operations for better performance and non-blocking behavior.
pub async fn write_json_file<T: Serialize>(path: &Path, data: &T) -> Result<(), DomainError> {
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
        if path.is_file() && path.extension().map_or(false, |ext| ext == extension) {
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
