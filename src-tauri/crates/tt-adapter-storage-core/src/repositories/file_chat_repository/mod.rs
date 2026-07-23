use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Weak};
use std::time::Duration;

use tokio::sync::{Mutex, RwLock};

mod backup;
mod backup_codec;
mod backup_inventory;
mod backup_restore;
mod cache;
mod chat_dir_resolver;
mod chat_payload_commit;
mod extension_metadata;
mod extension_store;
mod group_chat_repository_impl;
mod importing;
mod integrity;
mod locate;
mod message_read;
mod message_search;
mod paths;
mod payload;
mod recent_selection;
mod repository_impl;
mod summary;
mod windowed_payload;
mod windowed_payload_io;

#[cfg(test)]
mod tests;

use self::backup_inventory::BackupHistoryState;
use self::cache::MemoryCache;
use self::summary::SummaryCache;
use crate::chat_directory_identity::{
    SharedChatAliasStore, chat_alias_path_for_user_dir, new_shared_chat_alias_store,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct ContentSignature {
    pub byte_len: u64,
    pub sha256: [u8; 32],
}

#[derive(Debug, Default)]
struct ContentSignatureState {
    epoch: u64,
    entries: HashMap<PathBuf, ContentSignature>,
}

impl ContentSignatureState {
    fn invalidate_all(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
        self.entries.clear();
    }
}

/// File-based chat repository implementation
pub struct FileChatRepository {
    characters_dir: PathBuf,
    chats_dir: PathBuf,
    group_chats_dir: PathBuf,
    backups_dir: PathBuf,
    chat_commit_staging_dir: PathBuf,
    chat_commit_sessions:
        Mutex<HashMap<uuid::Uuid, Arc<Mutex<chat_payload_commit::CommitSession>>>>,
    path_write_locks: Arc<Mutex<HashMap<PathBuf, Weak<Mutex<()>>>>>,
    current_content_signatures: Mutex<ContentSignatureState>,
    memory_cache: Arc<Mutex<MemoryCache>>,
    summary_cache: Arc<Mutex<SummaryCache>>,
    chat_aliases: SharedChatAliasStore,
    backup_policy: Arc<RwLock<tt_domain::models::settings::ChatBackupSettings>>,
    backup_history: Arc<Mutex<BackupHistoryState>>,
}

impl FileChatRepository {
    const CHAT_BACKUP_PREFIX: &'static str = "chat_";

    /// Create an isolated chat repository.
    ///
    /// This is a convenience wrapper for single-repository use. Runtime
    /// bootstrap constructs character and chat repositories together and must
    /// inject a shared `FileChatRepository` into the character repository.
    #[allow(dead_code)]
    pub fn new(
        characters_dir: PathBuf,
        chats_dir: PathBuf,
        group_chats_dir: PathBuf,
        backups_dir: PathBuf,
    ) -> Self {
        let chat_aliases_path = backups_dir
            .parent()
            .map(chat_alias_path_for_user_dir)
            .unwrap_or_else(|| backups_dir.join("chat_aliases_v1.json"));
        let chat_aliases = new_shared_chat_alias_store(chat_aliases_path);
        Self::with_chat_aliases(
            characters_dir,
            chats_dir,
            group_chats_dir,
            backups_dir,
            chat_aliases,
        )
    }

    /// Create a repository with the shared character/chat alias store.
    ///
    /// Character and chat repositories must share this store in production so
    /// lazy legacy-dir aliases are serialized through one cache. Prefer this
    /// constructor whenever both repositories are created for the same runtime.
    pub fn with_chat_aliases(
        characters_dir: PathBuf,
        chats_dir: PathBuf,
        group_chats_dir: PathBuf,
        backups_dir: PathBuf,
        chat_aliases: SharedChatAliasStore,
    ) -> Self {
        Self::with_chat_aliases_and_backup_settings(
            characters_dir,
            chats_dir,
            group_chats_dir,
            backups_dir,
            chat_aliases,
            tt_domain::models::settings::ChatBackupSettings::default(),
        )
    }

    pub fn with_chat_aliases_and_backup_settings(
        characters_dir: PathBuf,
        chats_dir: PathBuf,
        group_chats_dir: PathBuf,
        backups_dir: PathBuf,
        chat_aliases: SharedChatAliasStore,
        backup_settings: tt_domain::models::settings::ChatBackupSettings,
    ) -> Self {
        let chat_commit_staging_dir = backups_dir.with_file_name(".staging").join("chat-commits");
        // Create a memory cache with 100 chat capacity and 30 minute TTL
        let memory_cache = Arc::new(Mutex::new(MemoryCache::new(
            100,
            Duration::from_secs(30 * 60),
        )));
        let summary_index_path = backups_dir
            .parent()
            .map(|default_user_dir| {
                default_user_dir
                    .join("user")
                    .join("cache")
                    .join("chat_summary_index_v1.json")
            })
            .unwrap_or_else(|| backups_dir.join("chat_summary_index_v1.json"));
        let summary_cache = Arc::new(Mutex::new(SummaryCache::new(summary_index_path)));

        let path_write_locks = Arc::new(Mutex::new(HashMap::new()));

        Self {
            characters_dir,
            chats_dir,
            group_chats_dir,
            backups_dir,
            chat_commit_staging_dir,
            chat_commit_sessions: Mutex::new(HashMap::new()),
            path_write_locks,
            current_content_signatures: Mutex::new(ContentSignatureState::default()),
            memory_cache,
            summary_cache,
            chat_aliases,
            backup_policy: Arc::new(RwLock::new(backup_settings)),
            backup_history: Arc::new(Mutex::new(BackupHistoryState::new())),
        }
    }
}
