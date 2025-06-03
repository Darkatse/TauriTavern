use std::path::{Path, PathBuf};
use std::sync::Arc;
use async_trait::async_trait;
use serde_json::Value;
use tauri::AppHandle;
use tauri::Manager;
use tauri::path::BaseDirectory;

use crate::domain::errors::DomainError;
use crate::domain::models::preset::{Preset, PresetType, DefaultPreset, sanitize_filename};
use crate::domain::repositories::preset_repository::PresetRepository;
use crate::domain::repositories::content_repository::ContentRepository;
use crate::infrastructure::logging::logger;
use crate::infrastructure::persistence::file_system::{read_json_file, write_json_file, delete_file, list_files_with_extension};

/// File-based implementation of the PresetRepository
pub struct FilePresetRepository {
    /// Tauri app handle for path resolution
    app_handle: AppHandle,
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
    /// * `app_handle` - Tauri app handle for path resolution
    /// * `user_dir` - Base user directory path
    /// * `content_repository` - Content repository for default presets
    pub fn new(app_handle: AppHandle, user_dir: PathBuf, content_repository: Arc<dyn ContentRepository>) -> Self {
        Self {
            app_handle,
            user_dir,
            content_repository,
        }
    }

    /// Get the directory path for a specific preset type
    fn get_preset_directory(&self, preset_type: &PresetType) -> PathBuf {
        self.user_dir.join(preset_type.directory_name())
    }

    /// Get the full file path for a preset
    fn get_preset_path(&self, name: &str, preset_type: &PresetType) -> PathBuf {
        let directory = self.get_preset_directory(preset_type);
        let filename = format!("{}{}", sanitize_filename(name), preset_type.extension());
        directory.join(filename)
    }

    /// Ensure the preset directory exists
    async fn ensure_directory_exists(&self, preset_type: &PresetType) -> Result<(), DomainError> {
        let directory = self.get_preset_directory(preset_type);
        
        if !directory.exists() {
            tokio::fs::create_dir_all(&directory).await.map_err(|e| {
                logger::error(&format!("Failed to create preset directory {:?}: {}", directory, e));
                DomainError::InternalError(format!("Failed to create preset directory: {}", e))
            })?;
        }

        Ok(())
    }

    /// Get default preset from content system
    async fn get_default_preset_from_content(&self, name: &str, preset_type: &PresetType) -> Result<Option<DefaultPreset>, DomainError> {
        logger::debug(&format!("Looking for default preset: {} (type: {})", name, preset_type));

        // Get content index
        let content_items = self.content_repository.get_content_index().await?;

        // Find matching preset in content
        for item in content_items {
            // Check if this is a preset of the right type
            let item_preset_type = match item.content_type {
                crate::domain::repositories::content_repository::ContentType::KoboldPreset => Some(PresetType::Kobold),
                crate::domain::repositories::content_repository::ContentType::NovelPreset => Some(PresetType::Novel),
                crate::domain::repositories::content_repository::ContentType::OpenAIPreset => Some(PresetType::OpenAI),
                crate::domain::repositories::content_repository::ContentType::TextGenPreset => Some(PresetType::TextGen),
                crate::domain::repositories::content_repository::ContentType::Instruct => Some(PresetType::Instruct),
                crate::domain::repositories::content_repository::ContentType::Context => Some(PresetType::Context),
                crate::domain::repositories::content_repository::ContentType::SysPrompt => Some(PresetType::SysPrompt),
                crate::domain::repositories::content_repository::ContentType::Reasoning => Some(PresetType::Reasoning),
                _ => None,
            };

            if let Some(item_type) = item_preset_type {
                if item_type == *preset_type {
                    // Extract name from filename (remove extension)
                    let item_name = Path::new(&item.filename)
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or(&item.filename);

                    if item_name == name {
                        // Found matching preset, load it
                        let content_path = self.app_handle.path()
                            .resolve(&format!("default/content/{}", item.filename), BaseDirectory::Resource)
                            .map_err(|e| {
                                logger::error(&format!("Failed to resolve content path for {}: {}", item.filename, e));
                                DomainError::InternalError(format!("Failed to resolve content path: {}", e))
                            })?;

                        let data: Value = read_json_file(&content_path).await?;

                        return Ok(Some(DefaultPreset {
                            filename: item.filename,
                            name: name.to_string(),
                            preset_type: preset_type.clone(),
                            is_default: true,
                            data,
                        }));
                    }
                }
            }
        }

        logger::debug(&format!("Default preset not found: {} (type: {})", name, preset_type));
        Ok(None)
    }
}

#[async_trait]
impl PresetRepository for FilePresetRepository {
    async fn save_preset(&self, preset: &Preset) -> Result<(), DomainError> {
        logger::debug(&format!("Saving preset: {} (type: {})", preset.name, preset.preset_type));

        // Ensure directory exists
        self.ensure_directory_exists(&preset.preset_type).await?;

        // Get file path
        let file_path = self.get_preset_path(&preset.name, &preset.preset_type);

        // Prepare data with name included
        let data_with_name = preset.data_with_name();

        // Write file
        write_json_file(&file_path, &data_with_name).await?;

        logger::info(&format!("Preset saved to {:?}", file_path));
        Ok(())
    }

