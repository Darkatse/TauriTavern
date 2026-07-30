use std::collections::HashMap;
use std::time::{Duration, Instant};

use tt_domain::models::chat::Chat;

/// Memory cache for chat data
pub(super) struct MemoryCache {
    chats: HashMap<String, (Chat, Instant)>,
    capacity: usize,
    ttl: Duration,
}

impl MemoryCache {
    /// Create a new memory cache with the specified capacity and TTL
    pub(super) fn new(capacity: usize, ttl: Duration) -> Self {
        Self {
            chats: HashMap::with_capacity(capacity),
            capacity,
            ttl,
        }
    }

    /// Get a chat from the cache
    pub(super) fn get(&self, key: &str) -> Option<Chat> {
        if let Some((chat, timestamp)) = self.chats.get(key)
            && timestamp.elapsed() < self.ttl
        {
            return Some(chat.clone());
        }
        None
    }

    /// Set a chat in the cache
    pub(super) fn set(&mut self, key: String, chat: Chat) {
        // If we're at capacity, remove the oldest entry
        if self.chats.len() >= self.capacity
            && !self.chats.contains_key(&key)
            && let Some((oldest_key, _)) = self
                .chats
                .iter()
                .max_by_key(|(_, (_, timestamp))| timestamp.elapsed())
        {
            let oldest_key = oldest_key.clone();
            self.chats.remove(&oldest_key);
        }

        self.chats.insert(key, (chat, Instant::now()));
    }

    /// Remove a chat from the cache
    pub(super) fn remove(&mut self, key: &str) {
        self.chats.remove(key);
    }

    /// Clear the cache
    pub(super) fn clear(&mut self) {
        self.chats.clear();
    }
}
