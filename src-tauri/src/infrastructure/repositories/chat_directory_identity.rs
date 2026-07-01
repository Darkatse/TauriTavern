use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::Utc;
use percent_encoding::percent_decode_str;
use serde::{Deserialize, Serialize};
use tokio::fs;
use tokio::sync::Mutex;

use tt_adapter_storage_core::file_system::{replace_file_with_fallback, unique_temp_path};
use tt_domain::errors::DomainError;
use tt_domain::models::filename::sanitize_filename;

const CHAT_ALIAS_VERSION: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ChatAliasEntry {
    dir: String,
    reason: String,
    created_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ChatAliasFile {
    version: u32,
    #[serde(default)]
    aliases: HashMap<String, ChatAliasEntry>,
}

impl Default for ChatAliasFile {
    fn default() -> Self {
        Self {
            version: CHAT_ALIAS_VERSION,
            aliases: HashMap::new(),
        }
    }
}

pub(crate) struct ChatAliasStore {
    path: PathBuf,
    loaded: bool,
    aliases: HashMap<String, ChatAliasEntry>,
}

pub(crate) type SharedChatAliasStore = Arc<Mutex<ChatAliasStore>>;

impl ChatAliasStore {
    pub(crate) fn new(path: PathBuf) -> Self {
        Self {
            path,
            loaded: false,
            aliases: HashMap::new(),
        }
    }

    async fn ensure_loaded(&mut self) -> Result<(), DomainError> {
        if self.loaded {
            return Ok(());
        }

        let bytes = match fs::read(&self.path).await {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                self.loaded = true;
                return Ok(());
            }
            Err(error) => {
                return Err(DomainError::InternalError(format!(
                    "Failed to read chat alias file {:?}: {}",
                    self.path, error
                )));
            }
        };

        let file = serde_json::from_slice::<ChatAliasFile>(&bytes).map_err(|error| {
            DomainError::InvalidData(format!(
                "Failed to parse chat alias file {:?}: {}",
                self.path, error
            ))
        })?;

        if file.version != CHAT_ALIAS_VERSION {
            return Err(DomainError::InvalidData(format!(
                "Unsupported chat alias file version {}",
                file.version
            )));
        }

        self.aliases = file.aliases;
        self.loaded = true;
        Ok(())
    }

    async fn reload(&mut self) -> Result<(), DomainError> {
        self.loaded = false;
        self.aliases.clear();
        self.ensure_loaded().await
    }

    async fn get(&mut self, character_key: &str) -> Result<Option<String>, DomainError> {
        self.ensure_loaded().await?;
        Ok(self
            .aliases
            .get(character_key)
            .map(|entry| entry.dir.clone()))
    }

    async fn dir_is_mapped_to_other(
        &mut self,
        character_key: &str,
        dir_key: &str,
    ) -> Result<bool, DomainError> {
        self.reload().await?;
        Ok(self
            .aliases
            .iter()
            .any(|(key, entry)| key != character_key && entry.dir == dir_key))
    }

    async fn set_legacy_alias(
        &mut self,
        character_key: &str,
        dir_key: &str,
    ) -> Result<(), DomainError> {
        self.reload().await?;

        if let Some(existing) = self.aliases.get(character_key) {
            if existing.dir == dir_key {
                return Ok(());
            }

            return Err(DomainError::InvalidData(format!(
                "Conflicting chat alias for {}: {} != {}",
                character_key, existing.dir, dir_key
            )));
        }

        self.aliases.insert(
            character_key.to_string(),
            ChatAliasEntry {
                dir: dir_key.to_string(),
                reason: "legacy-avatar-url-normalizer".to_string(),
                created_at: Utc::now().to_rfc3339(),
            },
        );
        self.flush().await
    }

    async fn flush(&self) -> Result<(), DomainError> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).await.map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to create chat alias directory {:?}: {}",
                    parent, error
                ))
            })?;
        }

        let file = ChatAliasFile {
            version: CHAT_ALIAS_VERSION,
            aliases: self.aliases.clone(),
        };
        let bytes = serde_json::to_vec_pretty(&file).map_err(|error| {
            DomainError::InternalError(format!("Failed to serialize chat aliases: {}", error))
        })?;

        let temp_path = unique_temp_path(&self.path, "chat_aliases_v1.json");
        fs::write(&temp_path, bytes).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to write chat alias temp file {:?}: {}",
                temp_path, error
            ))
        })?;
        replace_file_with_fallback(&temp_path, &self.path).await
    }
}

