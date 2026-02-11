use async_trait::async_trait;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::AppHandle;
use tauri::Manager;
use tokio::fs;
use tokio::sync::Mutex;

use crate::domain::errors::DomainError;
use crate::domain::models::secret::Secrets;
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
        // 先尝试从缓存获取
        {
            let cache = self.cache.lock().await;
            if let Some(secrets) = cache.clone() {
                return Ok(secrets);
            }
        }

        // 如果缓存中没有，从文件加载
        self.ensure_file_exists().await?;

        let secrets = match read_json_file::<Secrets>(&self.secrets_file).await {
            Ok(s) => s,
            Err(e) => {
                logger::error(&format!("Failed to read secrets file: {}", e));
                Secrets::new() // 如果读取失败，返回空的Secrets
            }
        };

        // 更新缓存
        let mut cache = self.cache.lock().await;
        *cache = Some(secrets.clone());

        Ok(secrets)
    }

    async fn write_secret(&self, key: &str, value: &str) -> Result<(), DomainError> {
        let mut secrets = self.load().await?;

        secrets.set(key.to_string(), value.to_string());

        self.save(&secrets).await
    }

    async fn read_secret(&self, key: &str) -> Result<Option<String>, DomainError> {
        let secrets = self.load().await?;

        Ok(secrets.get(key).cloned())
    }

    async fn delete_secret(&self, key: &str) -> Result<(), DomainError> {
        let mut secrets = self.load().await?;

        secrets.delete(key);

        self.save(&secrets).await
    }

    async fn get_secret_state(&self) -> Result<HashMap<String, bool>, DomainError> {
        let secrets = self.load().await?;
        let mut state = secrets.get_state();

        // 确保所有已知密钥都有状态，如果不存在则设置为 false
        use crate::domain::models::secret::SecretKeys;

        // 定义所有已知密钥的列表
        let known_keys = [
            SecretKeys::HORDE,
            SecretKeys::MANCER,
            SecretKeys::VLLM,
            SecretKeys::APHRODITE,
            SecretKeys::TABBY,
            SecretKeys::OPENAI,
            SecretKeys::NOVEL,
            SecretKeys::CLAUDE,
            SecretKeys::OPENROUTER,
            SecretKeys::SCALE,
            SecretKeys::AI21,
            SecretKeys::SCALE_COOKIE,
            SecretKeys::MAKERSUITE,
            SecretKeys::SERPAPI,
            SecretKeys::MISTRALAI,
            SecretKeys::TOGETHERAI,
            SecretKeys::INFERMATICAI,
            SecretKeys::DREAMGEN,
            SecretKeys::CUSTOM,
            SecretKeys::OOBA,
            SecretKeys::NOMICAI,
            SecretKeys::KOBOLDCPP,
            SecretKeys::LLAMACPP,
            SecretKeys::COHERE,
            SecretKeys::PERPLEXITY,
            SecretKeys::GROQ,
            SecretKeys::AZURE_TTS,
            SecretKeys::FEATHERLESS,
            SecretKeys::ZEROONEAI,
            SecretKeys::HUGGINGFACE,
            SecretKeys::STABILITY,
            SecretKeys::CUSTOM_OPENAI_TTS,
            SecretKeys::NANOGPT,
            SecretKeys::TAVILY,
            SecretKeys::BFL,
            SecretKeys::GENERIC,
            SecretKeys::DEEPSEEK,
            SecretKeys::MOONSHOT,
            SecretKeys::SILICONFLOW,
            SecretKeys::ZAI,
            SecretKeys::SERPER,
            SecretKeys::FALAI,
            SecretKeys::XAI,
            SecretKeys::CSRF_SECRET,
        ];

        // 确保所有已知密钥都有状态
        for key in known_keys.iter() {
            state.entry(key.to_string()).or_insert(false);
        }

        // 添加可导出的密钥
        for key in SecretKeys::get_exportable_keys() {
            state.entry(key.to_string()).or_insert(false);
        }

        Ok(state)
    }
}
