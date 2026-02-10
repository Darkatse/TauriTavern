use async_trait::async_trait;
use mime_guess::from_path;
use std::path::{Path, PathBuf};
use tauri::AppHandle;
use tokio::fs;

use crate::domain::errors::DomainError;
use crate::domain::models::background::Background;
use crate::domain::repositories::background_repository::BackgroundRepository;
use crate::infrastructure::logging::logger;

/// File system implementation of the BackgroundRepository
pub struct FileBackgroundRepository {
    app_handle: AppHandle,
    backgrounds_dir: PathBuf,
}

impl FileBackgroundRepository {
    /// Create a new FileBackgroundRepository instance
    pub fn new(app_handle: AppHandle, backgrounds_dir: PathBuf) -> Self {
        Self {
            app_handle,
            backgrounds_dir,
        }
    }

    /// Check if a file is an image
    fn is_image(&self, path: &Path) -> bool {
        if let Some(mime) = from_path(path).first() {
            return mime.type_() == "image";
        }
        false
    }

    /// Get the full path for a background filename
    fn get_full_path(&self, filename: &str) -> PathBuf {
        self.backgrounds_dir.join(filename)
    }
}

#[async_trait]
impl BackgroundRepository for FileBackgroundRepository {
    async fn get_all_backgrounds(&self) -> Result<Vec<Background>, DomainError> {
        logger::debug(&format!(
            "FileBackgroundRepository: Getting all backgrounds from {:?}",
            self.backgrounds_dir
        ));

        // Ensure the directory exists
        if !self.backgrounds_dir.exists() {
            fs::create_dir_all(&self.backgrounds_dir)
                .await
                .map_err(|e| {
                    logger::error(&format!("Failed to create backgrounds directory: {}", e));
                    DomainError::InternalError(format!(
                        "Failed to create backgrounds directory: {}",
                        e
                    ))
                })?;
            return Ok(Vec::new());
        }

        // Read the directory
        let mut entries = fs::read_dir(&self.backgrounds_dir).await.map_err(|e| {
            logger::error(&format!("Failed to read backgrounds directory: {}", e));
            DomainError::InternalError(format!("Failed to read backgrounds directory: {}", e))
        })?;

        let mut backgrounds = Vec::new();

        // Process each entry
        while let Some(entry) = entries.next_entry().await.map_err(|e| {
            logger::error(&format!("Failed to read directory entry: {}", e));
            DomainError::InternalError(format!("Failed to read directory entry: {}", e))
        })? {
            let path = entry.path();

            // Check if it's an image file
            if path.is_file() && self.is_image(&path) {
                if let Some(filename) = path.file_name().and_then(|n| n.to_str()) {
                    let background =
                        Background::new(filename.to_string(), path.to_string_lossy().to_string());
                    backgrounds.push(background);
                }
            }
        }

        Ok(backgrounds)
    }

    async fn delete_background(&self, filename: &str) -> Result<(), DomainError> {
        logger::debug(&format!(
            "FileBackgroundRepository: Deleting background: {}",
            filename
        ));

        let file_path = self.get_full_path(filename);

        // Check if the file exists
        if !file_path.exists() {
            return Err(DomainError::NotFound(format!(
                "Background not found: {}",
                filename
            )));
        }

        // Delete the file
        fs::remove_file(&file_path).await.map_err(|e| {
            logger::error(&format!("Failed to delete background file: {}", e));
            DomainError::InternalError(format!("Failed to delete background file: {}", e))
        })?;

        Ok(())
    }

    async fn rename_background(
        &self,
        old_filename: &str,
        new_filename: &str,
    ) -> Result<(), DomainError> {
        logger::debug(&format!(
            "FileBackgroundRepository: Renaming background from '{}' to '{}'",
            old_filename, new_filename
        ));

        let old_path = self.get_full_path(old_filename);
        let new_path = self.get_full_path(new_filename);

        // Check if the source file exists
        if !old_path.exists() {
            return Err(DomainError::NotFound(format!(
                "Background not found: {}",
                old_filename
            )));
        }

        // Check if the destination file already exists
        if new_path.exists() {
            return Err(DomainError::InvalidData(format!(
                "Background already exists: {}",
                new_filename
            )));
        }

        // Rename the file
        fs::copy(&old_path, &new_path).await.map_err(|e| {
            logger::error(&format!("Failed to copy background file: {}", e));
            DomainError::InternalError(format!("Failed to copy background file: {}", e))
        })?;

        fs::remove_file(&old_path).await.map_err(|e| {
            logger::error(&format!("Failed to delete old background file: {}", e));
            DomainError::InternalError(format!("Failed to delete old background file: {}", e))
        })?;

        Ok(())
    }

    async fn upload_background(&self, filename: &str, data: &[u8]) -> Result<String, DomainError> {
        logger::debug(&format!(
            "FileBackgroundRepository: Uploading background: {}",
            filename
        ));

        // Ensure the directory exists
        if !self.backgrounds_dir.exists() {
            fs::create_dir_all(&self.backgrounds_dir)
                .await
                .map_err(|e| {
                    logger::error(&format!("Failed to create backgrounds directory: {}", e));
                    DomainError::InternalError(format!(
                        "Failed to create backgrounds directory: {}",
                        e
                    ))
                })?;
        }

        let file_path = self.get_full_path(filename);

        // Write the file
        fs::write(&file_path, data).await.map_err(|e| {
            logger::error(&format!("Failed to write background file: {}", e));
            DomainError::InternalError(format!("Failed to write background file: {}", e))
        })?;

        Ok(filename.to_string())
    }
}
