use std::io;
use std::path::Path;

use tokio::fs;
use tt_domain::errors::DomainError;

use crate::chat_format_importers::validate_chat_jsonl_header_line;
use crate::file_system::move_file_no_replace_with_fallback;

use super::FileChatRepository;
use super::backup_codec::restore_staging_file_name;
use super::windowed_payload_io::read_first_line_and_end_offset;

impl FileChatRepository {
    pub(super) async fn restore_character_chat_backup_file(
        &self,
        backup_file_name: &str,
        character_name: &str,
        character_display_name: &str,
    ) -> Result<Vec<String>, DomainError> {
        self.ensure_directory_exists().await?;
        let character_dir = self.resolve_character_chat_dir(character_name).await?;
        fs::create_dir_all(&character_dir).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to create character chat directory {}: {error}",
                character_dir.display()
            ))
        })?;

        let dir_key = self.resolve_character_chat_dir_key(character_name).await?;
        let file_stem =
            self.next_import_chat_file_stem_in_dir(&dir_key, character_display_name, 0)?;
        let target_path = self.get_chat_path_for_dir_key(&dir_key, &file_stem)?;
        let stage_path = self.create_restore_staging_path().await?;
        let _write_guard = self.acquire_payload_mutation_lock(&target_path).await;

        self.copy_chat_backup_to_path(backup_file_name, &stage_path)
            .await?;
        if let Err(error) = validate_restored_character_chat(&stage_path).await {
            remove_restore_stage(&stage_path).await;
            return Err(error);
        }
        if let Err(error) = move_file_no_replace_with_fallback(&stage_path, &target_path).await {
            remove_restore_stage(&stage_path).await;
            return Err(error);
        }

        self.remove_summary_cache_for_path(&target_path).await;
        Ok(vec![Self::normalize_jsonl_file_name(&file_stem)?])
    }

    pub(super) async fn restore_group_chat_backup_file(
        &self,
        backup_file_name: &str,
    ) -> Result<String, DomainError> {
        self.ensure_directory_exists().await?;
        let chat_id = self.next_group_chat_id()?;
        let target_path = self.get_group_chat_path(&chat_id)?;
        let stage_path = self.create_restore_staging_path().await?;
        let _write_guard = self.acquire_payload_mutation_lock(&target_path).await;

        self.copy_chat_backup_to_path(backup_file_name, &stage_path)
            .await?;
        if let Err(error) = move_file_no_replace_with_fallback(&stage_path, &target_path).await {
            remove_restore_stage(&stage_path).await;
            return Err(error);
        }

        self.remove_summary_cache_for_path(&target_path).await;
        Ok(chat_id)
    }

    async fn create_restore_staging_path(&self) -> Result<std::path::PathBuf, DomainError> {
        fs::create_dir_all(&self.chat_commit_staging_dir)
            .await
            .map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to create chat restore staging directory {}: {error}",
                    self.chat_commit_staging_dir.display()
                ))
            })?;
        Ok(self
            .chat_commit_staging_dir
            .join(restore_staging_file_name()))
    }
}

async fn validate_restored_character_chat(path: &Path) -> Result<(), DomainError> {
    let (header, _) = read_first_line_and_end_offset(path).await?;
    validate_chat_jsonl_header_line(&header)
}

async fn remove_restore_stage(path: &Path) {
    match fs::remove_file(path).await {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => tracing::error!(
            path = %path.display(),
            error = %error,
            "Failed to remove rejected chat backup restore stage",
        ),
    }
}
