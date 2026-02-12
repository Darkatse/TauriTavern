use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tauri::path::BaseDirectory;
use tauri::AppHandle;
use tauri::Manager;
use tokio::fs;

use crate::domain::errors::DomainError;
use crate::domain::repositories::content_repository::{
    ContentItem, ContentRepository, ContentType,
};
use crate::infrastructure::logging::logger;
use crate::infrastructure::persistence::file_system::read_json_file;

/// Content index item from JSON
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ContentIndexItem {
    filename: String,
    #[serde(rename = "type")]
    content_type: String,
}

/// File Content Repository implementation
pub struct FileContentRepository {
    app_handle: AppHandle,
    user_content_dir: PathBuf,
}

impl FileContentRepository {
    /// Create a new FileContentRepository
    ///
    /// # Arguments
    ///
    /// * `app_handle` - Tauri app handle for resolving resource paths
    /// * `user_content_dir` - Path to the user content directory (e.g., data/default-user)
    ///   This should be the complete path to the user directory, not just the parent directory.
    pub fn new(app_handle: AppHandle, user_content_dir: PathBuf) -> Self {
        Self {
            app_handle,
            user_content_dir,
        }
    }

    /// Convert content type string to enum
    fn content_type_from_string(&self, content_type: &str) -> ContentType {
        match content_type {
            "settings" => ContentType::Settings,
            "character" => ContentType::Character,
            "sprites" => ContentType::Sprites,
            "background" => ContentType::Background,
            "world" => ContentType::World,
            "avatar" => ContentType::Avatar,
            "theme" => ContentType::Theme,
            "workflow" => ContentType::Workflow,
            "kobold_preset" => ContentType::KoboldPreset,
            "openai_preset" => ContentType::OpenAIPreset,
            "novel_preset" => ContentType::NovelPreset,
            "textgen_preset" => ContentType::TextGenPreset,
            "instruct" => ContentType::Instruct,
            "context" => ContentType::Context,
            "moving_ui" => ContentType::MovingUI,
            "quick_replies" => ContentType::QuickReplies,
            "sysprompt" => ContentType::SysPrompt,
            "reasoning" => ContentType::Reasoning,
            _ => {
                logger::warn(&format!("Unknown content type: {}", content_type));
                ContentType::Settings // Default to settings
            }
        }
    }

    /// Get the target directory for a content type
    fn get_target_directory(&self, content_type: &ContentType, user_dir: &Path) -> PathBuf {
        match content_type {
            ContentType::Settings => user_dir.to_path_buf(),
            ContentType::Character => user_dir.join("characters"),
            ContentType::Sprites => user_dir.join("characters"),
            ContentType::Background => user_dir.join("backgrounds"),
            ContentType::World => user_dir.join("worlds"),
            ContentType::Avatar => user_dir.join("User Avatars"),
            ContentType::Theme => user_dir.join("themes"),
            ContentType::Workflow => user_dir.join("user").join("workflows"),
            ContentType::KoboldPreset => user_dir.join("KoboldAI Settings"),
            ContentType::OpenAIPreset => user_dir.join("OpenAI Settings"),
            ContentType::NovelPreset => user_dir.join("NovelAI Settings"),
            ContentType::TextGenPreset => user_dir.join("TextGen Settings"),
            ContentType::Instruct => user_dir.join("instruct"),
            ContentType::Context => user_dir.join("context"),
            ContentType::MovingUI => user_dir.join("movingUI"),
            ContentType::QuickReplies => user_dir.join("QuickReplies"),
            ContentType::SysPrompt => user_dir.join("sysprompt"),
            ContentType::Reasoning => user_dir.join("reasoning"),
        }
    }

