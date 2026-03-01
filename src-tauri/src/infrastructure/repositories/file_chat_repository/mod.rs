use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Mutex;

mod backup;
mod cache;
mod importing;
mod paths;
mod payload;
mod repository_impl;
mod summary;
mod windowed_payload;

#[cfg(test)]
mod tests;

use self::cache::{MemoryCache, ThrottledBackup};
use self::summary::SummaryCache;

/// File-based chat repository implementation
pub struct FileChatRepository {
    characters_dir: PathBuf,
    chats_dir: PathBuf,
    group_chats_dir: PathBuf,
    backups_dir: PathBuf,
    memory_cache: Arc<Mutex<MemoryCache>>,
    summary_cache: Arc<Mutex<SummaryCache>>,
    throttled_backup: Arc<Mutex<ThrottledBackup>>,
    max_backups_per_chat: usize,
    max_total_backups: usize,
    backup_enabled: bool,
}

impl FileChatRepository {
    const CHAT_BACKUP_PREFIX: &'static str = "chat_";

    /// Create a new FileChatRepository
    pub fn new(
        characters_dir: PathBuf,
        chats_dir: PathBuf,
        group_chats_dir: PathBuf,
        backups_dir: PathBuf,
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

        Self {
            characters_dir,
            chats_dir,
            group_chats_dir,
            backups_dir,
            memory_cache,
            summary_cache,
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
