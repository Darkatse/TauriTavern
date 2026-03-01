use std::collections::HashSet;
use std::path::Path;

use async_trait::async_trait;
use serde_json::Value;
use tokio::fs;

use crate::domain::errors::DomainError;
use crate::domain::models::chat::{Chat, ChatMessage};
use crate::domain::repositories::chat_repository::{
    ChatExportFormat, ChatImportFormat, ChatPayloadChunk, ChatPayloadCursor, ChatPayloadTail,
    ChatRepository, ChatSearchResult, PinnedCharacterChat, PinnedGroupChat,
};
use crate::infrastructure::logging::logger;
use crate::infrastructure::persistence::chat_format_importers::{
    export_payload_to_plain_text, import_chat_payloads_from_json, import_chat_payloads_from_jsonl,
};
use crate::infrastructure::persistence::file_system::list_files_with_extension;
use crate::infrastructure::persistence::jsonl_utils::{
    parse_jsonl_bytes, read_jsonl_file, write_jsonl_file,
};

use super::FileChatRepository;
use super::summary::ChatFileDescriptor;

#[derive(Clone)]
struct RankedChatDescriptor {
    modified_millis: i64,
    descriptor: ChatFileDescriptor,
}

impl FileChatRepository {
    fn character_recent_pin_key(character_name: &str, file_name: &str) -> Option<String> {
        let normalized_character = character_name.trim();
        let normalized_file = file_name.trim();
        if normalized_character.is_empty() || normalized_file.is_empty() {
            return None;
        }

        Some(format!(
            "{}/{}",
            normalized_character,
            Self::normalize_jsonl_file_name(normalized_file)
        ))
    }

    fn group_recent_pin_key(chat_id: &str) -> Option<String> {
        let normalized_chat_id = chat_id.trim();
        if normalized_chat_id.is_empty() {
            return None;
        }

        let normalized_file = Self::normalize_jsonl_file_name(normalized_chat_id);
        Some(Self::strip_jsonl_extension(&normalized_file).to_string())
    }

    async fn select_recent_descriptors<F>(
        &self,
        descriptors: Vec<ChatFileDescriptor>,
        max_entries: usize,
        is_pinned: F,
    ) -> Result<Vec<ChatFileDescriptor>, DomainError>
    where
        F: Fn(&ChatFileDescriptor) -> bool,
    {
        let mut pinned = Vec::new();
        let mut non_pinned = Vec::new();

        for descriptor in descriptors {
            let metadata = fs::metadata(&descriptor.path).await.map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to read chat metadata {:?}: {}",
                    descriptor.path, error
                ))
            })?;
            let modified_millis = Self::file_signature_from_metadata(&metadata).modified_millis;

            let ranked = RankedChatDescriptor {
                modified_millis,
                descriptor,
            };

            if is_pinned(&ranked.descriptor) {
                pinned.push(ranked);
            } else {
                non_pinned.push(ranked);
            }
        }

        pinned.sort_by(|a, b| b.modified_millis.cmp(&a.modified_millis));
        non_pinned.sort_by(|a, b| b.modified_millis.cmp(&a.modified_millis));

        let non_pinned_limit = max_entries.saturating_sub(pinned.len());
        let mut selected: Vec<ChatFileDescriptor> =
            pinned.into_iter().map(|entry| entry.descriptor).collect();
        selected.extend(
            non_pinned
                .into_iter()
                .take(non_pinned_limit)
                .map(|entry| entry.descriptor),
        );

        Ok(selected)
    }
}

#[async_trait]
impl ChatRepository for FileChatRepository {
    async fn save(&self, chat: &Chat) -> Result<(), DomainError> {
        self.save_with_options(chat, false).await
    }

    async fn save_with_options(&self, chat: &Chat, force: bool) -> Result<(), DomainError> {
        self.ensure_directory_exists().await?;
        self.write_chat_file(chat, force).await?;
        if let Some(file_name) = &chat.file_name {
            let path = self.get_chat_path(&chat.character_name, file_name);
            self.remove_summary_cache_for_path(&path).await;
        }
        Ok(())
    }

    async fn get_chat(&self, character_name: &str, file_name: &str) -> Result<Chat, DomainError> {
        // Try to get from cache first
        let cache_key = self.get_cache_key(character_name, file_name);

        {
            let cache = self.memory_cache.lock().await;
            if let Some(chat) = cache.get(&cache_key) {
                return Ok(chat);
            }
        }

        // If not in cache, read from file
        let chat = self.read_chat_file(character_name, file_name).await?;

        // Update cache
        {
            let mut cache = self.memory_cache.lock().await;
            cache.set(cache_key, chat.clone());
        }

        Ok(chat)
    }