    /// Copy a file from default content to user content
    async fn copy_file(&self, source_path: &Path, dest_path: &Path) -> Result<(), DomainError> {
        // Create parent directory if it doesn't exist
        if let Some(parent) = dest_path.parent() {
            fs::create_dir_all(parent).await.map_err(|e| {
                logger::error(&format!("Failed to create directory {:?}: {}", parent, e));
                DomainError::InternalError(format!("Failed to create directory: {}", e))
            })?;
        }

        // Only copy if destination doesn't exist
        if !dest_path.exists() {
            logger::debug(&format!(
                "Copying file from {:?} to {:?}",
                source_path, dest_path
            ));

            fs::copy(source_path, dest_path).await.map_err(|e| {
                logger::error(&format!(
                    "Failed to copy file from {:?} to {:?}: {}",
                    source_path, dest_path, e
                ));
                DomainError::InternalError(format!("Failed to copy file: {}", e))
            })?;
        } else {
            logger::debug(&format!(
                "Skipping copy, file already exists: {:?}",
                dest_path
            ));
        }

        Ok(())
    }

    /// Copy a directory from default content to user content
    async fn copy_directory(&self, source_dir: &Path, dest_dir: &Path) -> Result<(), DomainError> {
        // Create destination directory if it doesn't exist
        fs::create_dir_all(dest_dir).await.map_err(|e| {
            logger::error(&format!("Failed to create directory {:?}: {}", dest_dir, e));
            DomainError::InternalError(format!("Failed to create directory: {}", e))
        })?;

        // Read source directory
        let mut entries = fs::read_dir(source_dir).await.map_err(|e| {
            logger::error(&format!("Failed to read directory {:?}: {}", source_dir, e));
            DomainError::InternalError(format!("Failed to read directory: {}", e))
        })?;

        // Process each entry
        while let Some(entry) = entries.next_entry().await.map_err(|e| {
            logger::error(&format!("Failed to read directory entry: {}", e));
            DomainError::InternalError(format!("Failed to read directory entry: {}", e))
        })? {
            let source_path = entry.path();
            let dest_path = dest_dir.join(source_path.file_name().unwrap());

            if source_path.is_dir() {
                // Recursively copy subdirectory
                Box::pin(self.copy_directory(&source_path, &dest_path)).await?;
            } else {
                // Copy file
                self.copy_file(&source_path, &dest_path).await?;
            }
        }

        Ok(())
    }
}

#[async_trait]
impl ContentRepository for FileContentRepository {
    async fn copy_default_content_to_user(&self, user_handle: &str) -> Result<(), DomainError> {
        tracing::info!(
            "Copying default content to user directory for user: {}",
            user_handle
        );

        // Get content index
        let content_items = self.get_content_index().await?;

        // User content directory - self.user_content_dir 已经是 data/default-user 路径了，不需要再 join user_handle
        let user_dir = self.user_content_dir.clone();

        // Create user directory if it doesn't exist
        fs::create_dir_all(&user_dir).await.map_err(|e| {
            tracing::error!("Failed to create user directory {:?}: {}", user_dir, e);
            DomainError::InternalError(format!("Failed to create user directory: {}", e))
        })?;

        // Create a content log to track copied files
        let mut content_log = Vec::new();
        let content_log_path = user_dir.join("content.log");

        // Read existing content log if it exists
        if content_log_path.exists() {
            let content_log_text = fs::read_to_string(&content_log_path).await.map_err(|e| {
                logger::error(&format!("Failed to read content log: {}", e));
                DomainError::InternalError(format!("Failed to read content log: {}", e))
            })?;

            content_log = content_log_text.lines().map(|s| s.to_string()).collect();
        }

        // Copy each content item
        for item in content_items {
            // Skip if already in content log
            if content_log.contains(&item.filename) {
                logger::debug(&format!(
                    "Skipping content item {}, already in log",
                    item.filename
                ));
                continue;
            }

            // Resolve the source path using Tauri's resource system
            let source_path = self
                .app_handle
                .path()
                .resolve(
                    &format!("default/content/{}", item.filename),
                    BaseDirectory::Resource,
                )
                .map_err(|e| {
                    logger::error(&format!(
                        "Failed to resolve source path for {}: {}",
                        item.filename, e
                    ));
                    DomainError::InternalError(format!("Failed to resolve source path: {}", e))
                })?;

            // Get the target directory based on content type
            let target_dir = self.get_target_directory(&item.content_type, &user_dir);

            // Ensure target directory exists
            fs::create_dir_all(&target_dir).await.map_err(|e| {
                logger::error(&format!(
                    "Failed to create target directory {:?}: {}",
                    target_dir, e
                ));
                DomainError::InternalError(format!("Failed to create target directory: {}", e))
            })?;

            // Get the base filename
            let base_filename = Path::new(&item.filename).file_name().ok_or_else(|| {
                logger::error(&format!("Invalid filename: {}", item.filename));
                DomainError::InvalidData(format!("Invalid filename: {}", item.filename))
            })?;

            // Create destination path
            let dest_path = target_dir.join(base_filename);

            logger::debug(&format!("Copying {} to {:?}", item.filename, dest_path));

            if source_path.is_dir() {
                Box::pin(self.copy_directory(&source_path, &dest_path)).await?;
            } else {
                self.copy_file(&source_path, &dest_path).await?;
            }

            // Add to content log
            content_log.push(item.filename.clone());
        }

        // Write content log
        fs::write(&content_log_path, content_log.join("\n"))
            .await
            .map_err(|e| {
                logger::error(&format!("Failed to write content log: {}", e));
                DomainError::InternalError(format!("Failed to write content log: {}", e))
            })?;

        tracing::info!("Default content copied successfully");
        Ok(())
    }

