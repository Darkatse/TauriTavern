use async_trait::async_trait;
use std::path::Path;

use tt_domain::errors::DomainError;
use tt_domain::models::avatar::{Avatar, AvatarUploadResult, CropInfo};
use tt_domain::models::persona::{Persona, Personas};

/// Repository for managing user avatars
#[async_trait]
pub trait AvatarRepository: Send + Sync {
    async fn get_personas(&self) -> Result<Personas, DomainError>;
    async fn save_persona(&self, avatar: &str, persona: &Persona) -> Result<(), DomainError>;
    /// Import or restore explicit Persona records, creating missing avatars when needed.
    async fn import_personas(&self, personas: &Personas) -> Result<(), DomainError>;
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
