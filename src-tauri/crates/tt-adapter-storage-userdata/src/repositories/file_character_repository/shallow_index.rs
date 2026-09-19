use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::fs;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use crate::png_card_metadata::read_character_data_from_png_file;
use tt_adapter_storage_core::file_system::{
    list_files_with_extension, replace_file, unique_temp_path,
};
use tt_domain::errors::DomainError;
use tt_domain::models::character::Character;

use super::FileCharacterRepository;
use super::cache::{
    CharacterShallowIndexCache, CharacterShallowIndexCachedCharacter,
    CharacterShallowIndexEntrySignature, CharacterShallowIndexSignature,
};
use super::helpers::{file_ctime_millis, file_modified_millis};

const MAX_CONCURRENT_SHALLOW_READS: usize = 8;
const PERSISTENT_SHALLOW_INDEX_SCHEMA_VERSION: u32 = 2;

#[derive(Serialize, Deserialize)]
struct PersistentShallowIndexSnapshot {
    schema_version: u32,
    entries: Vec<PersistentShallowIndexEntry>,
}

#[derive(Serialize, Deserialize)]
struct PersistentShallowIndexEntry {
    signature: CharacterShallowIndexEntrySignature,
    file_name: String,
    data_size: u64,
    character: Character,
}

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
        drop(cache);

        if let Err(error) = fs::remove_file(&self.shallow_index_path).await
            && error.kind() != io::ErrorKind::NotFound
        {
            tracing::warn!(
                "Failed to remove character shallow index '{}': {}",
                self.shallow_index_path.display(),
                error
            );
        }
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

        let persistent_candidate = if scan_complete {
            self.load_persistent_shallow_index_candidate().await
        } else {
            None
        };
        if let Some(cache) = &persistent_candidate
            && cache.signature == signature
        {
            let characters = Self::shallow_index_characters(cache);
            let mut memory_cache = self.shallow_index_cache.lock().await;
            *memory_cache = persistent_candidate;
            return Ok(characters);
        }

        let previous_by_avatar = cached
            .as_ref()
            .or(persistent_candidate.as_ref())
            .map(Self::shallow_index_by_avatar)
            .unwrap_or_default();
        let (mut indexed_characters, build_complete) = self
            .build_shallow_index_characters(scan_entries, &previous_by_avatar)
            .await?;

        let characters = indexed_characters
            .iter()
            .map(|entry| entry.character.clone())
            .collect();

        if scan_complete && build_complete {
            let cache = CharacterShallowIndexCache {
                signature,
                characters: std::mem::take(&mut indexed_characters),
            };
            if let Err(error) = self.save_persistent_shallow_index(&cache).await {
                tracing::warn!("Failed to persist character shallow index: {}", error);
            }
            let mut memory_cache = self.shallow_index_cache.lock().await;
            *memory_cache = Some(cache);
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

    fn reuse_cached_shallow_character(
        cached: &CharacterShallowIndexCachedCharacter,
        signature: &CharacterShallowIndexEntrySignature,
    ) -> Option<CharacterShallowIndexCachedCharacter> {
        if cached.signature.file_size != signature.file_size
            || cached.signature.modified_millis != signature.modified_millis
            || cached.signature.created_millis != signature.created_millis
        {
            return None;
        }

        let mut reused = cached.clone();
        reused.signature = signature.clone();
        reused.character.chat_size = signature.chat_size;
        reused.character.date_last_chat = signature.date_last_chat;
        Some(reused)
    }

    async fn load_persistent_shallow_index_candidate(&self) -> Option<CharacterShallowIndexCache> {
        let bytes = match fs::read(&self.shallow_index_path).await {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return None,
            Err(error) => {
                tracing::warn!(
                    "Ignoring unreadable character shallow index '{}': {}",
                    self.shallow_index_path.display(),
                    error
                );
                return None;
            }
        };

        let snapshot = match serde_json::from_slice::<PersistentShallowIndexSnapshot>(&bytes) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                tracing::warn!(
                    "Ignoring invalid character shallow index '{}': {}",
                    self.shallow_index_path.display(),
                    error
                );
                return None;
            }
        };

        if snapshot.schema_version != PERSISTENT_SHALLOW_INDEX_SCHEMA_VERSION {
            tracing::warn!(
                "Ignoring character shallow index schema {} (expected {})",
                snapshot.schema_version,
                PERSISTENT_SHALLOW_INDEX_SCHEMA_VERSION
            );
            return None;
        }

        let signature = CharacterShallowIndexSignature {
            entries: snapshot
                .entries
                .iter()
                .map(|entry| entry.signature.clone())
                .collect(),
        };

        let characters = snapshot
            .entries
            .into_iter()
            .map(|entry| {
                let mut character = entry.character;
                character.file_name = Some(entry.file_name);
                character.avatar = entry.signature.avatar.clone();
                character.chat_size = entry.signature.chat_size;
                character.data_size = entry.data_size;
                character.date_added = entry.signature.created_millis;
                character.date_last_chat = entry.signature.date_last_chat;
                character.shallow = true;
                CharacterShallowIndexCachedCharacter {
                    signature: entry.signature,
                    character,
                }
            })
            .collect();

        Some(CharacterShallowIndexCache {
            signature,
            characters,
        })
    }

    async fn save_persistent_shallow_index(
        &self,
        cache: &CharacterShallowIndexCache,
    ) -> Result<(), DomainError> {
        let snapshot = PersistentShallowIndexSnapshot {
            schema_version: PERSISTENT_SHALLOW_INDEX_SCHEMA_VERSION,
            entries: cache
                .characters
                .iter()
                .map(|entry| {
                    let file_name = entry.character.file_name.clone().ok_or_else(|| {
                        DomainError::InternalError(format!(
                            "Character shallow index entry '{}' is missing file_name",
                            entry.signature.avatar
                        ))
                    })?;

                    Ok(PersistentShallowIndexEntry {
                        signature: entry.signature.clone(),
                        file_name,
                        data_size: entry.character.data_size,
                        character: entry.character.clone(),
                    })
                })
                .collect::<Result<Vec<_>, DomainError>>()?,
        };
        let bytes = serde_json::to_vec(&snapshot).map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to serialize character shallow index: {}",
                error
            ))
        })?;

        if let Some(parent) = self.shallow_index_path.parent() {
            fs::create_dir_all(parent).await.map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to create character shallow index directory '{}': {}",
                    parent.display(),
                    error
                ))
            })?;
        }

        let temp_path = unique_temp_path(&self.shallow_index_path);
        fs::write(&temp_path, bytes).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to write character shallow index temp file '{}': {}",
                temp_path.display(),
                error
            ))
        })?;
        replace_file(&temp_path, &self.shallow_index_path).await
    }

    async fn scan_shallow_index_entries(
        &self,
    ) -> Result<(Vec<CharacterShallowIndexScanEntry>, bool), DomainError> {
        let character_files = list_files_with_extension(&self.characters_dir, "png").await?;
        let mut chat_stats_by_name = self
            .calculate_shallow_index_chat_stats(&character_files)
            .await;
        let mut results: Vec<Option<CharacterShallowIndexScanEntry>> =
            (0..character_files.len()).map(|_| None).collect();
        let mut complete = true;
        let semaphore = Arc::new(Semaphore::new(Self::shallow_index_parallelism()));
        let mut jobs = JoinSet::new();

        for (index, path) in character_files.into_iter().enumerate() {
            let permit = semaphore.clone().acquire_owned().await.map_err(|_| {
                DomainError::InternalError(
                    "Shallow character index scanner gate closed".to_string(),
                )
            })?;
            let file_stem = Self::file_stem_from_path(&path);
            let chat_stats = chat_stats_by_name.remove(&file_stem).unwrap_or_else(|| {
                Err(DomainError::InternalError(format!(
                    "Missing chat stats for character '{}'",
                    file_stem
                )))
            });

            jobs.spawn(async move {
                let _permit = permit;
                let result = Self::scan_shallow_index_entry(chat_stats, path).await;
                (index, result)
            });
        }

        while let Some(joined) = jobs.join_next().await {
            let (index, result) = joined.map_err(|error| {
                DomainError::InternalError(format!(
                    "Shallow character index scanner failed: {}",
                    error
                ))
            })?;

            match result {
                Ok(entry) => results[index] = Some(entry),
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
        let mut entries: Vec<_> = results.into_iter().flatten().collect();
        entries.sort_by(|left, right| left.signature.avatar.cmp(&right.signature.avatar));

        Ok((entries, complete))
    }

    async fn calculate_shallow_index_chat_stats(
        &self,
        character_files: &[PathBuf],
    ) -> HashMap<String, Result<(u64, i64), DomainError>> {
        let character_names: Vec<String> = character_files
            .iter()
            .map(|path| Self::file_stem_from_path(path))
            .collect();

        match self
            .chat_repository
            .calculate_character_chat_stats_batch(character_names.clone())
            .await
        {
            Ok(stats) => stats.into_iter().collect(),
            Err(error) => {
                tracing::error!(
                    target: tt_contracts::observability::USER_VISIBLE_ERROR,
                    "Failed to calculate character chat statistics; using zero statistics: {}",
                    error
                );
                character_names
                    .into_iter()
                    .map(|name| (name, Ok((0, 0))))
                    .collect()
            }
        }
    }

    async fn scan_shallow_index_entry(
        chat_stats: Result<(u64, i64), DomainError>,
        path: PathBuf,
    ) -> Result<CharacterShallowIndexScanEntry, DomainError> {
        let metadata = fs::metadata(&path).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read character metadata '{}': {}",
                path.display(),
                error
            ))
        })?;
        let file_stem = Self::file_stem_from_path(&path);
        let avatar = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("")
            .to_string();
        let (chat_size, date_last_chat) = Self::chat_stats_or_default(&file_stem, chat_stats);
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

    fn file_stem_from_path(path: &Path) -> String {
        path.file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("")
            .to_string()
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
                && let Some(reused) = Self::reuse_cached_shallow_character(cached, &entry.signature)
            {
                results[index] = Some(reused);
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
        let mut character = Character::from_card_value(&raw_value).ok_or_else(|| {
            DomainError::InvalidData("Character payload must be a JSON object".to_string())
        })?;

        Self::sync_canonical_data_fields(&mut character, &raw_value);
        Self::normalize_imported_character(&mut character)?;
        let data_size = Self::calculate_character_data_size(&raw_value, &character);
        character.shallow = false;
        let signature = entry.signature;
        character.file_name = Some(entry.file_stem);
        character.avatar = signature.avatar;
        character.date_added = signature.created_millis;
        character.chat_size = signature.chat_size;
        character.data_size = data_size;
        character.date_last_chat = signature.date_last_chat;

        Ok(character.into_shallow())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cached_card_is_reused_when_only_chat_stats_change() {
        let cached = CharacterShallowIndexCachedCharacter {
            signature: CharacterShallowIndexEntrySignature {
                avatar: "Alice.png".to_string(),
                file_size: 100,
                modified_millis: 2,
                created_millis: 1,
                chat_size: 0,
                date_last_chat: 0,
            },
            character: Character::new(
                "Alice".to_string(),
                String::new(),
                String::new(),
                String::new(),
            )
            .into_shallow(),
        };

        let mut updated_signature = cached.signature.clone();
        updated_signature.chat_size = 42;
        updated_signature.date_last_chat = 3;
        let reused =
            FileCharacterRepository::reuse_cached_shallow_character(&cached, &updated_signature)
                .expect("unchanged card should be reusable");

        assert_eq!(reused.signature, updated_signature);
        assert_eq!(reused.character.chat_size, 42);
        assert_eq!(reused.character.date_last_chat, 3);

        let mut changed_card = updated_signature;
        changed_card.modified_millis = 4;
        assert!(
            FileCharacterRepository::reuse_cached_shallow_character(&cached, &changed_card)
                .is_none()
        );
    }
}
