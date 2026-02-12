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

/// File-based character repository implementation.
pub struct FileCharacterRepository {
    characters_dir: PathBuf,
    chats_dir: PathBuf,
    default_avatar_path: PathBuf,
    memory_cache: Arc<Mutex<MemoryCache>>,
}

impl FileCharacterRepository {
    /// Create a new `FileCharacterRepository`.
    pub fn new(characters_dir: PathBuf, chats_dir: PathBuf, default_avatar_path: PathBuf) -> Self {
        let memory_cache = Arc::new(Mutex::new(MemoryCache::new(
            100,
            Duration::from_secs(30 * 60),
        )));

        Self {
            characters_dir,
            chats_dir,
            default_avatar_path,
            memory_cache,
        }
    }
}
