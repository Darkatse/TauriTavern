use std::path::Path;
use std::sync::Arc;

use tokio::fs::{self, File};
use tokio::sync::{Mutex, Semaphore};
use tokio::task::JoinSet;
use tt_domain::errors::DomainError;
use tt_domain::models::chat::parse_message_timestamp_value;

use crate::chat_directory_identity::{self, SharedChatAliasStore};
use crate::file_system::list_files_with_extension;

use super::super::FileChatRepository;
use super::index::ChatStatsCacheEntry;
use super::projection;
use super::{ChatFileDescriptor, SummaryCache, summary_cache_key};

const MAX_CONCURRENT_READS: usize = 8;

impl FileChatRepository {
    pub async fn calculate_character_chat_stats(
        &self,
        character_name: &str,
    ) -> Result<(u64, i64), DomainError> {
        let mut results = self
            .calculate_character_chat_stats_batch(vec![character_name.to_string()])
            .await?;
        results
            .pop()
            .ok_or_else(|| {
                DomainError::InternalError(
                    "Character chat stats batch returned no result".to_string(),
                )
            })?
            .1
    }

    pub async fn calculate_character_chat_stats_batch(
        &self,
        character_names: Vec<String>,
    ) -> Result<Vec<(String, Result<(u64, i64), DomainError>)>, DomainError> {
        let mut results = Vec::with_capacity(character_names.len());
        let semaphore = Arc::new(Semaphore::new(Self::chat_stats_parallelism()));
        let mut jobs = JoinSet::new();

        for character_name in character_names {
            let permit = semaphore.clone().acquire_owned().await.map_err(|_| {
                DomainError::InternalError("Character chat stats scanner gate closed".to_string())
            })?;
            let characters_dir = self.characters_dir.clone();
            let chats_dir = self.chats_dir.clone();
            let chat_aliases = self.chat_aliases.clone();
            let summary_cache = self.summary_cache.clone();

            jobs.spawn(async move {
                let _permit = permit;
                let result = Self::calculate_character_chat_stats_from_parts(
                    &characters_dir,
                    &chats_dir,
                    &chat_aliases,
                    &summary_cache,
                    &character_name,
                )
                .await;
                (character_name, result)
            });
        }

        while let Some(joined) = jobs.join_next().await {
            results.push(joined.map_err(|error| {
                DomainError::InternalError(format!("Character chat stats scanner failed: {error}"))
            })?);
        }
        self.flush_summary_index_best_effort().await;
        Ok(results)
    }

    async fn calculate_character_chat_stats_from_parts(
        characters_dir: &Path,
        chats_dir: &Path,
        chat_aliases: &SharedChatAliasStore,
        summary_cache: &Arc<Mutex<SummaryCache>>,
        character_name: &str,
    ) -> Result<(u64, i64), DomainError> {
        let dir_key = chat_directory_identity::resolve_character_chat_dir_key(
            characters_dir,
            chats_dir,
            chat_aliases,
            character_name,
        )
        .await?;
        let files = list_files_with_extension(&chats_dir.join(dir_key), "jsonl").await?;
        let mut total_size = 0;
        let mut latest_date = 0;

        for path in files {
            let Some(file_name) = path
                .file_name()
                .and_then(|value| value.to_str())
                .map(ToString::to_string)
            else {
                continue;
            };
            let entry = Self::get_chat_stats_entry(
                summary_cache,
                &ChatFileDescriptor {
                    character_name: character_name.to_string(),
                    file_name,
                    path,
                },
            )
            .await?;
            total_size += entry.signature.size;
            latest_date = latest_date.max(entry.date);
        }
        Ok((total_size, latest_date))
    }

    pub(in crate::repositories::file_chat_repository) async fn get_chat_stats_date(
        summary_cache: &Arc<Mutex<SummaryCache>>,
        descriptor: &ChatFileDescriptor,
    ) -> Result<i64, DomainError> {
        Ok(Self::get_chat_stats_entry(summary_cache, descriptor)
            .await?
            .date)
    }

    async fn get_chat_stats_entry(
        summary_cache: &Arc<Mutex<SummaryCache>>,
        descriptor: &ChatFileDescriptor,
    ) -> Result<ChatStatsCacheEntry, DomainError> {
        let metadata = fs::metadata(&descriptor.path).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read chat metadata {:?}: {error}",
                descriptor.path
            ))
        })?;
        let signature = Self::file_signature_from_metadata(&metadata);
        let cache_key = summary_cache_key(&descriptor.path);

        {
            let mut cache = summary_cache.lock().await;
            cache.ensure_loaded()?;
            if let Some(entry) = cache.get_stats(&cache_key, signature) {
                return Ok(entry);
            }
        }

        let mut file = File::open(&descriptor.path).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to open chat file {:?}: {error}",
                descriptor.path
            ))
        })?;
        let signature =
            Self::file_signature_from_metadata(&file.metadata().await.map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to read chat metadata {:?}: {error}",
                    descriptor.path
                ))
            })?);
        {
            let mut cache = summary_cache.lock().await;
            cache.ensure_loaded()?;
            if let Some(entry) = cache.get_stats(&cache_key, signature) {
                return Ok(entry);
            }
        }

        let parsed_date =
            projection::read_last_raw_record(&mut file, &descriptor.path, signature.size)
                .await?
                .map(|record| parse_message_timestamp_value(record.send_date.as_ref()))
                .unwrap_or_default();
        let entry = ChatStatsCacheEntry {
            signature,
            date: if parsed_date > 0 {
                parsed_date
            } else {
                signature.modified_millis
            },
        };

        let mut cache = summary_cache.lock().await;
        cache.ensure_loaded()?;
        cache.set_stats(cache_key, entry.clone());
        Ok(entry)
    }

    pub(in crate::repositories::file_chat_repository) fn chat_stats_parallelism() -> usize {
        std::thread::available_parallelism()
            .map(|value| value.get())
            .unwrap_or(4)
            .clamp(1, MAX_CONCURRENT_READS)
    }
}
