mod cache;
mod helpers;
mod importer;
mod repository;
mod shallow_index;

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Mutex;
use tt_ports::avatar_images::AvatarImageProcessor;

use self::cache::{CharacterShallowIndexCache, MemoryCache};
use tt_adapter_storage_core::{FileChatRepository, chat_directory_identity::SharedChatAliasStore};

/// File-based character repository implementation.
pub struct FileCharacterRepository {
    characters_dir: PathBuf,
    chats_dir: PathBuf,
    default_avatar_path: PathBuf,
    avatar_images: Arc<dyn AvatarImageProcessor>,
    shallow_index_path: PathBuf,
    memory_cache: Arc<Mutex<MemoryCache>>,
    shallow_index_cache: Arc<Mutex<Option<CharacterShallowIndexCache>>>,
    chat_aliases: SharedChatAliasStore,
    chat_repository: Arc<FileChatRepository>,
}

impl FileCharacterRepository {
    pub fn with_chat_repository(
        characters_dir: PathBuf,
        chats_dir: PathBuf,
        default_avatar_path: PathBuf,
        avatar_images: Arc<dyn AvatarImageProcessor>,
        chat_aliases: SharedChatAliasStore,
        chat_repository: Arc<FileChatRepository>,
    ) -> Self {
        let shallow_index_path = character_shallow_index_path_for_characters_dir(&characters_dir);
        let memory_cache = Arc::new(Mutex::new(MemoryCache::new(
            100,
            Duration::from_secs(30 * 60),
        )));
        let shallow_index_cache = Arc::new(Mutex::new(None));

        Self {
            characters_dir,
            chats_dir,
            default_avatar_path,
            avatar_images,
            shallow_index_path,
            memory_cache,
            shallow_index_cache,
            chat_aliases,
            chat_repository,
        }
    }
}

fn character_shallow_index_path_for_characters_dir(characters_dir: &Path) -> PathBuf {
    characters_dir
        .parent()
        .map(|default_user_dir| {
            default_user_dir
                .join("user")
                .join("cache")
                .join("character_shallow_index_v1.json")
        })
        .unwrap_or_else(|| characters_dir.join("character_shallow_index_v1.json"))
}