    async fn get_character_chats(&self, character_name: &str) -> Result<Vec<Chat>, DomainError> {
        logger::debug(&format!("Getting chats for character: {}", character_name));

        // Ensure the character directory exists
        let character_dir = self.get_character_dir(character_name);
        if !character_dir.exists() {
            return Ok(Vec::new());
        }

        // List all JSONL files in the character directory
        let chat_files = list_files_with_extension(&character_dir, "jsonl").await?;
        let mut chats = Vec::new();

        for file_path in chat_files {
            let file_name = file_path
                .file_name()
                .and_then(|f| f.to_str())
                .unwrap_or("")
                .to_string();

            let mut chat = self.get_chat(character_name, &file_name).await?;
            // Keep the internal character ID stable for list/read-model flows.
            // Chat metadata may contain a mutable display name, but filesystem
            // layout and routing logic are keyed by directory (character_name).
            chat.character_name = character_name.to_string();
            chats.push(chat);
        }

        // Sort chats by last message date (newest first)
        chats.sort_by(|a, b| {
            b.get_last_message_timestamp()
                .cmp(&a.get_last_message_timestamp())
        });

        Ok(chats)
    }

    async fn get_all_chats(&self) -> Result<Vec<Chat>, DomainError> {
        logger::debug("Getting all chats");

        // Ensure the chats directory exists
        self.ensure_directory_exists().await?;

        // List all directories in the chats directory
        let mut entries = fs::read_dir(&self.chats_dir).await.map_err(|e| {
            logger::error(&format!("Failed to read chats directory: {}", e));
            DomainError::InternalError(format!("Failed to read chats directory: {}", e))
        })?;

        let mut all_chats = Vec::new();

        while let Some(entry) = entries.next_entry().await.map_err(|e| {
            logger::error(&format!("Failed to read directory entry: {}", e));
            DomainError::InternalError(format!("Failed to read directory entry: {}", e))
        })? {
            let path = entry.path();

            if path.is_dir() {
                let character_name = path
                    .file_name()
                    .and_then(|f| f.to_str())
                    .unwrap_or("")
                    .to_string();

                let chats = self.get_character_chats(&character_name).await?;
                all_chats.extend(chats);
            } else if path
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext.eq_ignore_ascii_case("jsonl"))
                .unwrap_or(false)
            {
                let file_name = path
                    .file_name()
                    .and_then(|f| f.to_str())
                    .unwrap_or("")
                    .to_string();

                let payload = read_jsonl_file(&path).await?;
                let chat = self.parse_chat_from_payload("", &file_name, &payload)?;
                all_chats.push(chat);
            }
        }

        // Sort chats by last message date (newest first)
        all_chats.sort_by(|a, b| {
            b.get_last_message_timestamp()
                .cmp(&a.get_last_message_timestamp())
        });

