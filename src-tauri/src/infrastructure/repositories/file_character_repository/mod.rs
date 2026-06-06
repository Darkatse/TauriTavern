mod cache;
mod helpers;
mod importer;
mod repository;

#[cfg(test)]
mod tests;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Mutex;

use self::cache::MemoryCache;
use crate::infrastructure::repositories::chat_directory_identity::{
    SharedChatAliasStore, chat_alias_path_for_user_dir, new_shared_chat_alias_store,
};

/// File-based character repository implementation.
pub struct FileCharacterRepository {
    characters_dir: PathBuf,
    chats_dir: PathBuf,
    thumbnails_avatar_dir: PathBuf,
    default_avatar_path: PathBuf,
    memory_cache: Arc<Mutex<MemoryCache>>,
    chat_aliases: SharedChatAliasStore,
}

impl FileCharacterRepository {
    /// Create a new `FileCharacterRepository`.
    pub fn new(
        characters_dir: PathBuf,
        chats_dir: PathBuf,
        thumbnails_avatar_dir: PathBuf,
        default_avatar_path: PathBuf,
    ) -> Self {
        let chat_aliases_path = chats_dir
            .parent()
            .map(chat_alias_path_for_user_dir)
            .unwrap_or_else(|| chats_dir.join("chat_aliases_v1.json"));
        let chat_aliases = new_shared_chat_alias_store(chat_aliases_path);
        Self::with_chat_aliases(
            characters_dir,
            chats_dir,
            thumbnails_avatar_dir,
            default_avatar_path,
            chat_aliases,
        )
    }

    pub(crate) fn with_chat_aliases(
        characters_dir: PathBuf,
        chats_dir: PathBuf,
        thumbnails_avatar_dir: PathBuf,
        default_avatar_path: PathBuf,
        chat_aliases: SharedChatAliasStore,
    ) -> Self {
        let memory_cache = Arc::new(Mutex::new(MemoryCache::new(
            100,
            Duration::from_secs(30 * 60),
        )));

        Self {
            characters_dir,
            chats_dir,
            thumbnails_avatar_dir,
            default_avatar_path,
            memory_cache,
            chat_aliases,
        }
    }
}
