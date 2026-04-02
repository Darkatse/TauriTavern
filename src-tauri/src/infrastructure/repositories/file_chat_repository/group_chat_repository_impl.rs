use std::collections::HashSet;
use std::path::Path;

use async_trait::async_trait;
use serde_json::Value;
use tokio::fs;

use crate::domain::errors::DomainError;
use crate::domain::repositories::chat_repository::ChatRepository;
use crate::domain::repositories::chat_types::{
    ChatMessageSearchHit, ChatMessageSearchQuery, ChatPayloadChunk, ChatPayloadCursor,
    ChatPayloadPatchOp, ChatPayloadTail, ChatSearchResult, FindLastMessageQuery,
    LocatedChatMessage, PinnedGroupChat,
};
use crate::domain::repositories::group_chat_repository::GroupChatRepository;
use crate::infrastructure::logging::logger;

use super::FileChatRepository;

#[async_trait]
impl GroupChatRepository for FileChatRepository {
    async fn list_group_chat_summaries(
        &self,
        chat_ids: Option<&[String]>,
        include_metadata: bool,
    ) -> Result<Vec<ChatSearchResult>, DomainError> {
        let descriptors = self.list_group_chat_files(chat_ids).await?;
        let mut results = Vec::with_capacity(descriptors.len());
        for descriptor in descriptors {
            results.push(self.get_chat_summary(&descriptor, include_metadata).await?);
        }
        results.sort_by(|a, b| b.date.cmp(&a.date));
        self.flush_summary_index_if_needed().await?;
        Ok(results)
    }

    async fn list_recent_group_chat_summaries(
        &self,
        chat_ids: Option<&[String]>,
        include_metadata: bool,
        max_entries: usize,
        pinned: &[PinnedGroupChat],
    ) -> Result<Vec<ChatSearchResult>, DomainError> {
        let descriptors = self.list_group_chat_files(chat_ids).await?;
        let pinned_keys: HashSet<String> = pinned
            .iter()
            .filter_map(|entry| Self::group_recent_pin_key(&entry.chat_id))
            .collect();

        let selected = self
            .select_recent_descriptors(descriptors, max_entries, |descriptor| {
                Self::group_recent_pin_key(&descriptor.file_name)
                    .map(|key| pinned_keys.contains(&key))
                    .unwrap_or(false)
            })
            .await?;

        let mut results = Vec::with_capacity(selected.len());
        for descriptor in selected {
            results.push(self.get_chat_summary(&descriptor, include_metadata).await?);
        }
        results.sort_by(|a, b| b.date.cmp(&a.date));
        self.flush_summary_index_if_needed().await?;
        Ok(results)
    }

    async fn search_group_chats(
        &self,
        query: &str,
        chat_ids: Option<&[String]>,
    ) -> Result<Vec<ChatSearchResult>, DomainError> {
        logger::debug("Searching group chats with streaming scanner");

        let normalized_query = Self::normalize_search_query(query);
        let search_cache_key = Self::group_search_cache_key(&normalized_query, chat_ids);
        if let Some(cached) = self.get_cached_search_results(&search_cache_key).await {
            return Ok(cached);
        }

        let descriptors = self.list_group_chat_files(chat_ids).await?;
        let fragments = Self::search_fragments(&normalized_query);
        let mut results = Vec::new();

        for descriptor in descriptors {
            let entry = self.get_chat_summary_entry(&descriptor, true).await?;
            let mut summary = entry.summary.clone();
            summary.chat_metadata = None;
            if fragments.is_empty() {
                results.push(summary);
                continue;
            }

            let file_stem = Self::strip_jsonl_extension(&descriptor.file_name);
            if Self::file_stem_matches_all(file_stem, &fragments) {
                results.push(summary);
                continue;
            }

            if !entry
                .fingerprint
                .as_ref()
                .expect("fingerprint is required for search")
                .might_match_fragments(&fragments)
            {
                continue;
            }

            if self
                .file_matches_query(&descriptor.path, file_stem, &fragments)
                .await?
            {
                results.push(summary);
            }
        }

        results.sort_by(|a, b| b.date.cmp(&a.date));
        self.cache_search_results(search_cache_key, results.clone())
            .await;
        self.flush_summary_index_if_needed().await?;
        Ok(results)
    }

