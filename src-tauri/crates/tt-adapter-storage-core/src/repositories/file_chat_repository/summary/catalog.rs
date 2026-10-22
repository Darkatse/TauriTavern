use std::cmp::Reverse;
use std::collections::HashSet;

use tokio::fs;
use tt_domain::errors::DomainError;
use tt_domain::models::chat::strip_jsonl_extension;
use tt_ports::repositories::chat_repository::ChatSearchResult;

use crate::file_system::list_files_with_extension;

use super::super::FileChatRepository;
use super::{
    ChatFileDescriptor, FileSignature, SummaryCacheEntry, message_display_text, projection,
    summary_cache_key,
};

impl FileChatRepository {
    async fn list_character_chat_directory_keys(&self) -> Result<Vec<String>, DomainError> {
        if !self.characters_dir.exists() {
            return Ok(Vec::new());
        }

        let mut entries = fs::read_dir(&self.characters_dir).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read characters directory {:?}: {error}",
                self.characters_dir
            ))
        })?;
        let mut keys = HashSet::new();

        while let Some(entry) = entries.next_entry().await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read characters directory entry {:?}: {error}",
                self.characters_dir
            ))
        })? {
            let path = entry.path();
            if !path.is_file()
                || !path
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("png"))
            {
                continue;
            }
            if let Some(stem) = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .filter(|stem| !stem.is_empty())
            {
                keys.insert(stem.to_string());
            }
        }

        let mut keys: Vec<_> = keys.into_iter().collect();
        keys.sort();
        Ok(keys)
    }

    pub(in crate::repositories::file_chat_repository) async fn list_character_chat_files(
        &self,
        character_filter: Option<&str>,
    ) -> Result<Vec<ChatFileDescriptor>, DomainError> {
        self.ensure_directory_exists().await?;

        if let Some(character_name) = character_filter {
            let dir = self.resolve_character_chat_dir(character_name).await?;
            return Ok(list_files_with_extension(&dir, "jsonl")
                .await?
                .into_iter()
                .filter_map(|path| {
                    Some(ChatFileDescriptor {
                        character_name: character_name.to_string(),
                        file_name: path.file_name()?.to_str()?.to_string(),
                        path,
                    })
                })
                .collect());
        }

        let mut descriptors = Vec::new();
        for character_name in self.list_character_chat_directory_keys().await? {
            let dir = self.resolve_character_chat_dir(&character_name).await?;
            descriptors.extend(
                list_files_with_extension(&dir, "jsonl")
                    .await?
                    .into_iter()
                    .filter_map(|path| {
                        Some(ChatFileDescriptor {
                            character_name: character_name.clone(),
                            file_name: path.file_name()?.to_str()?.to_string(),
                            path,
                        })
                    }),
            );
        }
        descriptors.extend(
            list_files_with_extension(&self.chats_dir, "jsonl")
                .await?
                .into_iter()
                .filter_map(|path| {
                    Some(ChatFileDescriptor {
                        character_name: String::new(),
                        file_name: path.file_name()?.to_str()?.to_string(),
                        path,
                    })
                }),
        );
        Ok(descriptors)
    }

    pub(in crate::repositories::file_chat_repository) async fn list_group_chat_files(
        &self,
        chat_ids: Option<&[String]>,
    ) -> Result<Vec<ChatFileDescriptor>, DomainError> {
        self.ensure_directory_exists().await?;

        if let Some(chat_ids) = chat_ids {
            let mut descriptors = Vec::new();
            for id in chat_ids
                .iter()
                .map(|id| strip_jsonl_extension(id).to_string())
                .collect::<HashSet<_>>()
            {
                let path = self.get_group_chat_path(&id)?;
                if path.exists() {
                    descriptors.push(ChatFileDescriptor {
                        character_name: String::new(),
                        file_name: Self::normalize_jsonl_file_name(&id)?,
                        path,
                    });
                }
            }
            return Ok(descriptors);
        }

        Ok(list_files_with_extension(&self.group_chats_dir, "jsonl")
            .await?
            .into_iter()
            .filter_map(|path| {
                Some(ChatFileDescriptor {
                    character_name: String::new(),
                    file_name: path.file_name()?.to_str()?.to_string(),
                    path,
                })
            })
            .collect())
    }

    async fn lookup_chat_summary(
        &self,
        descriptor: &ChatFileDescriptor,
        require_fingerprint: bool,
    ) -> Result<(FileSignature, Option<SummaryCacheEntry>), DomainError> {
        let metadata = fs::metadata(&descriptor.path).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read chat metadata {:?}: {error}",
                descriptor.path
            ))
        })?;
        let signature = Self::file_signature_from_metadata(&metadata);
        let cache_key = summary_cache_key(&descriptor.path);

        let mut cache = self.summary_cache.lock().await;
        cache.ensure_loaded().await;
        let entry = cache
            .get(&cache_key)
            .filter(|entry| entry.signature == signature)
            .filter(|entry| !require_fingerprint || entry.fingerprint.is_some())
            .cloned();
        Ok((signature, entry))
    }

    pub(super) async fn get_chat_summary_entry(
        &self,
        descriptor: &ChatFileDescriptor,
        require_fingerprint: bool,
    ) -> Result<SummaryCacheEntry, DomainError> {
        let (signature, cached) = self
            .lookup_chat_summary(descriptor, require_fingerprint)
            .await?;
        if let Some(entry) = cached {
            return Ok(entry);
        }

        let (scanned, _) = self
            .scan_chat_summary_file(
                &descriptor.path,
                &descriptor.character_name,
                &descriptor.file_name,
                signature,
                require_fingerprint,
            )
            .await?;
        self.summary_cache
            .lock()
            .await
            .set(summary_cache_key(&descriptor.path), scanned.clone());
        Ok(scanned)
    }

    /// Character-list summaries plus untruncated last-message display text.
    /// Missing text remains `None` so the consumer can choose its empty label.
    /// Full text is returned from the cold scan or read only from the tail on a cache hit.
    pub async fn list_chat_summaries_with_full_text(
        &self,
        character_name: &str,
    ) -> Result<Vec<(ChatSearchResult, Option<String>)>, DomainError> {
        let descriptors = self.list_character_chat_files(Some(character_name)).await?;
        let mut results = Vec::with_capacity(descriptors.len());
        for descriptor in descriptors {
            match self.get_chat_summary_with_full_text(&descriptor).await {
                Ok(result) => results.push(result),
                Err(error) => tracing::error!(
                    target: tt_contracts::observability::USER_VISIBLE_ERROR,
                    "Failed to inspect chat '{}': {}",
                    descriptor.path.display(),
                    error
                ),
            }
        }
        results.sort_by_key(|(summary, _)| Reverse(summary.date));
        self.flush_summary_index_best_effort().await;
        Ok(results)
    }

    async fn get_chat_summary_with_full_text(
        &self,
        descriptor: &ChatFileDescriptor,
    ) -> Result<(ChatSearchResult, Option<String>), DomainError> {
        let (signature, cached) = self.lookup_chat_summary(descriptor, false).await?;
        let (entry, text) = if let Some(entry) = cached {
            let text = if entry.preview_unavailable {
                message_display_text(projection::MessageText::Unavailable)
            } else if entry.summary.message_count == 0 {
                None
            } else {
                message_display_text(projection::read_last_raw_tail(&descriptor.path).await?.mes)
            };
            (entry, text)
        } else {
            let (entry, text) = self
                .scan_chat_summary_file(
                    &descriptor.path,
                    &descriptor.character_name,
                    &descriptor.file_name,
                    signature,
                    false,
                )
                .await?;
            self.summary_cache
                .lock()
                .await
                .set(summary_cache_key(&descriptor.path), entry.clone());
            (entry, text)
        };
        Ok((entry.summary.into(), text))
    }

    pub(in crate::repositories::file_chat_repository) async fn get_chat_summary(
        &self,
        descriptor: &ChatFileDescriptor,
        include_metadata: bool,
    ) -> Result<ChatSearchResult, DomainError> {
        let mut result = ChatSearchResult::from(
            self.get_chat_summary_entry(descriptor, false)
                .await?
                .summary,
        );
        if include_metadata {
            result.chat_metadata = self
                .read_optional_chat_metadata_from_path(&descriptor.path)
                .await?;
        }
        Ok(result)
    }

    pub(in crate::repositories::file_chat_repository) async fn collect_chat_summaries(
        &self,
        descriptors: Vec<ChatFileDescriptor>,
        include_metadata: bool,
    ) -> Vec<ChatSearchResult> {
        let mut results = Vec::with_capacity(descriptors.len());
        for descriptor in descriptors {
            match self.get_chat_summary(&descriptor, include_metadata).await {
                Ok(summary) => results.push(summary),
                Err(error) => tracing::error!(
                    target: tt_contracts::observability::USER_VISIBLE_ERROR,
                    "Failed to inspect chat '{}': {}",
                    descriptor.path.display(),
                    error
                ),
            }
        }
        results
    }

    pub(in crate::repositories::file_chat_repository) async fn get_character_chat_summary_internal(
        &self,
        character_name: &str,
        file_name: &str,
        include_metadata: bool,
    ) -> Result<ChatSearchResult, DomainError> {
        self.ensure_directory_exists().await?;
        let path = self
            .resolve_character_chat_path(character_name, file_name)
            .await?;
        if !path.exists() {
            return Err(DomainError::NotFound(format!(
                "Chat not found: {character_name}/{file_name}"
            )));
        }
        self.get_chat_summary(
            &ChatFileDescriptor {
                character_name: character_name.to_string(),
                file_name: Self::normalize_jsonl_file_name(file_name)?,
                path,
            },
            include_metadata,
        )
        .await
    }

    pub(in crate::repositories::file_chat_repository) async fn get_group_chat_summary_internal(
        &self,
        chat_id: &str,
        include_metadata: bool,
    ) -> Result<ChatSearchResult, DomainError> {
        self.ensure_directory_exists().await?;
        let path = self.get_group_chat_path(chat_id)?;
        if !path.exists() {
            return Err(DomainError::NotFound(format!(
                "Group chat not found: {chat_id}"
            )));
        }
        self.get_chat_summary(
            &ChatFileDescriptor {
                character_name: String::new(),
                file_name: Self::normalize_jsonl_file_name(chat_id)?,
                path,
            },
            include_metadata,
        )
        .await
    }
}
