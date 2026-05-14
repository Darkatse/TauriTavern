use std::sync::Arc;

use crate::application::dto::image_metadata_dto::{
    CreateImageMetadataFolderDto, DeleteImageMetadataFolderDto, ImageMetadataFolderAssignmentDto,
    SetImageMetadataFolderThumbnailsDto, UpdateImageMetadataFolderDto,
};
use crate::application::errors::ApplicationError;
use crate::domain::errors::DomainError;
use crate::domain::models::background::BackgroundListEntry;
use crate::domain::models::image_metadata::{
    BackgroundFoldersPayload, ImageMetadataFolder, ImageMetadataIndex,
};
use crate::domain::repositories::image_metadata_repository::ImageMetadataRepository;

pub struct ImageMetadataService {
    repository: Arc<dyn ImageMetadataRepository>,
}

impl ImageMetadataService {
    pub fn new(repository: Arc<dyn ImageMetadataRepository>) -> Self {
        Self { repository }
    }

    pub async fn get_all_background_metadata(
        &self,
        prefix: Option<&str>,
    ) -> Result<ImageMetadataIndex, DomainError> {
        self.repository.read_metadata_index(prefix).await
    }

    pub async fn get_background_list_entries(
        &self,
    ) -> Result<Vec<BackgroundListEntry>, DomainError> {
        self.repository.get_background_list_entries().await
    }

    pub async fn get_background_folders(&self) -> Result<BackgroundFoldersPayload, DomainError> {
        self.repository.get_background_folders().await
    }

    pub async fn create_folder(
        &self,
        dto: CreateImageMetadataFolderDto,
    ) -> Result<ImageMetadataFolder, ApplicationError> {
        self.repository
            .create_folder(dto.name.trim())
            .await
            .map_err(Into::into)
    }

    pub async fn update_folder(
        &self,
        dto: UpdateImageMetadataFolderDto,
    ) -> Result<ImageMetadataFolder, ApplicationError> {
        self.repository
            .update_folder(
                dto.id.trim(),
                dto.name.as_deref().map(str::trim),
                dto.thumbnail_file.as_deref().map(str::trim),
            )
            .await
            .map_err(Into::into)
    }

    pub async fn delete_folder(
        &self,
        dto: DeleteImageMetadataFolderDto,
    ) -> Result<(), ApplicationError> {
        self.repository.delete_folder(dto.id.trim()).await?;
        Ok(())
    }

    pub async fn set_folder_thumbnails(
        &self,
        dto: SetImageMetadataFolderThumbnailsDto,
    ) -> Result<(), ApplicationError> {
        let updates = dto
            .updates
            .into_iter()
            .map(|update| {
                (
                    update.id.trim().to_string(),
                    update.thumbnail_file.trim().to_string(),
                )
            })
            .collect();
        self.repository.set_folder_thumbnails(updates).await?;
        Ok(())
    }

    pub async fn assign_images_to_folder(
        &self,
        dto: ImageMetadataFolderAssignmentDto,
    ) -> Result<(), ApplicationError> {
        self.repository
            .assign_images_to_folder(dto.id.trim(), dto.paths)
            .await?;
        Ok(())
    }

    pub async fn unassign_images_from_folder(
        &self,
        dto: ImageMetadataFolderAssignmentDto,
    ) -> Result<(), ApplicationError> {
        self.repository
            .unassign_images_from_folder(dto.id.trim(), dto.paths)
            .await?;
        Ok(())
    }
}