        Ok(all_chats)
    }

    async fn delete_chat(&self, character_name: &str, file_name: &str) -> Result<(), DomainError> {
        logger::debug(&format!("Deleting chat: {}/{}", character_name, file_name));

        let path = self.get_chat_path(character_name, file_name);

        if !path.exists() {
            return Err(DomainError::NotFound(format!(
                "Chat not found: {}/{}",
                character_name, file_name
            )));
        }

        // Delete the file
        fs::remove_file(&path).await.map_err(|e| {
            logger::error(&format!("Failed to delete chat file: {}", e));
            DomainError::InternalError(format!("Failed to delete chat file: {}", e))
        })?;

        // Remove from cache
        let cache_key = self.get_cache_key(character_name, file_name);
        {
            let mut cache = self.memory_cache.lock().await;
            cache.remove(&cache_key);
        }
        self.remove_summary_cache_for_path(&path).await;

        Ok(())
    }

    async fn rename_chat(
        &self,
        character_name: &str,
        old_file_name: &str,
        new_file_name: &str,
    ) -> Result<(), DomainError> {
        logger::debug(&format!(
            "Renaming chat: {}/{} -> {}/{}",
            character_name, old_file_name, character_name, new_file_name
        ));

        let old_path = self.get_chat_path(character_name, old_file_name);
        if !old_path.exists() {
            return Err(DomainError::NotFound(format!(
                "Chat not found: {}/{}",
                character_name, old_file_name
            )));
        }

        let new_path = self.get_chat_path(character_name, new_file_name);
        if new_path.exists() {
            return Err(DomainError::InvalidData(format!(
                "Chat already exists: {}/{}",
                character_name, new_file_name
            )));
        }

        // Keep payload byte-for-byte intact to avoid field drift.
        fs::copy(&old_path, &new_path).await.map_err(|e| {
            logger::error(&format!("Failed to copy chat file: {}", e));
            DomainError::InternalError(format!("Failed to copy chat file: {}", e))
        })?;

        fs::remove_file(&old_path).await.map_err(|e| {
            logger::error(&format!("Failed to delete old chat file: {}", e));
            DomainError::InternalError(format!("Failed to delete old chat file: {}", e))
        })?;

        // Update cache
        let old_cache_key = self.get_cache_key(character_name, old_file_name);
        let new_cache_key = self.get_cache_key(character_name, new_file_name);

        {
            let mut cache = self.memory_cache.lock().await;
            if let Some(mut chat) = cache.get(&old_cache_key) {
                chat.file_name = Some(Self::strip_jsonl_extension(new_file_name).to_string());
                cache.remove(&old_cache_key);
                cache.set(new_cache_key, chat);
            } else {
                cache.remove(&old_cache_key);
            }
        }
        self.remove_summary_cache_for_path(&old_path).await;
        self.remove_summary_cache_for_path(&new_path).await;

        Ok(())
    }

    async fn add_message(
        &self,
        character_name: &str,
        file_name: &str,
        message: ChatMessage,
    ) -> Result<Chat, DomainError> {
        logger::debug(&format!(
            "Adding message to chat: {}/{}",
            character_name, file_name
        ));

        // Get the chat
        let mut chat = self.get_chat(character_name, file_name).await?;

        // Add the message
        chat.add_message(message);

        // Save the chat
        self.save(&chat).await?;

        Ok(chat)
    }

    async fn search_chats(
        &self,
        query: &str,
        character_filter: Option<&str>,
    ) -> Result<Vec<ChatSearchResult>, DomainError> {
        logger::debug("Searching character chats with streaming scanner");

        let normalized_query = Self::normalize_search_query(query);
        let search_cache_key =
            Self::character_search_cache_key(&normalized_query, character_filter);
        if let Some(cached) = self.get_cached_search_results(&search_cache_key).await {
            return Ok(cached);
        }

        let descriptors = self.list_character_chat_files(character_filter).await?;
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

    async fn list_chat_summaries(
        &self,
        character_filter: Option<&str>,
        include_metadata: bool,
    ) -> Result<Vec<ChatSearchResult>, DomainError> {
        let descriptors = self.list_character_chat_files(character_filter).await?;
        let mut results = Vec::with_capacity(descriptors.len());
        for descriptor in descriptors {
            results.push(self.get_chat_summary(&descriptor, include_metadata).await?);
        }
        results.sort_by(|a, b| b.date.cmp(&a.date));
        self.flush_summary_index_if_needed().await?;
        Ok(results)
    }

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

    async fn list_recent_chat_summaries(
        &self,
        character_filter: Option<&str>,
        include_metadata: bool,
        max_entries: usize,
        pinned: &[PinnedCharacterChat],
    ) -> Result<Vec<ChatSearchResult>, DomainError> {
        let descriptors = self.list_character_chat_files(character_filter).await?;
        let pinned_keys: HashSet<String> = pinned
            .iter()
            .filter_map(|entry| {
                Self::character_recent_pin_key(&entry.character_name, &entry.file_name)
            })
            .collect();

        let selected = self
            .select_recent_descriptors(descriptors, max_entries, |descriptor| {
                Self::character_recent_pin_key(&descriptor.character_name, &descriptor.file_name)
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

    async fn import_chat(
        &self,
        character_name: &str,
        file_path: &Path,
        format: ChatImportFormat,
    ) -> Result<Chat, DomainError> {
        logger::debug(&format!(
            "Importing chat for character {} from {:?}",
            character_name, file_path
        ));

        let import_type = match format {
            ChatImportFormat::SillyTavern => "jsonl",
            _ => "json",
        };

        let imported_files = self
            .import_chat_payload(
                character_name,
                character_name,
                "User",
                file_path,
                import_type,
            )
            .await?;

        let first = imported_files.first().ok_or_else(|| {
            DomainError::InvalidData("No chat was imported from the provided file".to_string())
        })?;

        self.get_chat(character_name, Self::strip_jsonl_extension(first))
            .await
    }

    async fn export_chat(
        &self,
        character_name: &str,
        file_name: &str,
        target_path: &Path,
        format: ChatExportFormat,
    ) -> Result<(), DomainError> {
        logger::debug(&format!(
            "Exporting chat: {}/{} to {:?}",
            character_name, file_name, target_path
        ));

        match format {
            ChatExportFormat::JSONL => {
                let candidate_path = self.get_chat_path(character_name, file_name);
                let chat_path = if candidate_path.exists() {
                    candidate_path
                } else {
                    self.chats_dir
                        .join(Self::normalize_jsonl_file_name(file_name))
                };

                // Copy the file
                fs::copy(&chat_path, target_path).await.map_err(|e| {
                    logger::error(&format!("Failed to export chat: {}", e));
                    DomainError::InternalError(format!("Failed to export chat: {}", e))
                })?;
            }
            ChatExportFormat::PlainText => {
                let payload = self.get_chat_payload(character_name, file_name).await?;
                let text = export_payload_to_plain_text(&payload);

                // Write the file
                fs::write(target_path, text).await.map_err(|e| {
                    logger::error(&format!("Failed to write export file: {}", e));
                    DomainError::InternalError(format!("Failed to write export file: {}", e))
                })?;
            }
        }

        Ok(())
    }

    async fn backup_chat(&self, character_name: &str, file_name: &str) -> Result<(), DomainError> {
        let chat_path = self.get_chat_path(character_name, file_name);
        if !chat_path.exists() {
            return Err(DomainError::NotFound(format!(
                "Chat not found: {}/{}",
                character_name, file_name
            )));
        }

        let backup_key = self.get_cache_key(character_name, file_name);
        self.backup_chat_file(&chat_path, character_name, &backup_key)
            .await
    }

    async fn list_chat_backups(&self) -> Result<Vec<ChatSearchResult>, DomainError> {
        let descriptors = self.list_chat_backup_files().await?;
        let mut results = Vec::with_capacity(descriptors.len());

        for descriptor in descriptors {
            match self.get_chat_summary(&descriptor, false).await {
                Ok(summary) => results.push(summary),
                Err(error) => {
                    logger::warn(&format!(
                        "Failed to read chat backup summary {:?}: {}",
                        descriptor.path, error
                    ));
                }
            }
        }

        results.sort_by(|a, b| b.date.cmp(&a.date));
        self.flush_summary_index_if_needed().await?;
        Ok(results)
    }

    async fn get_chat_backup_bytes(&self, backup_file_name: &str) -> Result<Vec<u8>, DomainError> {
        self.ensure_directory_exists().await?;

        let path = self.resolve_existing_backup_path(backup_file_name)?;
        if !path.exists() {
            return Err(DomainError::NotFound(format!(
                "Chat backup not found: {}",
                backup_file_name
            )));
        }

        self.read_payload_bytes_from_path(&path).await
    }

    async fn delete_chat_backup(&self, backup_file_name: &str) -> Result<(), DomainError> {
        self.ensure_directory_exists().await?;

        let path = self.resolve_existing_backup_path(backup_file_name)?;
        if !path.exists() {
            return Err(DomainError::NotFound(format!(
                "Chat backup not found: {}",
                backup_file_name
            )));
        }

        fs::remove_file(&path).await.map_err(|error| {
            DomainError::InternalError(format!("Failed to delete chat backup file: {}", error))
        })?;

        self.remove_summary_cache_for_path(&path).await;
        self.flush_summary_index_if_needed().await?;
        Ok(())
    }

    async fn get_chat_payload(
        &self,
        character_name: &str,
        file_name: &str,
    ) -> Result<Vec<Value>, DomainError> {
        let bytes = self
            .get_chat_payload_bytes(character_name, file_name)
            .await?;
        parse_jsonl_bytes(&bytes)
    }

    async fn get_chat_payload_bytes(
        &self,
        character_name: &str,
        file_name: &str,
    ) -> Result<Vec<u8>, DomainError> {
        let path = self.get_chat_path(character_name, file_name);
        if !path.exists() {
            return Err(DomainError::NotFound(format!(
                "Chat not found: {}/{}",
                character_name, file_name
            )));
        }

        self.read_payload_bytes_from_path(&path).await
    }

    async fn get_chat_payload_path(
        &self,
        character_name: &str,
        file_name: &str,
    ) -> Result<std::path::PathBuf, DomainError> {
        let path = self.get_chat_path(character_name, file_name);
        if !path.exists() {
            return Err(DomainError::NotFound(format!(
                "Chat not found: {}/{}",
                character_name, file_name
            )));
        }

        Ok(path)
    }

    async fn get_chat_payload_tail_lines(
        &self,
        character_name: &str,
        file_name: &str,
        max_lines: usize,
    ) -> Result<ChatPayloadTail, DomainError> {
        self.get_character_payload_tail_lines(character_name, file_name, max_lines)
            .await
    }

    async fn get_chat_payload_before_lines(
        &self,
        character_name: &str,
        file_name: &str,
        cursor: ChatPayloadCursor,
        max_lines: usize,
    ) -> Result<ChatPayloadChunk, DomainError> {
        self.get_character_payload_before_lines(character_name, file_name, cursor, max_lines)
            .await
    }

    async fn save_chat_payload_windowed(
        &self,
        character_name: &str,
        file_name: &str,
        cursor: ChatPayloadCursor,
        header: String,
        lines: Vec<String>,
        force: bool,
    ) -> Result<ChatPayloadCursor, DomainError> {
        self.save_character_payload_windowed(
            character_name,
            file_name,
            cursor,
            header,
            lines,
            force,
        )
        .await
    }

    async fn save_chat_payload_from_path(
        &self,
        character_name: &str,
        file_name: &str,
        source_path: &Path,
        force: bool,
    ) -> Result<(), DomainError> {
        self.ensure_directory_exists().await?;

        let character_dir = self.get_character_dir(character_name);
        if !character_dir.exists() {
            fs::create_dir_all(&character_dir).await.map_err(|e| {
                DomainError::InternalError(format!(
                    "Failed to create character chat directory: {}",
                    e
                ))
            })?;
        }

        let path = self.get_chat_path(character_name, file_name);
        let backup_key = self.get_cache_key(character_name, file_name);
        self.write_payload_file_to_path(&path, source_path, force, character_name, &backup_key)
            .await?;

        {
            let mut cache = self.memory_cache.lock().await;
            cache.remove(&backup_key);
        }
        self.remove_summary_cache_for_path(&path).await;

        Ok(())
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

    async fn import_chat_payload(
        &self,
        character_name: &str,
        character_display_name: &str,
        user_name: &str,
        file_path: &Path,
        format: &str,
    ) -> Result<Vec<String>, DomainError> {
        self.ensure_directory_exists().await?;

        let file_text = fs::read_to_string(file_path).await.map_err(|e| {
            DomainError::InternalError(format!("Failed to read chat import file: {}", e))
        })?;

        let normalized_format = format.trim().to_lowercase();
        let payloads = match normalized_format.as_str() {
            "jsonl" => vec![import_chat_payloads_from_jsonl(
                &file_text,
                user_name,
                character_display_name,
            )?],
            "json" => {
                let value: Value = serde_json::from_str(&file_text).map_err(|e| {
                    DomainError::InvalidData(format!("Failed to parse chat import JSON: {}", e))
                })?;
                import_chat_payloads_from_json(&value, user_name, character_display_name)?
            }
            other => {
                return Err(DomainError::InvalidData(format!(
                    "Unsupported chat import format: {}",
                    other
                )));
            }
        };

        let character_dir = self.get_character_dir(character_name);
        if !character_dir.exists() {
            fs::create_dir_all(&character_dir).await.map_err(|e| {
                DomainError::InternalError(format!(
                    "Failed to create character chat directory: {}",
                    e
                ))
            })?;
        }

        let mut created_files = Vec::with_capacity(payloads.len());
        for (index, payload) in payloads.iter().enumerate() {
            let file_stem =
                self.next_import_chat_file_stem(character_name, character_display_name, index);
            let path = self.get_chat_path(character_name, &file_stem);
            write_jsonl_file(&path, payload).await?;
            self.remove_summary_cache_for_path(&path).await;
            created_files.push(Self::normalize_jsonl_file_name(&file_stem));
        }

        Ok(created_files)
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

    async fn clear_cache(&self) -> Result<(), DomainError> {
        {
            let mut cache = self.memory_cache.lock().await;
            cache.clear();
        }
        self.clear_summary_cache().await;
        Ok(())
    }
}

impl FileChatRepository {
    fn character_search_cache_key(query: &str, character_filter: Option<&str>) -> String {
        let character_key = character_filter.unwrap_or("*");
        format!("character|{}|{}", character_key, query)
    }

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
