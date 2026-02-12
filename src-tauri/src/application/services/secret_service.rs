use std::sync::Arc;

use crate::application::dto::secret_dto::{
    AllSecretsDto, FindSecretResponseDto, SecretStateDto, SecretStateItemDto,
};
use crate::application::errors::ApplicationError;
use crate::domain::models::secret::SecretKeys;
use crate::domain::repositories::secret_repository::SecretRepository;

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

    /// 写入密钥并返回新密钥 ID
    pub async fn write_secret(
        &self,
        key: &str,
        value: &str,
        label: Option<&str>,
    ) -> Result<String, ApplicationError> {
        tracing::info!("Writing secret: {}", key);

        let id = self
            .secret_repository
            .write_secret(key, value, label.unwrap_or("Unlabeled"))
            .await?;
        Ok(id)
    }

    /// 读取密钥状态
    pub async fn read_secret_state(&self) -> Result<SecretStateDto, ApplicationError> {
        tracing::info!("Reading secret state");
        let secrets = self.secret_repository.load().await?;
        let mut states = SecretKeys::known_keys()
            .iter()
            .map(|key| ((*key).to_string(), None))
            .collect::<std::collections::HashMap<String, Option<Vec<SecretStateItemDto>>>>();

        for (key, entries) in secrets.secrets {
            if key == SecretKeys::MIGRATED {
                continue;
            }

            if entries.is_empty() {
                states.insert(key, None);
                continue;
            }

            let can_expose = self.allow_keys_exposure
                || SecretKeys::get_exportable_keys().contains(&key.as_str());
            let items = entries
                .into_iter()
                .map(|entry| SecretStateItemDto {
                    id: entry.id,
                    value: Self::mask_secret_value(&entry.value, can_expose),
                    label: entry.label,
                    active: entry.active,
                })
                .collect::<Vec<_>>();

            states.insert(key, Some(items));
        }

        Ok(SecretStateDto { states })
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
            secrets: secrets.active_secret_values(),
        })
    }

    /// 查找特定密钥
    pub async fn find_secret(
        &self,
        key: &str,
        id: Option<&str>,
    ) -> Result<FindSecretResponseDto, ApplicationError> {
        tracing::info!("Finding secret: {}", key);

        if !self.allow_keys_exposure && !SecretKeys::get_exportable_keys().contains(&key) {
            return Err(ApplicationError::PermissionDenied(
                "Keys exposure not allowed".to_string(),
            ));
        }

        let secret = self.secret_repository.read_secret(key, id).await?;

        match secret {
            Some(value) if !value.is_empty() => Ok(FindSecretResponseDto { value }),
            _ => Err(ApplicationError::NotFound(format!(
                "Secret not found: {}",
                key
            ))),
        }
    }

    pub async fn delete_secret(&self, key: &str, id: Option<&str>) -> Result<(), ApplicationError> {
        tracing::info!("Deleting secret: {}", key);
        self.secret_repository.delete_secret(key, id).await?;
        Ok(())
    }

    pub async fn rotate_secret(&self, key: &str, id: &str) -> Result<(), ApplicationError> {
        tracing::info!("Rotating secret: {}", key);
        self.secret_repository.rotate_secret(key, id).await?;
        Ok(())
    }

    pub async fn rename_secret(
        &self,
        key: &str,
        id: &str,
        label: &str,
    ) -> Result<(), ApplicationError> {
        tracing::info!("Renaming secret: {}", key);
        self.secret_repository.rename_secret(key, id, label).await?;
        Ok(())
    }

    fn mask_secret_value(value: &str, can_expose: bool) -> String {
        if can_expose {
            return value.to_string();
        }

        const THRESHOLD: usize = 10;
        const EXPOSED_SUFFIX: usize = 3;
        let chars = value.chars().collect::<Vec<_>>();
        if chars.len() <= THRESHOLD {
            return "*".repeat(THRESHOLD);
        }

        let suffix = chars[chars.len() - EXPOSED_SUFFIX..]
            .iter()
            .collect::<String>();
        format!("{}{}", "*".repeat(THRESHOLD - EXPOSED_SUFFIX), suffix)
    }
}
