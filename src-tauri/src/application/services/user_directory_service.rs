use std::sync::Arc;

use crate::domain::repositories::user_directory_repository::UserDirectoryRepository;
use crate::application::dto::user_directory_dto::UserDirectoryDto;
use crate::application::errors::ApplicationError;
use crate::infrastructure::logging::logger;

pub struct UserDirectoryService {
    user_directory_repository: Arc<dyn UserDirectoryRepository>,
}

impl UserDirectoryService {
    pub fn new(user_directory_repository: Arc<dyn UserDirectoryRepository>) -> Self {
        Self {
            user_directory_repository,
        }
    }
    
    pub async fn get_user_directory(&self, handle: &str) -> Result<UserDirectoryDto, ApplicationError> {
        tracing::info!("Getting user directory for: {}", handle);
        
        let directory = self.user_directory_repository.get_user_directory(handle).await?;
        
        Ok(UserDirectoryDto::from(directory))
    }
    
    pub async fn get_default_user_directory(&self) -> Result<UserDirectoryDto, ApplicationError> {
        tracing::info!("Getting default user directory");
        
        let directory = self.user_directory_repository.get_default_user_directory().await?;
        
        Ok(UserDirectoryDto::from(directory))
    }
    
    pub async fn ensure_user_directories_exist(&self, handle: &str) -> Result<(), ApplicationError> {
        tracing::info!("Ensuring directories exist for user: {}", handle);
        
        self.user_directory_repository.ensure_user_directories_exist(handle).await?;
        
        Ok(())
    }
    
    pub async fn ensure_default_user_directories_exist(&self) -> Result<(), ApplicationError> {
        tracing::info!("Ensuring directories exist for default user");
        
        self.user_directory_repository.ensure_default_user_directories_exist().await?;
        
        Ok(())
    }
}
