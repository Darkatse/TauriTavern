use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use serde_json::Value;
use tokio::fs;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use crate::png_card_metadata::read_character_data_from_png_file;
use tt_adapter_storage_core::file_system::list_files_with_extension;
use tt_domain::errors::DomainError;
use tt_domain::models::character::Character;

use super::FileCharacterRepository;
use super::cache::{
    CharacterShallowIndexCache, CharacterShallowIndexCachedCharacter,
    CharacterShallowIndexEntrySignature, CharacterShallowIndexSignature,
};
use super::helpers::{file_ctime_millis, file_modified_millis};

const MAX_CONCURRENT_SHALLOW_READS: usize = 8;

#[derive(Debug)]
struct CharacterShallowIndexScanEntry {
    path: PathBuf,
    file_stem: String,
    signature: CharacterShallowIndexEntrySignature,
}

impl FileCharacterRepository {
    pub(crate) async fn clear_shallow_index_cache(&self) {
        let mut cache = self.shallow_index_cache.lock().await;
        *cache = None;
    }

    pub(crate) async fn load_shallow_character_index(&self) -> Result<Vec<Character>, DomainError> {
        self.ensure_directory_exists().await?;

        let (scan_entries, scan_complete) = self.scan_shallow_index_entries().await?;
        let signature = CharacterShallowIndexSignature {
            entries: scan_entries
                .iter()
                .map(|entry| entry.signature.clone())
                .collect(),
        };

        let cached = self.shallow_index_cache.lock().await.clone();
        if scan_complete
            && let Some(cache) = &cached
            && cache.signature == signature
        {
            return Ok(Self::shallow_index_characters(cache));
        }

        let previous_by_avatar = cached
            .as_ref()
            .map(Self::shallow_index_by_avatar)
            .unwrap_or_default();
        let (mut indexed_characters, build_complete) = self
            .build_shallow_index_characters(scan_entries, &previous_by_avatar)
            .await?;
        if (!scan_complete || !build_complete)
            && let Some(cache) = &cached
        {
            return Ok(Self::shallow_index_characters(cache));
        }

        let characters = indexed_characters
            .iter()
            .map(|entry| entry.character.clone())
            .collect();

        if scan_complete && build_complete {
            let mut cache = self.shallow_index_cache.lock().await;
            *cache = Some(CharacterShallowIndexCache {
                signature,
                characters: std::mem::take(&mut indexed_characters),
            });
        }

        Ok(characters)
    }

    fn shallow_index_characters(cache: &CharacterShallowIndexCache) -> Vec<Character> {
        cache
            .characters
            .iter()
            .map(|entry| entry.character.clone())
            .collect()
    }

    fn shallow_index_by_avatar(
        cache: &CharacterShallowIndexCache,
    ) -> HashMap<String, CharacterShallowIndexCachedCharacter> {
        cache
            .characters
            .iter()
            .cloned()
            .map(|entry| (entry.signature.avatar.clone(), entry))
            .collect()
    }

    async fn scan_shallow_index_entries(
        &self,
    ) -> Result<(Vec<CharacterShallowIndexScanEntry>, bool), DomainError> {
        let character_files = list_files_with_extension(&self.characters_dir, "png").await?;
        let mut entries = Vec::with_capacity(character_files.len());
        let mut complete = true;

        for path in character_files {
            match self.scan_shallow_index_entry(path).await {
                Ok(entry) => entries.push(entry),
                Err(error) => {
                    complete = false;
                    tracing::error!(
                        target: tt_contracts::observability::USER_VISIBLE_ERROR,
                        "Failed to inspect character for shallow index: {}",
                        error
                    );
                }
            }
        }
        entries.sort_by(|left, right| left.signature.avatar.cmp(&right.signature.avatar));

        Ok((entries, complete))
    }

