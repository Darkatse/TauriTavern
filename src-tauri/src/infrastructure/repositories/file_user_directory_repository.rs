use std::path::{Path, PathBuf};
use std::sync::Arc;
use async_trait::async_trait;
use tokio::fs;
use tauri::AppHandle;
use tauri::Manager;

use crate::domain::errors::DomainError;
use crate::domain::models::user_directory::UserDirectory;
use crate::domain::repositories::user_directory_repository::UserDirectoryRepository;
use crate::infrastructure::logging::logger;

pub struct FileUserDirectoryRepository {
    app_handle: AppHandle,
    data_root: PathBuf,
}

impl FileUserDirectoryRepository {
    pub fn new(app_handle: AppHandle) -> Self {
        // 使用 Tauri 的 app_data_dir 获取应用数据目录
        let app_data_dir = app_handle.path().app_data_dir()
            .expect("Failed to get app data directory");

        // 构建数据根目录
        let data_root = app_data_dir.join("data");

        tracing::info!("User directory repository initialized with data root: {:?}", data_root);

        Self {
            app_handle,
            data_root,
        }
    }

    async fn create_directories(&self, directories: &UserDirectory) -> Result<(), DomainError> {
        tracing::info!("Creating directories for user: {}", directories.handle);

        for dir in directories.all_directories() {
            if !dir.exists() {
                tracing::debug!("Creating directory: {:?}", dir);
                fs::create_dir_all(dir).await.map_err(|e| {
                    tracing::error!("Failed to create directory {:?}: {}", dir, e);
                    DomainError::InternalError(format!("Failed to create directory: {}", e))
                })?;
            }
        }

        tracing::info!("Successfully created directories for user: {}", directories.handle);
        Ok(())
    }
}

#[async_trait]
impl UserDirectoryRepository for FileUserDirectoryRepository {
    async fn get_user_directory(&self, handle: &str) -> Result<UserDirectory, DomainError> {
        tracing::debug!("Getting user directory for: {}", handle);
        Ok(UserDirectory::new(&self.data_root, handle))
    }

    async fn get_default_user_directory(&self) -> Result<UserDirectory, DomainError> {
        logger::debug("Getting default user directory");
        Ok(UserDirectory::default_user(&self.data_root))
    }

    async fn ensure_user_directories_exist(&self, handle: &str) -> Result<(), DomainError> {
        let directories = self.get_user_directory(handle).await?;
        self.create_directories(&directories).await
    }

    async fn ensure_default_user_directories_exist(&self) -> Result<(), DomainError> {
        let directories = self.get_default_user_directory().await?;
        self.create_directories(&directories).await
    }
}
