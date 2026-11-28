use async_trait::async_trait;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::infrastructure::assets::{BundledResources, read_resource_document};
use tt_adapter_storage_core::file_system::{delete_file, write_bytes_file};
use tt_adapter_storage_core::preset_file_naming::{PresetFilePaths, load_named_preset_files};
use tt_domain::errors::DomainError;
use tt_domain::json_document::JsonDocument;
use tt_domain::models::preset::PresetType;
use tt_ports::repositories::content_repository::ContentRepository;
use tt_ports::repositories::preset_repository::PresetRepository;

/// File-based implementation of the PresetRepository
pub struct FilePresetRepository {
    /// Bundled resources holding the default presets
    resources: Arc<dyn BundledResources>,
    /// Base user directory (e.g., data/default-user)
    user_dir: PathBuf,
    /// Content repository for default presets
    content_repository: Arc<dyn ContentRepository>,
}

impl FilePresetRepository {
    /// Create a new FilePresetRepository
    ///
    /// # Arguments
    ///
    /// * `resources` - Bundled resources holding the default presets
    /// * `user_dir` - Base user directory path
    /// * `content_repository` - Content repository for default presets
    pub fn new(
        resources: Arc<dyn BundledResources>,
        user_dir: PathBuf,
        content_repository: Arc<dyn ContentRepository>,
    ) -> Self {
        Self {
            resources,
            user_dir,
            content_repository,
        }
    }

    /// Get the directory path for a specific preset type
    fn get_preset_directory(&self, preset_type: &PresetType) -> PathBuf {
        self.user_dir.join(preset_type.directory_name())
    }

    /// Resolve canonical and deprecated legacy preset file paths for a logical preset name.
    fn get_preset_paths(
        &self,
        name: &str,
        preset_type: &PresetType,
    ) -> Result<PresetFilePaths, DomainError> {
        PresetFilePaths::new(
            name,
            &self.get_preset_directory(preset_type),
            preset_type.extension(),
        )
    }

    /// Ensure the preset directory exists
    async fn ensure_directory_exists(&self, preset_type: &PresetType) -> Result<(), DomainError> {
        let directory = self.get_preset_directory(preset_type);

        if !directory.exists() {
            tokio::fs::create_dir_all(&directory).await.map_err(|e| {
                tracing::error!("Failed to create preset directory {:?}: {}", directory, e);
                DomainError::InternalError(format!("Failed to create preset directory: {}", e))
            })?;
        }

        Ok(())
    }

    /// Get default preset from content system
    async fn get_default_preset_from_content(
        &self,
        name: &str,
        preset_type: &PresetType,
    ) -> Result<Option<JsonDocument>, DomainError> {
        tracing::debug!(
            "Looking for default preset: {} (type: {})",
            name,
            preset_type
        );

        // Get content index
        let content_items = self.content_repository.get_content_index().await?;

        // Find matching preset in content
        for item in content_items {
            // Check if this is a preset of the right type
            let item_preset_type = match item.content_type {
                tt_ports::repositories::content_repository::ContentType::KoboldPreset => {
                    Some(PresetType::Kobold)
                }
                tt_ports::repositories::content_repository::ContentType::NovelPreset => {
                    Some(PresetType::Novel)
                }
                tt_ports::repositories::content_repository::ContentType::OpenAIPreset => {
                    Some(PresetType::OpenAI)
                }
                tt_ports::repositories::content_repository::ContentType::TextGenPreset => {
                    Some(PresetType::TextGen)
                }
                tt_ports::repositories::content_repository::ContentType::Instruct => {
                    Some(PresetType::Instruct)
                }
                tt_ports::repositories::content_repository::ContentType::Context => {
                    Some(PresetType::Context)
                }
                tt_ports::repositories::content_repository::ContentType::SysPrompt => {
                    Some(PresetType::SysPrompt)
                }
                tt_ports::repositories::content_repository::ContentType::Reasoning => {
                    Some(PresetType::Reasoning)
                }
                _ => None,
            };

            if let Some(item_type) = item_preset_type
                && item_type == *preset_type
            {
                // Extract name from filename (remove extension)
                let item_name = Path::new(&item.filename)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or(&item.filename);

                if item_name == name {
                    return read_resource_document(
                        self.resources.as_ref(),
                        &format!("default/content/{}", item.filename),
                    )
                    .map(Some);
                }
            }
        }

        tracing::debug!("Default preset not found: {} (type: {})", name, preset_type);
        Ok(None)
    }
}

#[async_trait]
impl PresetRepository for FilePresetRepository {
    async fn save_preset(
        &self,
        name: &str,
        preset_type: &PresetType,
        mut preset: JsonDocument,
    ) -> Result<(), DomainError> {
        tracing::debug!("Saving preset: {} (type: {})", name, preset_type);

        // Ensure directory exists
        self.ensure_directory_exists(preset_type).await?;

        let file_path = self
            .get_preset_paths(name, preset_type)?
            .prepare_for_save()
            .await?;

        // Listing reads the name back from the file, whose stem is sanitized.
        preset.set("name", &Value::from(name));
        write_bytes_file(&file_path, &preset.to_pretty_vec()).await?;

        tracing::info!("Preset saved to {:?}", file_path);
        Ok(())
    }

    async fn delete_preset(&self, name: &str, preset_type: &PresetType) -> Result<(), DomainError> {
        tracing::debug!("Deleting preset: {} (type: {})", name, preset_type);

        let file_path = self
            .get_preset_paths(name, preset_type)?
            .resolve_existing()?
            .ok_or_else(|| DomainError::NotFound(format!("Preset not found: {}", name)))?;

        delete_file(&file_path).await?;

        tracing::info!("Preset deleted: {:?}", file_path);
        Ok(())
    }

    async fn preset_exists(
        &self,
        name: &str,
        preset_type: &PresetType,
    ) -> Result<bool, DomainError> {
        Ok(self
            .get_preset_paths(name, preset_type)?
            .resolve_existing()?
            .is_some())
    }

    async fn get_preset(
        &self,
        name: &str,
        preset_type: &PresetType,
    ) -> Result<Option<JsonDocument>, DomainError> {
        tracing::debug!("Getting preset: {} (type: {})", name, preset_type);

        let Some(file_path) = self
            .get_preset_paths(name, preset_type)?
            .resolve_existing()?
        else {
            return Ok(None);
        };

        let bytes = tokio::fs::read(&file_path).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read preset file '{}': {}",
                file_path.display(),
                error
            ))
        })?;
        JsonDocument::parse(&bytes).map(Some).map_err(|error| {
            DomainError::InvalidData(format!(
                "Invalid preset JSON in '{}': {}",
                file_path.display(),
                error
            ))
        })
    }

    async fn list_presets(&self, preset_type: &PresetType) -> Result<Vec<String>, DomainError> {
        tracing::debug!("Listing presets of type: {}", preset_type);

        let directory = self.get_preset_directory(preset_type);

        if !directory.exists() {
            tracing::debug!("Preset directory does not exist: {:?}", directory);
            return Ok(vec![]);
        }

        let preset_names: Vec<String> = load_named_preset_files(&directory)
            .await?
            .into_iter()
            .map(|entry| entry.name)
            .collect();

        tracing::debug!(
            "Found {} presets of type {}",
            preset_names.len(),
            preset_type
        );

        Ok(preset_names)
    }

    async fn get_default_preset(
        &self,
        name: &str,
        preset_type: &PresetType,
    ) -> Result<Option<JsonDocument>, DomainError> {
        self.get_default_preset_from_content(name, preset_type)
            .await
    }
}
