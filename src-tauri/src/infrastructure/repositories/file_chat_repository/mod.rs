use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Weak};
use std::time::Duration;

use tokio::sync::Mutex;

mod backup;
mod cache;
mod chat_dir_resolver;
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
mod windowed_patch;
mod windowed_payload;
mod windowed_payload_io;

#[cfg(test)]
mod tests;

use self::cache::{MemoryCache, ThrottledBackup};
use self::summary::SummaryCache;
use crate::infrastructure::repositories::chat_directory_identity::{
    SharedChatAliasStore, chat_alias_path_for_user_dir, new_shared_chat_alias_store,
};

/// File-based chat repository implementation
pub struct FileChatRepository {
    characters_dir: PathBuf,
    chats_dir: PathBuf,
    group_chats_dir: PathBuf,
    backups_dir: PathBuf,
    path_write_locks: Arc<Mutex<HashMap<PathBuf, Weak<Mutex<()>>>>>,
    memory_cache: Arc<Mutex<MemoryCache>>,
    summary_cache: Arc<Mutex<SummaryCache>>,
    chat_aliases: SharedChatAliasStore,
    throttled_backup: Arc<Mutex<ThrottledBackup>>,
    max_backups_per_chat: usize,
    max_total_backups: usize,
    backup_enabled: bool,
}

impl FileChatRepository {
    const CHAT_BACKUP_PREFIX: &'static str = "chat_";

    /// Create an isolated chat repository.
    ///
    /// This is a convenience wrapper for single-repository use. Runtime
    /// bootstrap constructs character and chat repositories together and must
    /// call `with_chat_aliases` so both repositories share one alias store.
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
    pub(crate) fn with_chat_aliases(
        characters_dir: PathBuf,
        chats_dir: PathBuf,
        group_chats_dir: PathBuf,
        backups_dir: PathBuf,
        chat_aliases: SharedChatAliasStore,
    ) -> Self {
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

        // Match SillyTavern default: backups.chat.throttleInterval = 10_000ms
        let throttled_backup = Arc::new(Mutex::new(ThrottledBackup::new(10)));
        let path_write_locks = Arc::new(Mutex::new(HashMap::new()));

        Self {
            characters_dir,
            chats_dir,
            group_chats_dir,
            backups_dir,
            path_write_locks,
            memory_cache,
            summary_cache,
            chat_aliases,
            throttled_backup,
            // Match SillyTavern defaults:
            // - per-chat backups: 50
            // - total backups: unlimited (-1 in SillyTavern config)
            max_backups_per_chat: 50,
            max_total_backups: usize::MAX,
            backup_enabled: true,
        }
    }
}
