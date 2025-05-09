use async_trait::async_trait;
use std::path::Path;

use crate::domain::errors::DomainError;
use crate::domain::models::avatar::{Avatar, AvatarUploadResult, CropInfo};

/// Repository for managing user avatars
#[async_trait]
pub trait AvatarRepository: Send + Sync {
    /// Get all avatars
    async fn get_avatars(&self) -> Result<Vec<Avatar>, DomainError>;

    /// Delete an avatar
    async fn delete_avatar(&self, avatar_name: &str) -> Result<(), DomainError>;

    /// Upload an avatar
    async fn upload_avatar(
        &self,
        file_path: &Path,
        overwrite_name: Option<String>,
        crop_info: Option<CropInfo>,
    ) -> Result<AvatarUploadResult, DomainError>;
}
