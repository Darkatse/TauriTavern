use std::path::{Path, PathBuf};

use tokio::fs;

use crate::domain::errors::DomainError;
use crate::infrastructure::logging::logger;
use crate::infrastructure::persistence::file_system::list_files_with_extension;

use super::FileChatRepository;

impl FileChatRepository {
    /// Backup a chat file
    pub(super) async fn backup_chat_file(
        &self,
        chat_path: &Path,
        backup_name: &str,
        backup_key: &str,
    ) -> Result<(), DomainError> {
        if !self.backup_enabled {
            return Ok(());
        }

        // Check if we should backup
        {
            let throttled = self.throttled_backup.lock().await;
            if !throttled.should_backup(backup_key) {
                return Ok(());
            }
        }

        // Get the backup file path
        let backup_path = self.get_backup_path(backup_name);

        // Copy the file
        fs::copy(chat_path, &backup_path).await.map_err(|e| {
            logger::error(&format!("Failed to backup chat file: {}", e));
            DomainError::InternalError(format!("Failed to backup chat file: {}", e))
        })?;

        // Update the last backup time
        {
            let mut throttled = self.throttled_backup.lock().await;
            throttled.update(backup_key);
        }

        // Remove old backups following SillyTavern semantics:
        // 1) per-chat prefix limit
        // 2) global chat_ prefix limit
        let per_chat_prefix = Self::backup_file_prefix(backup_name);
        self.remove_old_backups_with_prefix(&per_chat_prefix, self.max_backups_per_chat)
            .await?;
        self.remove_old_backups_with_prefix(Self::CHAT_BACKUP_PREFIX, self.max_total_backups)
            .await?;

        Ok(())
    }

    /// Remove old backups with a specific file name prefix.
    async fn remove_old_backups_with_prefix(
        &self,
        prefix: &str,
        max_backups: usize,
    ) -> Result<(), DomainError> {
        if max_backups == usize::MAX {
            return Ok(());
        }

        logger::debug(&format!("Removing old backups for prefix: {}", prefix));

        // List all backup files
        let mut matching_backups: Vec<(PathBuf, std::fs::Metadata)> = Vec::new();
        for path in list_files_with_extension(&self.backups_dir, "jsonl").await? {
            if let Ok(metadata) = fs::metadata(&path).await {
                let file_name = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();

                if file_name.starts_with(prefix) {
                    matching_backups.push((path, metadata));
                }
            }
        }

        if matching_backups.len() <= max_backups {
            return Ok(());
        }

        // Sort backups by modification time (oldest first)
        matching_backups.sort_by(|(_, a), (_, b)| {
            a.modified()
                .unwrap_or_else(|_| std::time::SystemTime::UNIX_EPOCH)
                .cmp(
                    &b.modified()
                        .unwrap_or_else(|_| std::time::SystemTime::UNIX_EPOCH),
                )
        });

        while matching_backups.len() > max_backups {
            if let Some((path, _)) = matching_backups.first() {
                let path = path.clone();
                if let Err(e) = fs::remove_file(&path).await {
                    logger::error(&format!("Failed to remove old backup {:?}: {}", path, e));
                } else {
                    logger::debug(&format!("Removed old backup: {:?}", path));
                }
            }
            matching_backups.remove(0);
        }

        Ok(())
    }
}