    async fn get_group_chat_payload_path(
        &self,
        chat_id: &str,
    ) -> Result<std::path::PathBuf, DomainError> {
        let path = self.get_group_chat_path(chat_id);
        if !path.exists() {
            return Err(DomainError::NotFound(format!(
                "Group chat not found: {}",
                chat_id
            )));
        }

        Ok(path)
    }

    async fn get_group_chat_payload_tail_lines(
        &self,
        chat_id: &str,
        max_lines: usize,
    ) -> Result<ChatPayloadTail, DomainError> {
        self.get_group_payload_tail_lines(chat_id, max_lines).await
    }

    async fn get_group_chat_payload_before_lines(
        &self,
        chat_id: &str,
        cursor: ChatPayloadCursor,
        max_lines: usize,
    ) -> Result<ChatPayloadChunk, DomainError> {
        self.get_group_payload_before_lines(chat_id, cursor, max_lines)
            .await
    }

    async fn save_group_chat_payload_windowed(
        &self,
        chat_id: &str,
        cursor: ChatPayloadCursor,
        header: String,
        lines: Vec<String>,
        force: bool,
    ) -> Result<ChatPayloadCursor, DomainError> {
        self.save_group_payload_windowed(chat_id, cursor, header, lines, force)
            .await
    }

    async fn patch_group_chat_payload_windowed(
        &self,
        chat_id: &str,
        cursor: ChatPayloadCursor,
        header: String,
        op: ChatPayloadPatchOp,
        force: bool,
    ) -> Result<ChatPayloadCursor, DomainError> {
        self.patch_group_payload_windowed(chat_id, cursor, header, op, force)
            .await
    }

    async fn save_group_chat_payload_from_path(
        &self,
        chat_id: &str,
        source_path: &Path,
        force: bool,
    ) -> Result<(), DomainError> {
        self.ensure_directory_exists().await?;
        let path = self.get_group_chat_path(chat_id);
        let backup_key = format!("group:{}", Self::strip_jsonl_extension(chat_id));
        self.write_payload_file_to_path(&path, source_path, force, chat_id, &backup_key)
            .await?;
        self.remove_summary_cache_for_path(&path).await;
        Ok(())
    }

    async fn delete_group_chat_payload(&self, chat_id: &str) -> Result<(), DomainError> {
        let path = self.get_group_chat_path(chat_id);
        if !path.exists() {
            return Err(DomainError::NotFound(format!(
                "Group chat not found: {}",
                chat_id
            )));
        }

        fs::remove_file(&path).await.map_err(|e| {
            DomainError::InternalError(format!("Failed to delete group chat file: {}", e))
        })?;
        self.remove_summary_cache_for_path(&path).await;
        Ok(())
    }

    async fn rename_group_chat_payload(
        &self,
        old_file_name: &str,
        new_file_name: &str,
    ) -> Result<(), DomainError> {
        let old_path = self.get_group_chat_path(old_file_name);
        if !old_path.exists() {
            return Err(DomainError::NotFound(format!(
                "Group chat not found: {}",
                old_file_name
            )));
        }

        let new_path = self.get_group_chat_path(new_file_name);
        if new_path.exists() {
            return Err(DomainError::InvalidData(format!(
                "Group chat already exists: {}",
                new_file_name
            )));
        }

        fs::copy(&old_path, &new_path).await.map_err(|e| {
            DomainError::InternalError(format!("Failed to copy group chat file: {}", e))
        })?;

        fs::remove_file(&old_path).await.map_err(|e| {
            DomainError::InternalError(format!("Failed to remove old group chat file: {}", e))
        })?;
        self.remove_summary_cache_for_path(&old_path).await;
        self.remove_summary_cache_for_path(&new_path).await;

        Ok(())
    }