pub(crate) fn chat_alias_path_for_user_dir(default_user_dir: &Path) -> PathBuf {
    default_user_dir
        .join("user")
        .join("cache")
        .join("chat_aliases_v1.json")
}

pub(crate) fn new_shared_chat_alias_store(path: PathBuf) -> SharedChatAliasStore {
    Arc::new(Mutex::new(ChatAliasStore::new(path)))
}

pub(crate) fn new_shared_chat_alias_store_for_user_dir(
    default_user_dir: &Path,
) -> SharedChatAliasStore {
    new_shared_chat_alias_store(chat_alias_path_for_user_dir(default_user_dir))
}

pub(crate) fn sanitize_chat_dir_key(value: &str, fallback: &str) -> String {
    let sanitized = sanitize_filename(value);
    if sanitized.is_empty() {
        fallback.to_string()
    } else {
        sanitized
    }
}

/// Resolve the physical chat directory for a character stem.
///
/// The JS/API boundary contract is `avatar_url = exact avatar filename`.
/// Rust chat repositories already receive the corresponding avatar stem as
/// their internal key. Do not trim, URL-decode, strip query/hash, or take a
/// basename here; those transformations are only reproduced below to locate
/// directories created by the former broken normalizer.
pub(crate) async fn resolve_character_chat_dir_key(
    characters_dir: &Path,
    chats_dir: &Path,
    aliases: &SharedChatAliasStore,
    character_name: &str,
) -> Result<String, DomainError> {
    let canonical_key = sanitize_chat_dir_key(character_name, "character");

    if let Some(alias_key) = existing_alias_dir_key(chats_dir, aliases, &canonical_key).await? {
        return Ok(alias_key);
    }

    let canonical_dir = chats_dir.join(&canonical_key);
    if path_is_dir(&canonical_dir).await? {
        return Ok(canonical_key);
    }

    if let Some(legacy_key) =
        resolve_legacy_dir_key(characters_dir, chats_dir, aliases, &canonical_key).await?
    {
        return Ok(legacy_key);
    }

    Ok(canonical_key)
}

async fn existing_alias_dir_key(
    chats_dir: &Path,
    aliases: &SharedChatAliasStore,
    canonical_key: &str,
) -> Result<Option<String>, DomainError> {
    let alias_key = {
        let mut aliases = aliases.lock().await;
        aliases.get(canonical_key).await?
    };

    let Some(alias_key) = alias_key else {
        return Ok(None);
    };

    let alias_dir = chats_dir.join(&alias_key);
    if path_is_dir(&alias_dir).await? {
        Ok(Some(alias_key))
    } else {
        Ok(None)
    }
}

async fn resolve_legacy_dir_key(
    characters_dir: &Path,
    chats_dir: &Path,
    aliases: &SharedChatAliasStore,
    canonical_key: &str,
) -> Result<Option<String>, DomainError> {
    let mut matches = Vec::new();
    for candidate_key in legacy_chat_dir_candidate_keys(canonical_key) {
        if legacy_candidate_is_ambiguous(characters_dir, canonical_key, &candidate_key).await? {
            continue;
        }

        let dir = chats_dir.join(&candidate_key);
        if dir_has_jsonl(&dir).await? {
            matches.push(candidate_key);
        }
    }

    match matches.len() {
        0 => Ok(None),
        1 => {
            let legacy_key = matches.remove(0);
            let mut aliases = aliases.lock().await;
            if aliases
                .dir_is_mapped_to_other(canonical_key, &legacy_key)
                .await?
            {
                return Err(DomainError::InvalidData(format!(
                    "Legacy chat directory {} is already mapped to another character",
                    legacy_key
                )));
            }
            aliases.set_legacy_alias(canonical_key, &legacy_key).await?;
            Ok(Some(legacy_key))
        }
        _ => Err(DomainError::InvalidData(format!(
            "Ambiguous legacy chat directories for {}: {}",
            canonical_key,
            matches.join(", ")
        ))),
    }
}

