use serde_json::{json, Value};
use std::path::Path;
use std::sync::Arc;

use crate::application::errors::ApplicationError;
use crate::domain::models::world_info::WorldInfo;
use crate::domain::repositories::world_info_repository::WorldInfoRepository;

pub struct WorldInfoService {
    world_info_repository: Arc<dyn WorldInfoRepository>,
}

impl WorldInfoService {
    pub fn new(world_info_repository: Arc<dyn WorldInfoRepository>) -> Self {
        Self {
            world_info_repository,
        }
    }

    pub async fn get_world_info(&self, name: &str) -> Result<Value, ApplicationError> {
        let world_info = self
            .world_info_repository
            .get_world_info(name, true)
            .await?
            .unwrap_or_else(|| json!({ "entries": {} }));

        Ok(world_info)
    }

    pub async fn save_world_info(&self, name: &str, data: Value) -> Result<(), ApplicationError> {
        let world_info = WorldInfo::new(name.to_string(), data);
        world_info
            .validate()
            .map_err(ApplicationError::ValidationError)?;

        self.world_info_repository
            .save_world_info(&world_info.name, &world_info.data)
            .await?;

        Ok(())
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

        if original_filename.trim().is_empty() {
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
