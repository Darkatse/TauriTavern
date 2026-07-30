use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tauri::AppHandle;
use tokio::fs;

use crate::infrastructure::assets::{
    copy_resource_to_file, list_default_content_files_under, read_resource_json,
};
use tt_domain::errors::DomainError;
use tt_ports::repositories::content_repository::{
    ContentItem, ContentRepository, ContentScope, ContentType,
};

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
    data_root: PathBuf,
    user_content_dir: PathBuf,
}

impl FileContentRepository {
    /// Create a new FileContentRepository
    ///
    /// # Arguments
    ///
    /// * `app_handle` - Tauri app handle for resolving resource paths
    /// * `data_root` - Path to the global data root
    /// * `user_content_dir` - Path to the user content directory (e.g., data/default-user)
    ///   This should be the complete path to the user directory, not just the parent directory.
    pub fn new(app_handle: AppHandle, data_root: PathBuf, user_content_dir: PathBuf) -> Self {
        Self {
            app_handle,
            data_root,
            user_content_dir,
        }
    }

    /// Convert content type string to enum
    fn content_type_from_string(&self, content_type: &str) -> Result<ContentType, DomainError> {
        match content_type {
            "settings" => Ok(ContentType::Settings),
            "character" => Ok(ContentType::Character),
            "sprites" => Ok(ContentType::Sprites),
            "background" => Ok(ContentType::Background),
            "world" => Ok(ContentType::World),
            "avatar" => Ok(ContentType::Avatar),
            "theme" => Ok(ContentType::Theme),
            "workflow" => Ok(ContentType::Workflow),
            "kobold_preset" => Ok(ContentType::KoboldPreset),
            "openai_preset" => Ok(ContentType::OpenAIPreset),
            "novel_preset" => Ok(ContentType::NovelPreset),
            "textgen_preset" => Ok(ContentType::TextGenPreset),
            "instruct" => Ok(ContentType::Instruct),
            "context" => Ok(ContentType::Context),
            "moving_ui" => Ok(ContentType::MovingUI),
            "quick_replies" => Ok(ContentType::QuickReplies),
            "sysprompt" => Ok(ContentType::SysPrompt),
            "reasoning" => Ok(ContentType::Reasoning),
            "error_page" => Ok(ContentType::ErrorPage),
            "stylesheet" => Ok(ContentType::Stylesheet),
            _ => Err(DomainError::InvalidData(format!(
                "Unknown default content type: {}",
                content_type
            ))),
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
            ContentType::ErrorPage => self.data_root.join("_errors"),
            ContentType::Stylesheet => self.data_root.join("_css"),
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

    fn content_log_path(&self, scope: ContentScope, user_dir: &Path) -> PathBuf {
        match scope {
            ContentScope::User => user_dir.join("content.log"),
            ContentScope::Global => self.data_root.join("content.log"),
        }
    }

    async fn read_content_log(path: &Path) -> Result<Vec<String>, DomainError> {
        match fs::read_to_string(path).await {
            Ok(text) => Ok(text
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(ToOwned::to_owned)
                .collect()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(error) => {
                tracing::error!("Failed to read content log {:?}: {}", path, error);
                Err(DomainError::InternalError(format!(
                    "Failed to read content log: {}",
                    error
                )))
            }
        }
    }

    async fn write_content_log(path: &Path, entries: &[String]) -> Result<(), DomainError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await.map_err(|error| {
                tracing::error!(
                    "Failed to create content log directory {:?}: {}",
                    parent,
                    error
                );
                DomainError::InternalError(format!(
                    "Failed to create content log directory: {}",
                    error
                ))
            })?;
        }

        fs::write(path, entries.join("\n")).await.map_err(|error| {
            tracing::error!("Failed to write content log {:?}: {}", path, error);
            DomainError::InternalError(format!("Failed to write content log: {}", error))
        })
    }

    async fn seed_content_scope(
        &self,
        scope: ContentScope,
        content_items: &[ContentItem],
        user_dir: &Path,
    ) -> Result<(), DomainError> {
        let scoped_items = content_items
            .iter()
            .filter(|item| item.content_type.scope() == scope)
            .collect::<Vec<_>>();

        if scoped_items.is_empty() {
            return Ok(());
        }

        let log_path = self.content_log_path(scope, user_dir);
        let mut content_log = Self::read_content_log(&log_path).await?;

        for item in scoped_items {
            if content_log.iter().any(|entry| entry == &item.filename) {
                tracing::debug!("Skipping content item {}, already in log", item.filename);
                continue;
            }

            let target_dir = self.get_target_directory(&item.content_type, user_dir);
            fs::create_dir_all(&target_dir).await.map_err(|error| {
                tracing::error!(
                    "Failed to create target directory {:?}: {}",
                    target_dir,
                    error
                );
                DomainError::InternalError(format!("Failed to create target directory: {}", error))
            })?;

            let resource_entries = self.expand_resource_entries(&item.filename)?;

            for resource_entry in resource_entries {
                let resource_path = format!("default/content/{}", resource_entry);
                let dest_path = self.build_destination_path(item, &resource_entry, &target_dir)?;

                if dest_path.exists() {
                    tracing::debug!("Skipping copy, file already exists: {:?}", dest_path);
                    continue;
                }

                tracing::debug!("Copying {} to {:?}", resource_path, dest_path);
                copy_resource_to_file(&self.app_handle, &resource_path, &dest_path).await?;
            }

            content_log.push(item.filename.clone());
        }

        Self::write_content_log(&log_path, &content_log).await
    }

    async fn content_log_contains_any(
        &self,
        scope: ContentScope,
        content_items: &[ContentItem],
        user_dir: &Path,
    ) -> Result<bool, DomainError> {
        let scoped_items = content_items
            .iter()
            .filter(|item| item.content_type.scope() == scope)
            .collect::<Vec<_>>();

        if scoped_items.is_empty() {
            return Ok(false);
        }

        let content_log = Self::read_content_log(&self.content_log_path(scope, user_dir)).await?;
        Ok(scoped_items
            .iter()
            .any(|item| content_log.iter().any(|entry| entry == &item.filename)))
    }
}

#[async_trait]
impl ContentRepository for FileContentRepository {
    async fn copy_default_content_to_user(&self, user_handle: &str) -> Result<(), DomainError> {
        tracing::info!("Synchronizing default content for user: {}", user_handle);

        let content_items = self.get_content_index().await?;
        let user_dir = self.user_content_dir.clone();

        fs::create_dir_all(&user_dir).await.map_err(|e| {
            tracing::error!("Failed to create user directory {:?}: {}", user_dir, e);
            DomainError::InternalError(format!("Failed to create user directory: {}", e))
        })?;

        self.seed_content_scope(ContentScope::Global, &content_items, &user_dir)
            .await?;
        self.seed_content_scope(ContentScope::User, &content_items, &user_dir)
            .await?;

        tracing::info!("Default content synchronized successfully");
        Ok(())
    }

    async fn get_content_index(&self) -> Result<Vec<ContentItem>, DomainError> {
        let index_items: Vec<ContentIndexItem> =
            read_resource_json(&self.app_handle, "default/content/index.json")?;

        // Convert to domain model
        let mut content_items = Vec::with_capacity(index_items.len());
        for item in index_items {
            content_items.push(ContentItem {
                filename: item.filename,
                content_type: self.content_type_from_string(&item.content_type)?,
            });
        }

        Ok(content_items)
    }

    async fn is_default_content_initialized(&self, user_handle: &str) -> Result<bool, DomainError> {
        tracing::debug!(
            "Checking if default content is initialized for user: {}",
            user_handle
        );

        let user_dir = self.user_content_dir.clone();
        let content_items = self.get_content_index().await?;

        let has_content = self
            .content_log_contains_any(ContentScope::User, &content_items, &user_dir)
            .await?;

        tracing::debug!(
            "Default content initialized for user {}: {}",
            user_handle,
            has_content
        );

        Ok(has_content)
    }
}