    async fn get_content_index(&self) -> Result<Vec<ContentItem>, DomainError> {
        // Resolve the path to the content index file using Tauri's resource system
        let content_index_path = self
            .app_handle
            .path()
            .resolve("default/content/index.json", BaseDirectory::Resource)
            .map_err(|e| {
                logger::error(&format!("Failed to resolve content index path: {}", e));
                DomainError::InternalError(format!("Failed to resolve content index path: {}", e))
            })?;

        logger::debug(&format!(
            "Reading content index from {:?}",
            content_index_path
        ));

        // Read content index
        let index_items: Vec<ContentIndexItem> = read_json_file(&content_index_path).await?;

        // Convert to domain model
        let content_items = index_items
            .into_iter()
            .map(|item| ContentItem {
                filename: item.filename,
                content_type: self.content_type_from_string(&item.content_type),
            })
            .collect();

        Ok(content_items)
    }

    async fn is_default_content_initialized(&self, user_handle: &str) -> Result<bool, DomainError> {
        logger::debug(&format!(
            "Checking if default content is initialized for user: {}",
            user_handle
        ));

        // User content directory - self.user_content_dir 已经是 data/default-user 路径了，不需要再 join user_handle
        let user_dir = self.user_content_dir.clone();

        // Content log path
        let content_log_path = user_dir.join("content.log");

        // If content log doesn't exist, content is not initialized
        if !content_log_path.exists() {
            logger::debug(&format!("Content log not found for user: {}", user_handle));
            return Ok(false);
        }

        // Read content log
        let content_log_text = fs::read_to_string(&content_log_path).await.map_err(|e| {
            logger::error(&format!("Failed to read content log: {}", e));
            DomainError::InternalError(format!("Failed to read content log: {}", e))
        })?;

        // Get content index
        let content_items = self.get_content_index().await?;

        // Check if all content items are in the log
        let content_log: Vec<String> = content_log_text.lines().map(|s| s.to_string()).collect();

        // If content log is empty, content is not initialized
        if content_log.is_empty() {
            logger::debug(&format!("Content log is empty for user: {}", user_handle));
            return Ok(false);
        }

        // Check if at least some content items are in the log
        // We don't require all items to be in the log, as new content might be added later
        let has_content = content_items
            .iter()
            .any(|item| content_log.contains(&item.filename));

        logger::debug(&format!(
            "Default content initialized for user {}: {}",
            user_handle, has_content
        ));

        Ok(has_content)
    }
}
