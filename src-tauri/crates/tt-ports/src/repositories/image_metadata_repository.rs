use async_trait::async_trait;

use tt_domain::errors::DomainError;
use tt_domain::models::background::BackgroundListEntry;
use tt_domain::models::image_metadata::{
    BackgroundFoldersPayload, ImageMetadataFolder, ImageMetadataIndex,
};

#[async_trait]
pub trait ImageMetadataRepository: Send + Sync {
    async fn read_metadata_index(
        &self,
        prefix: Option<&str>,
    ) -> Result<ImageMetadataIndex, DomainError>;

    async fn get_background_list_entries(&self) -> Result<Vec<BackgroundListEntry>, DomainError>;

    async fn get_background_folders(&self) -> Result<BackgroundFoldersPayload, DomainError>;

    async fn create_folder(&self, name: &str) -> Result<ImageMetadataFolder, DomainError>;

    async fn update_folder(
        &self,
        id: &str,
        name: Option<&str>,
        thumbnail_file: Option<&str>,
    ) -> Result<ImageMetadataFolder, DomainError>;

    async fn delete_folder(&self, id: &str) -> Result<(), DomainError>;

    async fn set_folder_thumbnails(
        &self,
        updates: Vec<(String, String)>,
    ) -> Result<(), DomainError>;

    async fn assign_images_to_folder(
        &self,
        id: &str,
        paths: Vec<String>,
    ) -> Result<(), DomainError>;

    async fn unassign_images_from_folder(
        &self,
        id: &str,
        paths: Vec<String>,
    ) -> Result<(), DomainError>;

    async fn remove_background_metadata(&self, filename: &str) -> Result<(), DomainError>;

    async fn rename_background_metadata(
        &self,
        old_filename: &str,
        new_filename: &str,
    ) -> Result<(), DomainError>;
}
