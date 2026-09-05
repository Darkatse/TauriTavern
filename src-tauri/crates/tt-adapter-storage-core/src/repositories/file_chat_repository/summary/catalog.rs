use std::collections::HashSet;

use tokio::fs;
use tt_domain::errors::DomainError;
use tt_domain::models::chat::strip_jsonl_extension;
use tt_ports::repositories::chat_repository::ChatSearchResult;

use crate::file_system::list_files_with_extension;

use super::super::FileChatRepository;
use super::{ChatFileDescriptor, SummaryCacheEntry, summary_cache_key};

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

    pub(super) async fn get_chat_summary_entry(
        &self,
        descriptor: &ChatFileDescriptor,
        require_fingerprint: bool,
    ) -> Result<SummaryCacheEntry, DomainError> {
        self.summary_cache.lock().await.ensure_loaded()?;

        let metadata = fs::metadata(&descriptor.path).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read chat metadata {:?}: {error}",
                descriptor.path
            ))
        })?;
        let signature = Self::file_signature_from_metadata(&metadata);
        let cache_key = summary_cache_key(&descriptor.path);

        {
            let cache = self.summary_cache.lock().await;
            if let Some(entry) = cache.get(&cache_key)
                && entry.signature == signature
                && (!require_fingerprint || entry.fingerprint.is_some())
            {
                return Ok(entry.clone());
            }
        }

        let scanned = self
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
            .set(cache_key, scanned.clone());
        Ok(scanned)
    }

    pub(in crate::repositories::file_chat_repository) async fn get_chat_summary(
        &self,
        descriptor: &ChatFileDescriptor,
        include_metadata: bool,
    ) -> Result<ChatSearchResult, DomainError> {
        let mut summary = self
            .get_chat_summary_entry(descriptor, false)
            .await?
            .summary;
        if !include_metadata {
            summary.chat_metadata = None;
        }
        Ok(summary)
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