async fn legacy_candidate_is_ambiguous(
    characters_dir: &Path,
    canonical_key: &str,
    candidate_key: &str,
) -> Result<bool, DomainError> {
    if candidate_key == canonical_key {
        return Ok(true);
    }

    let character_card = characters_dir.join(format!("{}.png", candidate_key));
    if path_is_file(&character_card).await? {
        return Ok(true);
    }

    Ok(false)
}

fn legacy_chat_dir_candidate_keys(canonical_key: &str) -> Vec<String> {
    let mut candidates = Vec::new();
    push_legacy_candidate(&mut candidates, canonical_key.trim());

    let raw_without_fragment = strip_query_or_hash(canonical_key);
    push_legacy_candidate(&mut candidates, raw_without_fragment);

    if let Ok(decoded) = percent_decode_str(canonical_key).decode_utf8() {
        let decoded_without_fragment = strip_query_or_hash(&decoded);
        push_legacy_candidate(&mut candidates, decoded_without_fragment);
        push_legacy_candidate(&mut candidates, legacy_basename(decoded_without_fragment));
    }

    candidates
        .into_iter()
        .filter_map(|candidate| {
            let sanitized = sanitize_chat_dir_key(&candidate, "");
            (!sanitized.is_empty() && sanitized != canonical_key).then_some(sanitized)
        })
        .fold(Vec::new(), |mut unique, candidate| {
            if !unique.contains(&candidate) {
                unique.push(candidate);
            }
            unique
        })
}

fn push_legacy_candidate(candidates: &mut Vec<String>, candidate: &str) {
    if !candidate.is_empty() {
        candidates.push(candidate.to_string());
    }
}

fn strip_query_or_hash(value: &str) -> &str {
    value
        .split_once(['?', '#'])
        .map(|(head, _)| head)
        .unwrap_or(value)
}

fn legacy_basename(value: &str) -> &str {
    value
        .rsplit_once(['/', '\\'])
        .map(|(_, tail)| tail)
        .unwrap_or(value)
}

async fn path_is_dir(path: &Path) -> Result<bool, DomainError> {
    match fs::metadata(path).await {
        Ok(metadata) => Ok(metadata.is_dir()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(DomainError::InternalError(format!(
            "Failed to read chat directory metadata {:?}: {}",
            path, error
        ))),
    }
}

async fn path_is_file(path: &Path) -> Result<bool, DomainError> {
    match fs::metadata(path).await {
        Ok(metadata) => Ok(metadata.is_file()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(DomainError::InternalError(format!(
            "Failed to read character metadata {:?}: {}",
            path, error
        ))),
    }
}

async fn dir_has_jsonl(path: &Path) -> Result<bool, DomainError> {
    let mut entries = match fs::read_dir(path).await {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(DomainError::InternalError(format!(
                "Failed to read legacy chat directory {:?}: {}",
                path, error
            )));
        }
    };

    while let Some(entry) = entries.next_entry().await.map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to read legacy chat directory entry {:?}: {}",
            path, error
        ))
    })? {
        let file_type = entry.file_type().await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read legacy chat entry type {:?}: {}",
                entry.path(),
                error
            ))
        })?;
        if !file_type.is_file() {
            continue;
        }

        if entry
            .path()
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("jsonl"))
        {
            return Ok(true);
        }
    }

    Ok(false)
}