    async fn scan_shallow_index_entry(
        &self,
        path: PathBuf,
    ) -> Result<CharacterShallowIndexScanEntry, DomainError> {
        let metadata = fs::metadata(&path).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read character metadata '{}': {}",
                path.display(),
                error
            ))
        })?;
        let file_stem = path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("")
            .to_string();
        let avatar = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("")
            .to_string();
        let (chat_size, date_last_chat) = self.calculate_chat_stats(&file_stem).await?;
        let modified_millis = file_modified_millis(&metadata);

        Ok(CharacterShallowIndexScanEntry {
            path,
            file_stem,
            signature: CharacterShallowIndexEntrySignature {
                avatar,
                file_size: metadata.len(),
                modified_millis,
                created_millis: file_ctime_millis(&metadata).unwrap_or(modified_millis),
                chat_size,
                date_last_chat,
            },
        })
    }

    async fn build_shallow_index_characters(
        &self,
        scan_entries: Vec<CharacterShallowIndexScanEntry>,
        previous_by_avatar: &HashMap<String, CharacterShallowIndexCachedCharacter>,
    ) -> Result<(Vec<CharacterShallowIndexCachedCharacter>, bool), DomainError> {
        let mut results = vec![None; scan_entries.len()];
        let mut complete = true;
        let semaphore = Arc::new(Semaphore::new(Self::shallow_index_parallelism()));
        let mut jobs = JoinSet::new();

        for (index, entry) in scan_entries.into_iter().enumerate() {
            if let Some(cached) = previous_by_avatar.get(&entry.signature.avatar)
                && cached.signature == entry.signature
            {
                results[index] = Some(cached.clone());
                continue;
            }

            let permit = semaphore.clone().acquire_owned().await.map_err(|_| {
                DomainError::InternalError("Shallow character index worker gate closed".to_string())
            })?;

            jobs.spawn(async move {
                let _permit = permit;
                let file_stem = entry.file_stem.clone();
                let signature = entry.signature.clone();
                let result = Self::read_shallow_character_from_entry(entry).await;
                (index, file_stem, signature, result)
            });
        }

        while let Some(joined) = jobs.join_next().await {
            let (index, file_stem, signature, result) = joined.map_err(|error| {
                DomainError::InternalError(format!(
                    "Shallow character index worker failed: {}",
                    error
                ))
            })?;

            match result {
                Ok(character) => {
                    results[index] = Some(CharacterShallowIndexCachedCharacter {
                        signature,
                        character,
                    });
                }
                Err(error) => {
                    complete = false;
                    tracing::error!(
                        target: tt_contracts::observability::USER_VISIBLE_ERROR,
                        "Failed to process character {}: {}",
                        file_stem,
                        error
                    );
                }
            }
        }

        Ok((results.into_iter().flatten().collect(), complete))
    }

    fn shallow_index_parallelism() -> usize {
        std::thread::available_parallelism()
            .map(|value| value.get())
            .unwrap_or(4)
            .clamp(1, MAX_CONCURRENT_SHALLOW_READS)
    }

    async fn read_shallow_character_from_entry(
        entry: CharacterShallowIndexScanEntry,
    ) -> Result<Character, DomainError> {
        let json_data = read_character_data_from_png_file(&entry.path).await?;
        let raw_value: Value = serde_json::from_str(&json_data).map_err(|error| {
            DomainError::InvalidData(format!("Failed to parse character data: {}", error))
        })?;
        let mut character: Character =
            serde_json::from_value(raw_value.clone()).map_err(|error| {
                DomainError::InvalidData(format!("Failed to decode character data: {}", error))
            })?;

        Self::sync_canonical_data_fields(&mut character, &raw_value);
        Self::normalize_imported_character(&mut character)?;
        let data_size = Self::calculate_data_size(&character.data);
        character.shallow = false;
        let signature = entry.signature;
        character.file_name = Some(entry.file_stem);
        character.avatar = signature.avatar;
        character.date_added = signature.created_millis;
        let create_date_fallback =
            (signature.created_millis > 0).then_some(signature.created_millis);
        if let Some(repaired_create_date) =
            Self::repaired_character_create_date(&character.create_date, create_date_fallback)
        {
            character.create_date = repaired_create_date;
        }
        character.chat_size = signature.chat_size;
        character.data_size = data_size;
        character.date_last_chat = signature.date_last_chat;

        Ok(character.into_shallow())
    }
}