    async fn import_group_chat_payload(&self, file_path: &Path) -> Result<String, DomainError> {
        self.ensure_directory_exists().await?;

        let chat_id = self.next_group_chat_id();
        let target_path = self.get_group_chat_path(&chat_id);

        fs::copy(file_path, &target_path).await.map_err(|e| {
            DomainError::InternalError(format!("Failed to import group chat file: {}", e))
        })?;
        self.remove_summary_cache_for_path(&target_path).await;

        Ok(chat_id)
    }

    async fn get_group_chat_summary(
        &self,
        chat_id: &str,
        include_metadata: bool,
    ) -> Result<ChatSearchResult, DomainError> {
        self.get_group_chat_summary_internal(chat_id, include_metadata)
            .await
    }

    async fn get_group_chat_metadata(&self, chat_id: &str) -> Result<Value, DomainError> {
        let path = self.get_group_chat_path(chat_id);
        self.read_chat_metadata_from_path(&path).await
    }

    async fn set_group_chat_metadata_extension(
        &self,
        chat_id: &str,
        namespace: &str,
        value: Value,
    ) -> Result<(), DomainError> {
        let path = self.get_group_chat_path(chat_id);
        self.set_chat_metadata_extension_in_path(&path, namespace, value)
            .await
    }

    async fn get_group_chat_store_json(
        &self,
        chat_id: &str,
        namespace: &str,
        key: &str,
    ) -> Result<Value, DomainError> {
        self.get_group_chat_store_json_value(chat_id, namespace, key)
            .await
    }

    async fn set_group_chat_store_json(
        &self,
        chat_id: &str,
        namespace: &str,
        key: &str,
        value: Value,
    ) -> Result<(), DomainError> {
        self.set_group_chat_store_json_value(chat_id, namespace, key, value)
            .await
    }

    async fn update_group_chat_store_json(
        &self,
        chat_id: &str,
        namespace: &str,
        key: &str,
        value: Value,
    ) -> Result<(), DomainError> {
        self.update_group_chat_store_json_value(chat_id, namespace, key, value)
            .await
    }

    async fn rename_group_chat_store_key(
        &self,
        chat_id: &str,
        namespace: &str,
        key: &str,
        new_key: &str,
    ) -> Result<(), DomainError> {
        self.rename_group_chat_store_key_value(chat_id, namespace, key, new_key)
            .await
    }

    async fn delete_group_chat_store_json(
        &self,
        chat_id: &str,
        namespace: &str,
        key: &str,
    ) -> Result<(), DomainError> {
        self.delete_group_chat_store_json_value(chat_id, namespace, key)
            .await
    }

    async fn list_group_chat_store_keys(
        &self,
        chat_id: &str,
        namespace: &str,
    ) -> Result<Vec<String>, DomainError> {
        self.list_group_chat_store_keys_for_namespace(chat_id, namespace)
            .await
    }

    async fn find_last_group_chat_message(
        &self,
        chat_id: &str,
        query: FindLastMessageQuery,
    ) -> Result<Option<LocatedChatMessage>, DomainError> {
        self.find_last_group_chat_message_internal(chat_id, query)
            .await
    }

    async fn search_group_chat_messages(
        &self,
        chat_id: &str,
        query: ChatMessageSearchQuery,
    ) -> Result<Vec<ChatMessageSearchHit>, DomainError> {
        self.search_group_chat_messages_internal(chat_id, query)
            .await
    }

    async fn clear_cache(&self) -> Result<(), DomainError> {
        <Self as ChatRepository>::clear_cache(self).await
    }
}

impl FileChatRepository {
    fn group_search_cache_key(query: &str, chat_ids: Option<&[String]>) -> String {
        let filter_key = if let Some(chat_ids) = chat_ids {
            let mut normalized_ids: Vec<String> = chat_ids
                .iter()
                .map(|id| Self::strip_jsonl_extension(id).to_string())
                .collect();
            normalized_ids.sort();
            normalized_ids.dedup();
            normalized_ids.join(",")
        } else {
            "*".to_string()
        };
        format!("group|{}|{}", filter_key, query)
    }
}
