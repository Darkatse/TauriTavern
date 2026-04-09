use async_trait::async_trait;
use std::io;
use std::path::{Path, PathBuf};

use tokio::fs;

use crate::domain::errors::DomainError;
use crate::domain::repositories::prompt_cache_repository::{
    PromptCacheKey, PromptCacheRepository, PromptDigestSnapshot,
};
use crate::infrastructure::persistence::file_system::{
    replace_file_with_fallback, unique_temp_path,
};

pub struct FilePromptCacheRepository {
    base_dir: PathBuf,
}

impl FilePromptCacheRepository {
    pub fn new(base_dir: PathBuf) -> Self {
        Self { base_dir }
    }

    fn path_for_key(&self, key: PromptCacheKey) -> PathBuf {
        self.base_dir.join(prompt_cache_file_name(key))
    }
}

fn prompt_cache_file_name(key: PromptCacheKey) -> &'static str {
    match key {
        PromptCacheKey::Claude => "claude.json",
        PromptCacheKey::OpenRouterClaude => "openrouter-claude.json",
    }
}

async fn read_optional_json_file<T: for<'de> serde::Deserialize<'de>>(
    path: &Path,
) -> Result<Option<T>, DomainError> {
    let contents = match fs::read_to_string(path).await {
        Ok(contents) => contents,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(DomainError::InternalError(format!(
                "Failed to read file {:?}: {}",
                path, error
            )));
        }
    };

    let parsed = serde_json::from_str::<T>(&contents).map_err(|error| {
        DomainError::InvalidData(format!("Invalid JSON in file {:?}: {}", path, error))
    })?;

    Ok(Some(parsed))
}

#[async_trait]
impl PromptCacheRepository for FilePromptCacheRepository {
    async fn load_prompt_digests(
        &self,
        key: PromptCacheKey,
    ) -> Result<Option<PromptDigestSnapshot>, DomainError> {
        let path = self.path_for_key(key);
        read_optional_json_file(&path).await
    }

    async fn save_prompt_digests(
        &self,
        key: PromptCacheKey,
        snapshot: PromptDigestSnapshot,
    ) -> Result<(), DomainError> {
        let path = self.path_for_key(key);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await.map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to create directory {:?}: {}",
                    parent, error
                ))
            })?;
        }

        let json = serde_json::to_vec_pretty(&snapshot).map_err(|error| {
            DomainError::InvalidData(format!(
                "Failed to serialize prompt cache snapshot: {}",
                error
            ))
        })?;

        let temp_path = unique_temp_path(&path, "prompt-cache.json");
        fs::write(&temp_path, json).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to write prompt cache snapshot {:?}: {}",
                temp_path, error
            ))
        })?;

        replace_file_with_fallback(&temp_path, &path).await?;
        Ok(())
    }
}
