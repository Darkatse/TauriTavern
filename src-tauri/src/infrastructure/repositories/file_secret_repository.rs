use async_trait::async_trait;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::AppHandle;
use tauri::Manager;
use tokio::fs;
use tokio::sync::Mutex;

use crate::domain::errors::DomainError;
use crate::domain::models::secret::{SecretEntry, SecretKeys, Secrets};
use crate::domain::repositories::secret_repository::SecretRepository;
use crate::infrastructure::logging::logger;
use crate::infrastructure::persistence::file_system::{read_json_file, write_json_file};

pub struct FileSecretRepository {
    secrets_file: PathBuf,
    cache: Arc<Mutex<Option<Secrets>>>,
}

impl FileSecretRepository {
    pub fn new(app_handle: AppHandle) -> Self {
        // 使用 Tauri 的 app_data_dir 获取应用数据目录
        let app_data_dir = app_handle
            .path()
            .app_data_dir()
            .expect("Failed to get app data directory");

        // 构建 secrets.json 文件路径
        let secrets_file = app_data_dir
            .join("data")
            .join("default-user")
            .join("secrets.json");

        tracing::info!(
            "Secret repository initialized with secrets file: {:?}",
            secrets_file
        );

        Self {
            secrets_file,
            cache: Arc::new(Mutex::new(None)),
        }
    }

    async fn ensure_file_exists(&self) -> Result<(), DomainError> {
        if !self.secrets_file.exists() {
            tracing::info!("Creating secrets file: {:?}", self.secrets_file);

            // 确保父目录存在
            if let Some(parent) = self.secrets_file.parent() {
                if !parent.exists() {
                    fs::create_dir_all(parent).await.map_err(|e| {
                        tracing::error!(
                            "Failed to create parent directory for secrets file: {}",
                            e
                        );
                        DomainError::InternalError(format!("Failed to create directory: {}", e))
                    })?;
                }
            }

            // 创建空的secrets文件
            let empty_secrets = Secrets::new();
            write_json_file(&self.secrets_file, &empty_secrets).await?;
        }

        Ok(())
    }
}

#[async_trait]
impl SecretRepository for FileSecretRepository {
    async fn save(&self, secrets: &Secrets) -> Result<(), DomainError> {
        self.ensure_file_exists().await?;

        write_json_file(&self.secrets_file, secrets).await?;

        // 更新缓存
        let mut cache = self.cache.lock().await;
        *cache = Some(secrets.clone());

        Ok(())
    }

    async fn load(&self) -> Result<Secrets, DomainError> {
        {
            let cache = self.cache.lock().await;
            if let Some(secrets) = cache.clone() {
                return Ok(secrets);
            }
        }

        self.ensure_file_exists().await?;

        let raw = match read_json_file::<Value>(&self.secrets_file).await {
            Ok(value) => value,
            Err(e) => {
                logger::error(&format!("Failed to read secrets file: {}", e));
                Value::Object(Default::default())
            }
        };
        let (secrets, migrated) = Self::deserialize_compat(raw);

        if migrated {
            if let Err(error) = self.save(&secrets).await {
                logger::error(&format!(
                    "Failed to persist migrated secrets file: {}",
                    error
                ));
            }
        } else {
            let mut cache = self.cache.lock().await;
            *cache = Some(secrets.clone());
        }

        Ok(secrets)
    }

    async fn write_secret(
        &self,
        key: &str,
        value: &str,
        label: &str,
    ) -> Result<String, DomainError> {
        let mut secrets = self.load().await?;
        let id = secrets.write_secret(key.to_string(), value.to_string(), label.to_string());
        self.save(&secrets).await?;
        Ok(id)
    }

    async fn read_secret(
        &self,
        key: &str,
        id: Option<&str>,
    ) -> Result<Option<String>, DomainError> {
        let secrets = self.load().await?;
        Ok(secrets.read_secret(key, id))
    }

    async fn delete_secret(&self, key: &str, id: Option<&str>) -> Result<(), DomainError> {
        let mut secrets = self.load().await?;
        if secrets.delete_secret(key, id) {
            self.save(&secrets).await?;
        }
        Ok(())
    }

    async fn rotate_secret(&self, key: &str, id: &str) -> Result<(), DomainError> {
        let mut secrets = self.load().await?;
        if secrets.rotate_secret(key, id) {
            self.save(&secrets).await?;
        }
        Ok(())
    }

    async fn rename_secret(&self, key: &str, id: &str, label: &str) -> Result<(), DomainError> {
        let mut secrets = self.load().await?;
        if secrets.rename_secret(key, id, label.to_string()) {
            self.save(&secrets).await?;
        }
        Ok(())
    }
}

impl FileSecretRepository {
    fn deserialize_compat(raw: Value) -> (Secrets, bool) {
        let mut secrets = Secrets::new();
        let mut migrated = false;

        let Value::Object(entries) = raw else {
            return (secrets, migrated);
        };

        for (key, value) in entries {
            if key == SecretKeys::MIGRATED {
                continue;
            }

            match value {
                Value::Array(array) => {
                    let mut normalized = Vec::new();
                    for item in array {
                        if let Ok(entry) = serde_json::from_value::<SecretEntry>(item) {
                            normalized.push(entry);
                        }
                    }

                    if !normalized.is_empty() {
                        Secrets::normalize_entries(&mut normalized);
                        secrets.secrets.insert(key, normalized);
                    }
                }
                Value::String(legacy) => {
                    if !legacy.trim().is_empty() {
                        secrets.write_secret(key.clone(), legacy, key);
                        migrated = true;
                    }
                }
                _ => {}
            }
        }

        (secrets, migrated)
    }
}
