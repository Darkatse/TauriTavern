use std::path::PathBuf;

use crate::chat_directory_identity;
use tt_domain::errors::DomainError;

use super::FileChatRepository;

impl FileChatRepository {
    pub(super) async fn resolve_character_chat_dir_key(
        &self,
        character_name: &str,
    ) -> Result<String, DomainError> {
        chat_directory_identity::resolve_character_chat_dir_key(
            &self.characters_dir,
            &self.chats_dir,
            &self.chat_aliases,
            character_name,
        )
        .await
    }

    pub(super) async fn resolve_character_chat_dir(
        &self,
        character_name: &str,
    ) -> Result<PathBuf, DomainError> {
        let dir_key = self.resolve_character_chat_dir_key(character_name).await?;
        Ok(self.get_character_dir_for_key(&dir_key))
    }

    pub(super) async fn resolve_character_chat_path(
        &self,
        character_name: &str,
        file_name: &str,
    ) -> Result<PathBuf, DomainError> {
        let normalized = Self::normalize_jsonl_file_name(file_name)?;
        let dir = self.resolve_character_chat_dir(character_name).await?;
        Ok(dir.join(normalized))
    }

    pub(super) fn get_chat_path_for_dir_key(
        &self,
        dir_key: &str,
        file_name: &str,
    ) -> Result<PathBuf, DomainError> {
        let normalized = Self::normalize_jsonl_file_name(file_name)?;
        Ok(self.get_character_dir_for_key(dir_key).join(normalized))
    }
}
