use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

use crate::application::dto::user_dto::{CreateUserDto, UpdateUserDto, UserDto};
use crate::application::errors::ApplicationError;
use crate::domain::models::user::User;
use crate::domain::repositories::user_repository::UserRepository;

pub struct UserService {
    user_repository: Arc<dyn UserRepository>,
}

impl UserService {
    pub fn new(user_repository: Arc<dyn UserRepository>) -> Self {
        Self { user_repository }
    }

    pub async fn create_user(&self, dto: CreateUserDto) -> Result<UserDto, ApplicationError> {
        tracing::info!("Creating user: {}", dto.username);

        // Check if username already exists
        let users = self.user_repository.find_all().await?;
        if users.iter().any(|u| u.username == dto.username) {
            return Err(ApplicationError::ValidationError(format!(
                "Username already exists: {}",
                dto.username
            )));
        }

        let now = Utc::now();
        let user = User {
            id: Uuid::new_v4().to_string(),
            username: dto.username,
            avatar: dto.avatar,
            created_at: now,
            updated_at: now,
            settings: Default::default(),
        };

        self.user_repository.save(&user).await?;

        Ok(UserDto::from(user))
    }

    pub async fn get_user(&self, id: &str) -> Result<UserDto, ApplicationError> {
        tracing::info!("Getting user: {}", id);

        let user = self.user_repository.find_by_id(id).await?;

        Ok(UserDto::from(user))
    }

    pub async fn get_user_by_username(&self, username: &str) -> Result<UserDto, ApplicationError> {
        tracing::info!("Getting user by username: {}", username);

        let user = self.user_repository.find_by_username(username).await?;

        Ok(UserDto::from(user))
    }

    pub async fn get_all_users(&self) -> Result<Vec<UserDto>, ApplicationError> {
        tracing::info!("Getting all users");

        let users = self.user_repository.find_all().await?;

        Ok(users.into_iter().map(UserDto::from).collect())
    }

    pub async fn update_user(&self, dto: UpdateUserDto) -> Result<UserDto, ApplicationError> {
        tracing::info!("Updating user: {}", dto.id);

        let mut user = self.user_repository.find_by_id(&dto.id).await?;

        if let Some(username) = dto.username {
            // Check if username already exists
            let users = self.user_repository.find_all().await?;
            if users
                .iter()
                .any(|u| u.username == username && u.id != dto.id)
            {
                return Err(ApplicationError::ValidationError(format!(
                    "Username already exists: {}",
                    username
                )));
            }

            user.username = username;
        }

        if let Some(avatar) = dto.avatar {
            user.avatar = Some(avatar);
        }

        if let Some(settings) = dto.settings {
            user.settings = settings.into();
        }

        user.updated_at = Utc::now();

        self.user_repository.update(&user).await?;

        Ok(UserDto::from(user))
    }

    pub async fn delete_user(&self, id: &str) -> Result<(), ApplicationError> {
        tracing::info!("Deleting user: {}", id);

        self.user_repository.delete(id).await?;

        Ok(())
    }
}
