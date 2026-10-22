use std::path::Path;
use std::sync::Arc;

use crate::dto::world_info_dto::WorldInfoJsonDto;
use crate::errors::ApplicationError;
use tt_contracts::byte_commit::CommitBegin;
use tt_domain::models::world_info::{sanitize_world_info_import_name, sanitize_world_info_name};
use tt_ports::repositories::world_info_repository::WorldInfoRepository;

pub struct WorldInfoService {
    world_info_repository: Arc<dyn WorldInfoRepository>,
}

impl WorldInfoService {
    pub fn new(world_info_repository: Arc<dyn WorldInfoRepository>) -> Self {
        Self {
            world_info_repository,
        }
    }

    pub async fn get_world_info(&self, name: &str) -> Result<WorldInfoJsonDto, ApplicationError> {
        let bytes = self
            .world_info_repository
            .read_world_info_json(name)
            .await?
            .unwrap_or_else(|| br#"{"entries":{}}"#.to_vec());

        Ok(WorldInfoJsonDto { bytes })
    }

    pub fn normalize_world_info_name(
        &self,
        name: &str,
        import_filename: bool,
    ) -> Result<String, ApplicationError> {
        let normalized = if import_filename {
            sanitize_world_info_import_name(name)
        } else {
            sanitize_world_info_name(name)
        };

        if normalized.is_empty() {
            return Err(ApplicationError::ValidationError(
                "World file must have a name".to_string(),
            ));
        }

        Ok(normalized)
    }

    pub async fn begin_commit(&self) -> Result<CommitBegin, ApplicationError> {
        Ok(self.world_info_repository.begin_commit().await?)
    }

    pub async fn append_commit(
        &self,
        session_id: &str,
        offset: u64,
        bytes: &[u8],
    ) -> Result<u64, ApplicationError> {
        Ok(self
            .world_info_repository
            .append_commit(session_id, offset, bytes)
            .await?)
    }

    pub async fn finish_commit(
        &self,
        session_id: &str,
        expected_size: u64,
    ) -> Result<(), ApplicationError> {
        self.world_info_repository
            .finish_commit(session_id, expected_size)
            .await?;
        Ok(())
    }

    pub async fn abort_commit(&self, session_id: &str) -> Result<(), ApplicationError> {
        Ok(self.world_info_repository.abort_commit(session_id).await?)
    }

    pub async fn delete_world_info(&self, name: &str) -> Result<(), ApplicationError> {
        self.world_info_repository.delete_world_info(name).await?;
        Ok(())
    }

    pub async fn import_world_info(
        &self,
        file_path: &str,
        original_filename: &str,
        converted_data: Option<String>,
    ) -> Result<String, ApplicationError> {
        let has_converted_data = converted_data
            .as_deref()
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false);

        if file_path.trim().is_empty() && !has_converted_data {
            return Err(ApplicationError::ValidationError(
                "World info import file path is required".to_string(),
            ));
        }

        if original_filename.is_empty() {
            return Err(ApplicationError::ValidationError(
                "World file must have a name".to_string(),
            ));
        }

        let imported_name = self
            .world_info_repository
            .import_world_info(
                Path::new(file_path),
                original_filename,
                converted_data.as_deref(),
            )
            .await?;

        Ok(imported_name)
    }
}
