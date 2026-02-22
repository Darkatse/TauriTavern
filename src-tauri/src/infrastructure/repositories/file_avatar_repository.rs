use async_trait::async_trait;
use image::ImageFormat;
use std::fs;
use std::path::{Path, PathBuf};
use tokio::fs as tokio_fs;

use crate::domain::errors::DomainError;
use crate::domain::models::avatar::{Avatar, AvatarUploadResult, CropInfo};
use crate::domain::repositories::avatar_repository::AvatarRepository;

// Constants for avatar dimensions
const AVATAR_WIDTH: u32 = 400;
const AVATAR_HEIGHT: u32 = 600;

/// File-based implementation of AvatarRepository
pub struct FileAvatarRepository {
    avatars_dir: PathBuf,
}

impl FileAvatarRepository {
    /// Create a new FileAvatarRepository
    pub fn new(avatars_dir: PathBuf) -> Self {
        // Create directory if it doesn't exist
        fs::create_dir_all(&avatars_dir).expect("Failed to create avatars directory");

        Self { avatars_dir }
    }

    /// Process an image file with optional cropping
    async fn process_image(
        &self,
        file_path: &Path,
        crop_info: Option<CropInfo>,
    ) -> Result<Vec<u8>, DomainError> {
        // Read the image file
        let img_data = tokio_fs::read(file_path)
            .await
            .map_err(|e| DomainError::InternalError(format!("Failed to read image file: {}", e)))?;

        // Load the image
        let mut img = image::load_from_memory(&img_data)
            .map_err(|e| DomainError::InternalError(format!("Failed to load image: {}", e)))?;

        // Apply cropping if specified
        if let Some(crop) = crop_info {
            if crop.x >= 0
                && crop.y >= 0
                && crop.width > 0
                && crop.height > 0
                && (crop.x as u32) < img.width()
                && (crop.y as u32) < img.height()
            {
                img = img.crop_imm(
                    crop.x as u32,
                    crop.y as u32,
                    crop.width as u32,
                    crop.height as u32,
                );
            }
        }

        // Resize the image to the standard avatar dimensions
        let resized_img = img.resize_exact(
            AVATAR_WIDTH,
            AVATAR_HEIGHT,
            image::imageops::FilterType::Lanczos3,
        );

        // Convert the image to PNG format
        let mut buffer = Vec::new();
        let mut cursor = std::io::Cursor::new(&mut buffer);
        resized_img
            .write_to(&mut cursor, ImageFormat::Png)
            .map_err(|e| DomainError::InternalError(format!("Failed to encode image: {}", e)))?;

        Ok(buffer)
    }

    /// Sanitize a filename
    fn sanitize_filename(&self, filename: &str) -> String {
        filename
            .chars()
            .map(|c| match c {
                '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
                _ => c,
            })
            .collect()
    }
}

#[async_trait]
impl AvatarRepository for FileAvatarRepository {
    async fn get_avatars(&self) -> Result<Vec<Avatar>, DomainError> {
        tracing::debug!("Getting all avatars");

        let mut avatars = Vec::new();

        // Read the avatars directory
        let entries = fs::read_dir(&self.avatars_dir).map_err(|e| {
            tracing::error!("Failed to read avatars directory: {}", e);
            DomainError::InternalError(format!("Failed to read avatars directory: {}", e))
        })?;

        // Process each entry
        for entry in entries {
            let entry = entry.map_err(|e| {
                tracing::error!("Failed to read directory entry: {}", e);
                DomainError::InternalError(format!("Failed to read directory entry: {}", e))
            })?;

            let path = entry.path();
            if path.is_file() {
                if let Some(name) = path.file_name() {
                    let name_str = name.to_string_lossy().to_string();
                    avatars.push(Avatar {
                        name: name_str,
                        path: path.clone(),
                    });
                }
            }
        }

        tracing::debug!("Found {} avatars", avatars.len());
        Ok(avatars)
    }

    async fn delete_avatar(&self, avatar_name: &str) -> Result<(), DomainError> {
        tracing::debug!("Deleting avatar: {}", avatar_name);

        // Sanitize the avatar name
        let sanitized_name = self.sanitize_filename(avatar_name);
        let avatar_path = self.avatars_dir.join(&sanitized_name);

        // Check if the avatar exists
        if !avatar_path.exists() {
            return Err(DomainError::NotFound(format!(
                "Avatar not found: {}",
                avatar_name
            )));
        }

        // Delete the avatar file
        tokio_fs::remove_file(&avatar_path).await.map_err(|e| {
            tracing::error!("Failed to delete avatar: {}", e);
            DomainError::InternalError(format!("Failed to delete avatar: {}", e))
        })?;

        tracing::info!("Avatar deleted: {}", avatar_name);
        Ok(())
    }

    async fn upload_avatar(
        &self,
        file_path: &Path,
        overwrite_name: Option<String>,
        crop_info: Option<CropInfo>,
    ) -> Result<AvatarUploadResult, DomainError> {
        tracing::debug!("Uploading avatar: {:?}", file_path);

        // Process the image
        let image_data = self.process_image(file_path, crop_info).await?;

        // Generate a filename
        let filename = match overwrite_name {
            Some(name) => self.sanitize_filename(&name),
            None => format!("{}.png", chrono::Utc::now().timestamp_millis()),
        };

        // Save the processed image
        let avatar_path = self.avatars_dir.join(&filename);
        tokio_fs::write(&avatar_path, &image_data)
            .await
            .map_err(|e| {
                tracing::error!("Failed to write avatar file: {}", e);
                DomainError::InternalError(format!("Failed to write avatar file: {}", e))
            })?;

        tracing::info!("Avatar uploaded: {}", filename);
        Ok(AvatarUploadResult { path: filename })
    }
}