    async fn delete_preset(&self, name: &str, preset_type: &PresetType) -> Result<(), DomainError> {
        logger::debug(&format!("Deleting preset: {} (type: {})", name, preset_type));

        let file_path = self.get_preset_path(name, preset_type);

        if !file_path.exists() {
            return Err(DomainError::NotFound(format!("Preset not found: {}", name)));
        }

        delete_file(&file_path).await?;

        logger::info(&format!("Preset deleted: {:?}", file_path));
        Ok(())
    }

    async fn preset_exists(&self, name: &str, preset_type: &PresetType) -> Result<bool, DomainError> {
        let file_path = self.get_preset_path(name, preset_type);
        Ok(file_path.exists())
    }

    async fn get_preset(&self, name: &str, preset_type: &PresetType) -> Result<Option<Preset>, DomainError> {
        logger::debug(&format!("Getting preset: {} (type: {})", name, preset_type));

        let file_path = self.get_preset_path(name, preset_type);

        if !file_path.exists() {
            return Ok(None);
        }

        let data: Value = read_json_file(&file_path).await?;

        let preset = Preset::new(name.to_string(), preset_type.clone(), data);

        Ok(Some(preset))
    }

    async fn list_presets(&self, preset_type: &PresetType) -> Result<Vec<String>, DomainError> {
        logger::debug(&format!("Listing presets of type: {}", preset_type));

        let directory = self.get_preset_directory(preset_type);

        if !directory.exists() {
            logger::debug(&format!("Preset directory does not exist: {:?}", directory));
            return Ok(vec![]);
        }

        let files = list_files_with_extension(&directory, preset_type.extension()).await?;

        let preset_names: Vec<String> = files.into_iter()
            .filter_map(|file_path| {
                file_path.file_stem()
                    .and_then(|stem| stem.to_str())
                    .map(|s| s.to_string())
            })
            .collect();

        logger::debug(&format!("Found {} presets of type {}", preset_names.len(), preset_type));

        Ok(preset_names)
    }

    async fn get_default_preset(&self, name: &str, preset_type: &PresetType) -> Result<Option<DefaultPreset>, DomainError> {
        self.get_default_preset_from_content(name, preset_type).await
    }

    async fn list_default_presets(&self, preset_type: &PresetType) -> Result<Vec<DefaultPreset>, DomainError> {
        logger::debug(&format!("Listing default presets of type: {}", preset_type));

        // Get content index
        let content_items = self.content_repository.get_content_index().await?;

        let mut default_presets = Vec::new();

        // Find all presets of the specified type
        for item in content_items {
            let item_preset_type = match item.content_type {
                crate::domain::repositories::content_repository::ContentType::KoboldPreset => Some(PresetType::Kobold),
                crate::domain::repositories::content_repository::ContentType::NovelPreset => Some(PresetType::Novel),
                crate::domain::repositories::content_repository::ContentType::OpenAIPreset => Some(PresetType::OpenAI),
                crate::domain::repositories::content_repository::ContentType::TextGenPreset => Some(PresetType::TextGen),
                crate::domain::repositories::content_repository::ContentType::Instruct => Some(PresetType::Instruct),
                crate::domain::repositories::content_repository::ContentType::Context => Some(PresetType::Context),
                crate::domain::repositories::content_repository::ContentType::SysPrompt => Some(PresetType::SysPrompt),
                crate::domain::repositories::content_repository::ContentType::Reasoning => Some(PresetType::Reasoning),
                _ => None,
            };

            if let Some(item_type) = item_preset_type {
                if item_type == *preset_type {
                    // Extract name from filename
                    let item_name = Path::new(&item.filename)
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or(&item.filename);

                    // Load preset data
                    let content_path = self.app_handle.path()
                        .resolve(&format!("default/content/{}", item.filename), BaseDirectory::Resource)
                        .map_err(|e| {
                            logger::error(&format!("Failed to resolve content path for {}: {}", item.filename, e));
                            DomainError::InternalError(format!("Failed to resolve content path: {}", e))
                        })?;

                    match read_json_file::<Value>(&content_path).await {
                        Ok(data) => {
                            default_presets.push(DefaultPreset {
                                filename: item.filename.clone(),
                                name: item_name.to_string(),
                                preset_type: preset_type.clone(),
                                is_default: true,
                                data,
                            });
                        }
                        Err(e) => {
                            logger::warn(&format!("Failed to load default preset {}: {}", item.filename, e));
                        }
                    }
                }
            }
        }

        logger::debug(&format!("Found {} default presets of type {}", default_presets.len(), preset_type));

        Ok(default_presets)
    }
}
