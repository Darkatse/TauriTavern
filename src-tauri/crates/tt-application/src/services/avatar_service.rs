use std::path::Path;
use std::sync::Arc;

use tt_domain::errors::DomainError;
use tt_domain::models::avatar::{AvatarUploadResult, CropInfo};
use tt_ports::repositories::avatar_repository::AvatarRepository;

/// Service for managing user avatars
pub struct AvatarService {
    avatar_repository: Arc<dyn AvatarRepository>,
}

impl AvatarService {
    /// Create a new AvatarService
    pub fn new(avatar_repository: Arc<dyn AvatarRepository>) -> Self {
        Self { avatar_repository }
    }

    /// Get all avatars
    pub async fn get_avatars(&self) -> Result<Vec<String>, DomainError> {
        tracing::debug!("Getting all avatars");
        let avatars = self.avatar_repository.get_avatars().await?;

        // Return only the avatar names
        let avatar_names = avatars.into_iter().map(|a| a.name).collect();
        Ok(avatar_names)
    }

    /// Delete an avatar
    pub async fn delete_avatar(&self, avatar_name: &str) -> Result<(), DomainError> {
        tracing::debug!("Deleting avatar: {}", avatar_name);
        self.avatar_repository.delete_avatar(avatar_name).await
    }

    /// Upload an avatar
    pub async fn upload_avatar(
        &self,
        file_path: &Path,
        overwrite_name: Option<String>,
        crop_info: Option<CropInfo>,
    ) -> Result<AvatarUploadResult, DomainError> {
        tracing::debug!("Uploading avatar: {:?}", file_path);
        self.avatar_repository
            .upload_avatar(file_path, overwrite_name, crop_info)
            .await
    }
}
