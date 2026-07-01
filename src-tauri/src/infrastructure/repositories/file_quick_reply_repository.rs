use async_trait::async_trait;
use std::path::PathBuf;
use tokio::fs;

use crate::infrastructure::persistence::file_system::{delete_file, write_json_file};
use tt_domain::errors::DomainError;
use tt_domain::models::filename::sanitize_filename;
use tt_domain::models::quick_reply::QuickReplySet;
use tt_ports::repositories::quick_reply_repository::QuickReplyRepository;

pub struct FileQuickReplyRepository {
    quick_replies_dir: PathBuf,
}

impl FileQuickReplyRepository {
    pub fn new(quick_replies_dir: PathBuf) -> Self {
        Self { quick_replies_dir }
    }

    async fn ensure_directory_exists(&self) -> Result<(), DomainError> {
        if !self.quick_replies_dir.exists() {
            fs::create_dir_all(&self.quick_replies_dir)
                .await
                .map_err(|error| {
                    DomainError::InternalError(format!(
                        "Failed to create quick reply directory {}: {}",
                        self.quick_replies_dir.display(),
                        error
                    ))
                })?;
        }

        Ok(())
    }

    fn get_quick_reply_path(&self, name: &str) -> Result<PathBuf, DomainError> {
        let filename = sanitize_filename(&format!("{name}.json"));
        if filename.is_empty() {
            return Err(DomainError::InvalidData(
                "Quick Reply set name is invalid".to_string(),
            ));
        }

        Ok(self.quick_replies_dir.join(filename))
    }
}

#[async_trait]
impl QuickReplyRepository for FileQuickReplyRepository {
    async fn save_quick_reply_set(&self, set: &QuickReplySet) -> Result<(), DomainError> {
        self.ensure_directory_exists().await?;
        let file_path = self.get_quick_reply_path(&set.name)?;
        write_json_file(&file_path, &set.data).await
    }

    async fn delete_quick_reply_set(&self, name: &str) -> Result<(), DomainError> {
        let file_path = self.get_quick_reply_path(name)?;
        if !file_path.exists() {
            return Ok(());
        }

        delete_file(&file_path).await
    }
}
