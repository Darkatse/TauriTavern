use async_trait::async_trait;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs;
use tokio::sync::Mutex;

use crate::domain::errors::DomainError;
use crate::domain::models::user::User;
use crate::domain::repositories::user_repository::UserRepository;
use crate::infrastructure::logging::logger;
use crate::infrastructure::persistence::file_system::{
    delete_file, list_files_with_extension, read_json_file, write_json_file,
};

pub struct FileUserRepository {
    users_dir: PathBuf,
    cache: Arc<Mutex<Vec<User>>>,
}

impl FileUserRepository {
    pub fn new(users_dir: PathBuf) -> Self {
        Self {
            users_dir,
            cache: Arc::new(Mutex::new(Vec::new())),
        }
    }

    async fn ensure_directory_exists(&self) -> Result<(), DomainError> {
        if !self.users_dir.exists() {
            tracing::info!("Creating users directory: {:?}", self.users_dir);
            fs::create_dir_all(&self.users_dir).await.map_err(|e| {
                tracing::error!("Failed to create users directory: {}", e);
                DomainError::InternalError(format!("Failed to create users directory: {}", e))
            })?;
        }
        Ok(())
    }

    fn get_user_path(&self, id: &str) -> PathBuf {
        self.users_dir.join(format!("{}.json", id))
    }

    async fn load_all_users(&self) -> Result<Vec<User>, DomainError> {
        self.ensure_directory_exists().await?;

        let user_files = list_files_with_extension(&self.users_dir, "json").await?;
        let mut users = Vec::new();

        for file_path in user_files {
            match read_json_file::<User>(&file_path).await {
                Ok(user) => {
                    users.push(user);
                }
                Err(e) => {
                    tracing::error!("Failed to load user from {:?}: {}", file_path, e);
                    // Continue loading other users
                }
            }
        }

        Ok(users)
    }
}

#[async_trait]
impl UserRepository for FileUserRepository {
    async fn save(&self, user: &User) -> Result<(), DomainError> {
        self.ensure_directory_exists().await?;

        let file_path = self.get_user_path(&user.id);
        write_json_file(&file_path, user).await?;

        // Update cache
        let mut cache = self.cache.lock().await;
        if let Some(index) = cache.iter().position(|u| u.id == user.id) {
            cache[index] = user.clone();
        } else {
            cache.push(user.clone());
        }

        Ok(())
    }

    async fn find_by_id(&self, id: &str) -> Result<User, DomainError> {
        // Try to get from cache first
        {
            let cache = self.cache.lock().await;
            if let Some(user) = cache.iter().find(|u| u.id == id) {
                return Ok(user.clone());
            }
        }

        // If not in cache, load from file
        let file_path = self.get_user_path(id);
        if !file_path.exists() {
            return Err(DomainError::NotFound(format!("User not found: {}", id)));
        }

        let user = read_json_file::<User>(&file_path).await?;

        // Update cache
        let mut cache = self.cache.lock().await;
        cache.push(user.clone());

        Ok(user)
    }

    async fn find_by_username(&self, username: &str) -> Result<User, DomainError> {
        let all_users = self.find_all().await?;

        all_users
            .into_iter()
            .find(|user| user.username == username)
            .ok_or_else(|| DomainError::NotFound(format!("User not found: {}", username)))
    }

    async fn find_all(&self) -> Result<Vec<User>, DomainError> {
        // Check if cache is empty
        {
            let cache = self.cache.lock().await;
            if !cache.is_empty() {
                return Ok(cache.clone());
            }
        }

        // Load all users
        let users = self.load_all_users().await?;

        // Update cache
        let mut cache = self.cache.lock().await;
        *cache = users.clone();

        Ok(users)
    }

    async fn delete(&self, id: &str) -> Result<(), DomainError> {
        let file_path = self.get_user_path(id);
        if !file_path.exists() {
            return Err(DomainError::NotFound(format!("User not found: {}", id)));
        }

        delete_file(&file_path).await?;

        // Update cache
        let mut cache = self.cache.lock().await;
        cache.retain(|u| u.id != id);

        Ok(())
    }

    async fn update(&self, user: &User) -> Result<(), DomainError> {
        // Check if user exists
        let file_path = self.get_user_path(&user.id);
        if !file_path.exists() {
            return Err(DomainError::NotFound(format!(
                "User not found: {}",
                user.id
            )));
        }

        // Save the updated user
        self.save(user).await
    }
}
