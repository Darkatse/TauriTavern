use std::sync::Arc;

use crate::application::dto::secret_dto::{AllSecretsDto, FindSecretResponseDto, SecretStateDto};
use crate::application::errors::ApplicationError;
use crate::domain::models::secret::SecretKeys;
use crate::domain::repositories::secret_repository::SecretRepository;
use crate::infrastructure::logging::logger;

pub struct SecretService {
    secret_repository: Arc<dyn SecretRepository>,
    allow_keys_exposure: bool,
}

impl SecretService {
    pub fn new(secret_repository: Arc<dyn SecretRepository>, allow_keys_exposure: bool) -> Self {
        Self {
            secret_repository,
            allow_keys_exposure,
        }
    }

    /// 写入密钥
    pub async fn write_secret(&self, key: &str, value: &str) -> Result<(), ApplicationError> {
        tracing::info!("Writing secret: {}", key);

        self.secret_repository.write_secret(key, value).await?;

        Ok(())
    }

    /// 读取密钥状态
    pub async fn read_secret_state(&self) -> Result<SecretStateDto, ApplicationError> {
        tracing::info!("Reading secret state");

        let state = self.secret_repository.get_secret_state().await?;

        Ok(SecretStateDto { states: state })
    }

    /// 查看所有密钥
    pub async fn view_secrets(&self) -> Result<AllSecretsDto, ApplicationError> {
        tracing::info!("Viewing all secrets");

        if !self.allow_keys_exposure {
            return Err(ApplicationError::PermissionDenied(
                "Keys exposure not allowed".to_string(),
            ));
        }

        let secrets = self.secret_repository.load().await?;

        Ok(AllSecretsDto {
            secrets: secrets.secrets,
        })
    }

    /// 查找特定密钥
    pub async fn find_secret(&self, key: &str) -> Result<FindSecretResponseDto, ApplicationError> {
        tracing::info!("Finding secret: {}", key);

        // 检查是否允许暴露密钥
        let exportable_keys = SecretKeys::get_exportable_keys();
        if !self.allow_keys_exposure && !exportable_keys.contains(&key) {
            return Err(ApplicationError::PermissionDenied(
                "Keys exposure not allowed".to_string(),
            ));
        }

        let secret = self.secret_repository.read_secret(key).await?;

        match secret {
            Some(value) if !value.is_empty() => Ok(FindSecretResponseDto { value }),
            _ => Err(ApplicationError::NotFound(format!(
                "Secret not found: {}",
                key
            ))),
        }
    }
}
