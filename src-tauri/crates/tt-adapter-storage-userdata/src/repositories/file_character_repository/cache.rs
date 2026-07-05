use std::collections::HashMap;
use std::time::{Duration, Instant};

use tt_domain::models::character::Character;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CharacterShallowIndexEntrySignature {
    pub(crate) avatar: String,
    pub(crate) file_size: u64,
    pub(crate) modified_millis: i64,
    pub(crate) created_millis: i64,
    pub(crate) chat_size: u64,
    pub(crate) date_last_chat: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CharacterShallowIndexSignature {
    pub(crate) entries: Vec<CharacterShallowIndexEntrySignature>,
}

#[derive(Debug, Clone)]
pub(crate) struct CharacterShallowIndexCachedCharacter {
    pub(crate) signature: CharacterShallowIndexEntrySignature,
    pub(crate) character: Character,
}

#[derive(Debug, Clone)]
pub(crate) struct CharacterShallowIndexCache {
    pub(crate) signature: CharacterShallowIndexSignature,
    pub(crate) characters: Vec<CharacterShallowIndexCachedCharacter>,
}

/// Memory cache for character data.
pub(crate) struct MemoryCache {
    characters: HashMap<String, (Character, Instant)>,
    capacity: usize,
    ttl: Duration,
}

impl MemoryCache {
    pub(crate) fn new(capacity: usize, ttl: Duration) -> Self {
        Self {
            characters: HashMap::with_capacity(capacity),
            capacity,
            ttl,
        }
    }

    pub(crate) fn get(&self, name: &str) -> Option<Character> {
        if let Some((character, timestamp)) = self.characters.get(name)
            && timestamp.elapsed() < self.ttl
        {
            return Some(character.clone());
        }

        None
    }

    pub(crate) fn set(&mut self, name: String, character: Character) {
        if self.characters.len() >= self.capacity
            && !self.characters.contains_key(&name)
            && let Some((oldest_key, _)) = self
                .characters
                .iter()
                .min_by_key(|(_, (_, timestamp))| timestamp.elapsed())
        {
            let oldest_key = oldest_key.clone();
            self.characters.remove(&oldest_key);
        }

        self.characters.insert(name, (character, Instant::now()));
    }

    pub(crate) fn remove(&mut self, name: &str) {
        self.characters.remove(name);
    }

    pub(crate) fn clear(&mut self) {
        self.characters.clear();
    }
}
