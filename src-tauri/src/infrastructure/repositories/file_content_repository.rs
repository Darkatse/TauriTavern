use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tauri::AppHandle;
use tokio::fs;

use crate::domain::errors::DomainError;
use crate::domain::repositories::content_repository::{
    ContentItem, ContentRepository, ContentType,
};
use crate::infrastructure::assets::{
    copy_resource_to_file, list_default_content_files_under, read_resource_json,
};
use crate::infrastructure::logging::logger;

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

    fn is_directory_entry(path: &str) -> bool {
        Path::new(path).extension().is_none()
    }

    fn expand_resource_entries(&self, filename: &str) -> Result<Vec<String>, DomainError> {
        if !Self::is_directory_entry(filename) {
            return Ok(vec![filename.to_string()]);
        }

        let entries = list_default_content_files_under(filename);
        if entries.is_empty() {
            return Err(DomainError::NotFound(format!(
                "Resource directory is empty or missing: {}",
                filename
            )));
        }

        Ok(entries)
    }

    fn build_destination_path(
        &self,
        item: &ContentItem,
        resource_entry: &str,
        target_dir: &Path,
    ) -> Result<PathBuf, DomainError> {
        if Self::is_directory_entry(&item.filename) {
            let dir_name = Path::new(&item.filename).file_name().ok_or_else(|| {
                DomainError::InvalidData(format!("Invalid directory entry: {}", item.filename))
            })?;

            let prefix = format!("{}/", item.filename.trim_matches('/').replace('\\', "/"));
            let relative_entry = resource_entry
                .strip_prefix(&prefix)
                .unwrap_or(resource_entry);

            return Ok(target_dir.join(dir_name).join(relative_entry));
        }

        let base_filename = Path::new(&item.filename).file_name().ok_or_else(|| {
            DomainError::InvalidData(format!("Invalid filename: {}", item.filename))
        })?;

        Ok(target_dir.join(base_filename))
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

            let resource_entries = self.expand_resource_entries(&item.filename)?;

            for resource_entry in resource_entries {
                let resource_path = format!("default/content/{}", resource_entry);
                let dest_path =
                    self.build_destination_path(&item, &resource_entry, &target_dir)?;

                if dest_path.exists() {
                    logger::debug(&format!(
                        "Skipping copy, file already exists: {:?}",
                        dest_path
                    ));
                    continue;
                }

                logger::debug(&format!(
                    "Copying {} to {:?}",
                    resource_path, dest_path
                ));

                copy_resource_to_file(&self.app_handle, &resource_path, &dest_path).await?;
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
        let index_items: Vec<ContentIndexItem> =
            read_resource_json(&self.app_handle, "default/content/index.json")?;

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
