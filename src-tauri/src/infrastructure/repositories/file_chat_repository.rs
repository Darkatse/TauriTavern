use async_trait::async_trait;
use chrono::Local;
use rand::random;
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Mutex;

use crate::domain::errors::DomainError;
use crate::domain::models::chat::{humanized_date, Chat, ChatMessage};
use crate::domain::repositories::chat_repository::{
    ChatExportFormat, ChatImportFormat, ChatRepository, ChatSearchResult,
};
use crate::infrastructure::logging::logger;
use crate::infrastructure::persistence::chat_format_importers::{
    export_to_plain_text, import_from_agnai, import_from_cai_tools, import_from_kobold_lite,
    import_from_ooba, import_from_risuai, import_from_sillytavern,
};
use crate::infrastructure::persistence::file_system::{delete_file, list_files_with_extension};
use crate::infrastructure::persistence::jsonl_utils::{
    append_jsonl_file, check_chat_integrity, read_first_line, read_jsonl_file, read_last_line,
    write_jsonl_file,
};

/// Memory cache for chat data
struct MemoryCache {
    chats: HashMap<String, (Chat, Instant)>,
    capacity: usize,
    ttl: Duration,
}

impl MemoryCache {
    /// Create a new memory cache with the specified capacity and TTL
    fn new(capacity: usize, ttl: Duration) -> Self {
        Self {
            chats: HashMap::with_capacity(capacity),
            capacity,
            ttl,
        }
    }

    /// Get a chat from the cache
    fn get(&self, key: &str) -> Option<Chat> {
        if let Some((chat, timestamp)) = self.chats.get(key) {
            if timestamp.elapsed() < self.ttl {
                return Some(chat.clone());
            }
        }
        None
    }

    /// Set a chat in the cache
    fn set(&mut self, key: String, chat: Chat) {
        // If we're at capacity, remove the oldest entry
        if self.chats.len() >= self.capacity && !self.chats.contains_key(&key) {
            if let Some((oldest_key, _)) = self
                .chats
                .iter()
                .min_by_key(|(_, (_, timestamp))| timestamp.elapsed())
            {
                let oldest_key = oldest_key.clone();
                self.chats.remove(&oldest_key);
            }
        }

        self.chats.insert(key, (chat, Instant::now()));
    }

    /// Remove a chat from the cache
    fn remove(&mut self, key: &str) {
        self.chats.remove(key);
    }

    /// Clear the cache
    fn clear(&mut self) {
        self.chats.clear();
    }
}

/// Throttled function for backups
struct ThrottledBackup {
    last_backup: HashMap<String, Instant>,
    interval: Duration,
}

impl ThrottledBackup {
    /// Create a new throttled backup with the specified interval
    fn new(interval_seconds: u64) -> Self {
        Self {
            last_backup: HashMap::new(),
            interval: Duration::from_secs(interval_seconds),
        }
    }

    /// Check if a backup should be performed
    fn should_backup(&self, key: &str) -> bool {
        if let Some(last) = self.last_backup.get(key) {
            last.elapsed() >= self.interval
        } else {
            true
        }
    }

    /// Update the last backup time
    fn update(&mut self, key: &str) {
        self.last_backup.insert(key.to_string(), Instant::now());
    }
}

/// File-based chat repository implementation
pub struct FileChatRepository {
    chats_dir: PathBuf,
    backups_dir: PathBuf,
    memory_cache: Arc<Mutex<MemoryCache>>,
    throttled_backup: Arc<Mutex<ThrottledBackup>>,
    max_backups_per_chat: usize,
    max_total_backups: usize,
    backup_enabled: bool,
}

impl FileChatRepository {
    const CHAT_BACKUP_PREFIX: &'static str = "chat_";

    /// Create a new FileChatRepository
    pub fn new(chats_dir: PathBuf) -> Self {
        // Create a memory cache with 100 chat capacity and 30 minute TTL
        let memory_cache = Arc::new(Mutex::new(MemoryCache::new(
            100,
            Duration::from_secs(30 * 60),
        )));

        // Create a throttled backup with 5 minute interval
        let throttled_backup = Arc::new(Mutex::new(ThrottledBackup::new(5 * 60)));

        // Create backups directory
        let backups_dir = chats_dir.join("backups");

        Self {
            chats_dir,
            backups_dir,
            memory_cache,
            throttled_backup,
            // Match SillyTavern defaults:
            // - per-chat backups: 50
            // - total backups: unlimited (-1 in SillyTavern config)
            max_backups_per_chat: 50,
            max_total_backups: usize::MAX,
            backup_enabled: true,
        }
    }

    /// Ensure the chats directory exists
    async fn ensure_directory_exists(&self) -> Result<(), DomainError> {
        if !self.chats_dir.exists() {
            tracing::info!("Creating chats directory: {:?}", self.chats_dir);
            fs::create_dir_all(&self.chats_dir).await.map_err(|e| {
                tracing::error!("Failed to create chats directory: {}", e);
                DomainError::InternalError(format!("Failed to create chats directory: {}", e))
            })?;
        }

        if !self.backups_dir.exists() {
            tracing::info!("Creating backups directory: {:?}", self.backups_dir);
            fs::create_dir_all(&self.backups_dir).await.map_err(|e| {
                tracing::error!("Failed to create backups directory: {}", e);
                DomainError::InternalError(format!("Failed to create backups directory: {}", e))
            })?;
        }

        Ok(())
    }

    /// Get the path to a character's chat directory
    fn get_character_dir(&self, character_name: &str) -> PathBuf {
        self.chats_dir.join(character_name)
    }

    /// Ensure chat file names always use the JSONL extension
    fn normalize_jsonl_file_name(file_name: &str) -> String {
        if file_name.ends_with(".jsonl") {
            file_name.to_string()
        } else {
            format!("{}.jsonl", file_name)
        }
    }

    /// Remove JSONL extension if present
    fn strip_jsonl_extension(file_name: &str) -> &str {
        file_name.strip_suffix(".jsonl").unwrap_or(file_name)
    }

    /// Build a timestamp that is safe to use in file names on all platforms.
    fn backup_timestamp() -> String {
        Local::now().format("%Y%m%d-%H%M%S").to_string()
    }

    /// Mirrors SillyTavern backup name normalization:
    /// sanitize(name).replace(/[^a-z0-9]/gi, '_').toLowerCase()
    fn sanitize_backup_name_for_sillytavern(input: &str) -> String {
        let mut sanitized = String::with_capacity(input.len());

        for ch in input.chars() {
            let is_invalid = matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')
                || ch.is_control();
            if !is_invalid {
                sanitized.push(ch);
            }
        }

        let trimmed = sanitized.trim_matches([' ', '.']).to_string();
        let lowered = trimmed.to_ascii_lowercase();

        let is_reserved = matches!(
            lowered.as_str(),
            "" | "."
                | ".."
                | "con"
                | "prn"
                | "aux"
                | "nul"
                | "com1"
                | "com2"
                | "com3"
                | "com4"
                | "com5"
                | "com6"
                | "com7"
                | "com8"
                | "com9"
                | "lpt1"
                | "lpt2"
                | "lpt3"
                | "lpt4"
                | "lpt5"
                | "lpt6"
                | "lpt7"
                | "lpt8"
                | "lpt9"
        );

        if is_reserved {
            return String::new();
        }

        lowered
            .chars()
            .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
            .collect()
    }

    fn backup_file_prefix(character_name: &str) -> String {
        format!(
            "{}{}_",
            Self::CHAT_BACKUP_PREFIX,
            Self::sanitize_backup_name_for_sillytavern(character_name)
        )
    }

    /// Build backup file name in the form `chat_<sanitized_character>_<timestamp>.jsonl`.
    fn backup_file_name(character_name: &str) -> String {
        format!(
            "{}{}.jsonl",
            Self::backup_file_prefix(character_name),
            Self::backup_timestamp()
        )
    }

    /// Get the path to a chat file
    fn get_chat_path(&self, character_name: &str, file_name: &str) -> PathBuf {
        let normalized = Self::normalize_jsonl_file_name(file_name);
        self.get_character_dir(character_name).join(normalized)
    }

    /// Get the path to a chat backup file
    fn get_backup_path(&self, character_name: &str, _file_name: &str) -> PathBuf {
        self.backups_dir
            .join(Self::backup_file_name(character_name))
    }

    /// Get the cache key for a chat
    fn get_cache_key(&self, character_name: &str, file_name: &str) -> String {
        format!(
            "{}:{}",
            character_name,
            Self::strip_jsonl_extension(file_name)
        )
    }

    /// Read a chat from a file
    async fn read_chat_file(
        &self,
        character_name: &str,
        file_name: &str,
    ) -> Result<Chat, DomainError> {
        logger::debug(&format!(
            "Reading chat file: {}/{}",
            character_name, file_name
        ));

        let file_name = Self::normalize_jsonl_file_name(file_name);

        let path = self.get_chat_path(character_name, &file_name);

        // Read the file as JSONL
        let objects: Vec<Value> = read_jsonl_file(&path).await?;

        if objects.is_empty() {
            return Err(DomainError::InvalidData("Empty JSONL file".to_string()));
        }

        // Parse the first object as metadata
        let metadata = &objects[0];

        // Extract metadata fields
        let user_name = metadata["user_name"].as_str().unwrap_or("User").to_string();
        let character_name = metadata["character_name"]
            .as_str()
            .unwrap_or(character_name)
            .to_string();
        let create_date = metadata["create_date"].as_str().unwrap_or("").to_string();

        // Create a new chat
        let mut chat = Chat {
            user_name,
            character_name,
            create_date,
            file_name: Some(Self::strip_jsonl_extension(&file_name).to_string()),
            ..Default::default()
        };

        // Parse chat metadata
        if let Some(chat_metadata) = metadata.get("chat_metadata") {
            if let Ok(parsed) = serde_json::from_value(chat_metadata.clone()) {
                chat.chat_metadata = parsed;
            }
        }

        // Parse messages
        for obj in objects.iter().skip(1) {
            if let Ok(message) = serde_json::from_value(obj.clone()) {
                chat.add_message(message);
            }
        }

        Ok(chat)
    }

    /// Write a chat to a file
    async fn write_chat_file(&self, chat: &Chat) -> Result<(), DomainError> {
        let file_name = chat
            .file_name
            .as_ref()
            .ok_or_else(|| DomainError::InvalidData("Chat file name is not set".to_string()))?;

        logger::debug(&format!(
            "Writing chat file: {}/{}",
            chat.character_name, file_name
        ));

        // Ensure the character directory exists
        let character_dir = self.get_character_dir(&chat.character_name);
        if !character_dir.exists() {
            fs::create_dir_all(&character_dir).await.map_err(|e| {
                logger::error(&format!("Failed to create character directory: {}", e));
                DomainError::InternalError(format!("Failed to create character directory: {}", e))
            })?;
        }

        let path = self.get_chat_path(&chat.character_name, file_name);

        // Create a vector of objects to write
        let mut objects = Vec::new();

        // Add metadata
        let mut metadata = serde_json::Map::new();
        metadata.insert(
            "user_name".to_string(),
            serde_json::Value::String(chat.user_name.clone()),
        );
        metadata.insert(
            "character_name".to_string(),
            serde_json::Value::String(chat.character_name.clone()),
        );
        metadata.insert(
            "create_date".to_string(),
            serde_json::Value::String(chat.create_date.clone()),
        );
        metadata.insert(
            "chat_metadata".to_string(),
            serde_json::to_value(&chat.chat_metadata).unwrap_or(serde_json::Value::Null),
        );

        objects.push(serde_json::Value::Object(metadata));

        // Add messages
        for message in &chat.messages {
            if let Ok(value) = serde_json::to_value(message) {
                objects.push(value);
            }
        }

        // Write the file
        write_jsonl_file(&path, &objects).await?;

        // Update cache
        let cache_key = self.get_cache_key(&chat.character_name, file_name);
        let mut cache = self.memory_cache.lock().await;
        cache.set(cache_key, chat.clone());

        Ok(())
    }

    /// Backup a chat file
    async fn backup_chat_file(
        &self,
        character_name: &str,
        file_name: &str,
    ) -> Result<(), DomainError> {
        if !self.backup_enabled {
            return Ok(());
        }

        let cache_key = self.get_cache_key(character_name, file_name);

        // Check if we should backup
        {
            let throttled = self.throttled_backup.lock().await;
            if !throttled.should_backup(&cache_key) {
                return Ok(());
            }
        }

        logger::debug(&format!(
            "Backing up chat file: {}/{}",
            character_name, file_name
        ));

        // Get the chat file path
        let chat_path = self.get_chat_path(character_name, file_name);

        // Get the backup file path
        let backup_path = self.get_backup_path(character_name, file_name);

        // Copy the file
        fs::copy(&chat_path, &backup_path).await.map_err(|e| {
            logger::error(&format!("Failed to backup chat file: {}", e));
            DomainError::InternalError(format!("Failed to backup chat file: {}", e))
        })?;

        // Update the last backup time
        {
            let mut throttled = self.throttled_backup.lock().await;
            throttled.update(&cache_key);
        }

        // Remove old backups following SillyTavern semantics:
        // 1) per-chat prefix limit
        // 2) global chat_ prefix limit
        let per_chat_prefix = Self::backup_file_prefix(character_name);
        self.remove_old_backups_with_prefix(&per_chat_prefix, self.max_backups_per_chat)
            .await?;
        self.remove_old_backups_with_prefix(Self::CHAT_BACKUP_PREFIX, self.max_total_backups)
            .await?;

        Ok(())
    }

    /// Remove old backups with a specific file name prefix.
    async fn remove_old_backups_with_prefix(
        &self,
        prefix: &str,
        max_backups: usize,
    ) -> Result<(), DomainError> {
        if max_backups == usize::MAX {
            return Ok(());
        }

        logger::debug(&format!("Removing old backups for prefix: {}", prefix));

        // List all backup files
        let mut matching_backups: Vec<(PathBuf, std::fs::Metadata)> = Vec::new();
        for path in list_files_with_extension(&self.backups_dir, "jsonl").await? {
            if let Ok(metadata) = fs::metadata(&path).await {
                let file_name = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();

                if file_name.starts_with(prefix) {
                    matching_backups.push((path, metadata));
                }
            }
        }

        if matching_backups.len() <= max_backups {
            return Ok(());
        }

        // Sort backups by modification time (oldest first)
        matching_backups.sort_by(|(_, a), (_, b)| {
            a.modified()
                .unwrap_or_else(|_| std::time::SystemTime::UNIX_EPOCH)
                .cmp(
                    &b.modified()
                        .unwrap_or_else(|_| std::time::SystemTime::UNIX_EPOCH),
                )
        });

        while matching_backups.len() > max_backups {
            if let Some((path, _)) = matching_backups.first() {
                let path = path.clone();
                if let Err(e) = fs::remove_file(&path).await {
                    logger::error(&format!("Failed to remove old backup {:?}: {}", path, e));
                } else {
                    logger::debug(&format!("Removed old backup: {:?}", path));
                }
            }
            matching_backups.remove(0);
        }

        Ok(())
    }
}

#[async_trait]
impl ChatRepository for FileChatRepository {
    async fn save(&self, chat: &Chat) -> Result<(), DomainError> {
        self.ensure_directory_exists().await?;
        self.write_chat_file(chat).await?;

        // Backup the chat if enabled
        if self.backup_enabled {
            if let Some(file_name) = &chat.file_name {
                self.backup_chat_file(&chat.character_name, file_name)
                    .await?;
            }
        }

        Ok(())
    }

    async fn get_chat(&self, character_name: &str, file_name: &str) -> Result<Chat, DomainError> {
        // Try to get from cache first
        let cache_key = self.get_cache_key(character_name, file_name);

        {
            let cache = self.memory_cache.lock().await;
            if let Some(chat) = cache.get(&cache_key) {
                return Ok(chat);
            }
        }

        // If not in cache, read from file
        let chat = self.read_chat_file(character_name, file_name).await?;

        // Update cache
        {
            let mut cache = self.memory_cache.lock().await;
            cache.set(cache_key, chat.clone());
        }

        Ok(chat)
    }

    async fn get_character_chats(&self, character_name: &str) -> Result<Vec<Chat>, DomainError> {
        logger::debug(&format!("Getting chats for character: {}", character_name));

        // Ensure the character directory exists
        let character_dir = self.get_character_dir(character_name);
        if !character_dir.exists() {
            return Ok(Vec::new());
        }

        // List all JSONL files in the character directory
        let chat_files = list_files_with_extension(&character_dir, "jsonl").await?;
        let mut chats = Vec::new();

        for file_path in chat_files {
            let file_name = file_path
                .file_name()
                .and_then(|f| f.to_str())
                .unwrap_or("")
                .to_string();

            match self.get_chat(character_name, &file_name).await {
                Ok(mut chat) => {
                    // Keep the internal character ID stable for list/read-model flows.
                    // Chat metadata may contain a mutable display name, but filesystem
                    // layout and routing logic are keyed by directory (character_name).
                    chat.character_name = character_name.to_string();
                    chats.push(chat);
                }
                Err(e) => {
                    logger::error(&format!(
                        "Failed to load chat {}/{}: {}",
                        character_name, file_name, e
                    ));
                    // Continue loading other chats
                }
            }
        }

        // Sort chats by last message date (newest first)
        chats.sort_by(|a, b| {
            b.get_last_message_timestamp()
                .cmp(&a.get_last_message_timestamp())
        });

        Ok(chats)
    }

    async fn get_all_chats(&self) -> Result<Vec<Chat>, DomainError> {
        logger::debug("Getting all chats");

        // Ensure the chats directory exists
        self.ensure_directory_exists().await?;

        // List all directories in the chats directory
        let mut entries = fs::read_dir(&self.chats_dir).await.map_err(|e| {
            logger::error(&format!("Failed to read chats directory: {}", e));
            DomainError::InternalError(format!("Failed to read chats directory: {}", e))
        })?;

        let mut all_chats = Vec::new();

        while let Some(entry) = entries.next_entry().await.map_err(|e| {
            logger::error(&format!("Failed to read directory entry: {}", e));
            DomainError::InternalError(format!("Failed to read directory entry: {}", e))
        })? {
            let path = entry.path();

            if path.is_dir() {
                let character_name = path
                    .file_name()
                    .and_then(|f| f.to_str())
                    .unwrap_or("")
                    .to_string();

                if character_name != "backups" {
                    match self.get_character_chats(&character_name).await {
                        Ok(chats) => {
                            all_chats.extend(chats);
                        }
                        Err(e) => {
                            logger::error(&format!(
                                "Failed to load chats for character {}: {}",
                                character_name, e
                            ));
                            // Continue loading other characters
                        }
                    }
                }
            }
        }

        // Sort chats by last message date (newest first)
        all_chats.sort_by(|a, b| {
            b.get_last_message_timestamp()
                .cmp(&a.get_last_message_timestamp())
        });

        Ok(all_chats)
    }

    async fn delete_chat(&self, character_name: &str, file_name: &str) -> Result<(), DomainError> {
        logger::debug(&format!("Deleting chat: {}/{}", character_name, file_name));

        let path = self.get_chat_path(character_name, file_name);

        if !path.exists() {
            return Err(DomainError::NotFound(format!(
                "Chat not found: {}/{}",
                character_name, file_name
            )));
        }

        // Delete the file
        fs::remove_file(&path).await.map_err(|e| {
            logger::error(&format!("Failed to delete chat file: {}", e));
            DomainError::InternalError(format!("Failed to delete chat file: {}", e))
        })?;

        // Remove from cache
        let cache_key = self.get_cache_key(character_name, file_name);
        let mut cache = self.memory_cache.lock().await;
        cache.remove(&cache_key);

        Ok(())
    }

    async fn rename_chat(
        &self,
        character_name: &str,
        old_file_name: &str,
        new_file_name: &str,
    ) -> Result<(), DomainError> {
        logger::debug(&format!(
            "Renaming chat: {}/{} -> {}/{}",
            character_name, old_file_name, character_name, new_file_name
        ));

        // Get the chat
        let mut chat = self.get_chat(character_name, old_file_name).await?;

        // Update the file name
        chat.file_name = Some(new_file_name.to_string());

        // Save the chat with the new file name
        self.write_chat_file(&chat).await?;

        // Delete the old file
        let old_path = self.get_chat_path(character_name, old_file_name);
        fs::remove_file(&old_path).await.map_err(|e| {
            logger::error(&format!("Failed to delete old chat file: {}", e));
            DomainError::InternalError(format!("Failed to delete old chat file: {}", e))
        })?;

        // Update cache
        let old_cache_key = self.get_cache_key(character_name, old_file_name);
        let new_cache_key = self.get_cache_key(character_name, new_file_name);

        let mut cache = self.memory_cache.lock().await;
        cache.remove(&old_cache_key);
        cache.set(new_cache_key, chat);

        Ok(())
    }

    async fn add_message(
        &self,
        character_name: &str,
        file_name: &str,
        message: ChatMessage,
    ) -> Result<Chat, DomainError> {
        logger::debug(&format!(
            "Adding message to chat: {}/{}",
            character_name, file_name
        ));

        // Get the chat
        let mut chat = self.get_chat(character_name, file_name).await?;

        // Add the message
        chat.add_message(message);

        // Save the chat
        self.save(&chat).await?;

        Ok(chat)
    }

    async fn search_chats(
        &self,
        query: &str,
        character_filter: Option<&str>,
    ) -> Result<Vec<ChatSearchResult>, DomainError> {
        logger::debug(&format!("Searching chats for: {}", query));

        let query = query.to_lowercase();
        let mut results = Vec::new();

        // Get all chats
        let chats = if let Some(character) = character_filter {
            self.get_character_chats(character).await?
        } else {
            self.get_all_chats().await?
        };

        // Search for matching chats
        for chat in chats {
            let file_name = chat.get_file_name();
            let preview = chat.get_preview();

            // Check if the file name or any message matches the query
            let file_name_match = file_name.to_lowercase().contains(&query);
            let message_match = chat
                .messages
                .iter()
                .any(|m| m.mes.to_lowercase().contains(&query));

            if file_name_match || message_match {
                // Get the file size
                let path = self.get_chat_path(&chat.character_name, &file_name);
                let (file_size, fallback_date) = if let Ok(metadata) = fs::metadata(&path).await {
                    let fallback_date = metadata
                        .modified()
                        .ok()
                        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|duration| duration.as_millis() as i64)
                        .unwrap_or(0);
                    (metadata.len(), fallback_date)
                } else {
                    (0, 0)
                };
                let date = chat.get_last_message_timestamp();
                let date = if date > 0 { date } else { fallback_date };

                results.push(ChatSearchResult {
                    character_name: chat.character_name.clone(),
                    file_name,
                    file_size,
                    message_count: chat.message_count(),
                    preview,
                    date,
                    chat_id: Some(chat.chat_metadata.chat_id_hash.to_string()),
                });
            }
        }

        // Sort results by date (newest first)
        results.sort_by(|a, b| b.date.cmp(&a.date));

        Ok(results)
    }

    async fn import_chat(
        &self,
        character_name: &str,
        file_path: &Path,
        format: ChatImportFormat,
    ) -> Result<Chat, DomainError> {
        logger::debug(&format!(
            "Importing chat for character {} from {:?}",
            character_name, file_path
        ));

        // Read the file
        let file_data = fs::read_to_string(file_path).await.map_err(|e| {
            logger::error(&format!("Failed to read file: {}", e));
            DomainError::InternalError(format!("Failed to read file: {}", e))
        })?;

        // Parse the file based on the format
        let data: Value = serde_json::from_str(&file_data).map_err(|e| {
            logger::error(&format!("Failed to parse JSON: {}", e));
            DomainError::InvalidData(format!("Failed to parse JSON: {}", e))
        })?;

        // Import the chat based on the format
        let mut chat = match format {
            ChatImportFormat::SillyTavern => import_from_sillytavern(&data)?,
            ChatImportFormat::Ooba => import_from_ooba(&data)?,
            ChatImportFormat::Agnai => import_from_agnai(&data)?,
            ChatImportFormat::CAITools => import_from_cai_tools(&data)?,
            ChatImportFormat::KoboldLite => import_from_kobold_lite(&data)?,
            ChatImportFormat::RisuAI => import_from_risuai(&data)?,
        };

        // Set the character name
        chat.character_name = character_name.to_string();

        // Generate a file name if not set
        if chat.file_name.is_none() {
            let now = chrono::Utc::now();
            let create_date = humanized_date(now);
            chat.file_name = Some(format!("{} - {}", character_name, create_date));
        }

        // Save the chat
        self.save(&chat).await?;

        Ok(chat)
    }

    async fn export_chat(
        &self,
        character_name: &str,
        file_name: &str,
        target_path: &Path,
        format: ChatExportFormat,
    ) -> Result<(), DomainError> {
        logger::debug(&format!(
            "Exporting chat: {}/{} to {:?}",
            character_name, file_name, target_path
        ));

        // Get the chat
        let chat = self.get_chat(character_name, file_name).await?;

        match format {
            ChatExportFormat::JSONL => {
                // Get the chat file path
                let chat_path = self.get_chat_path(character_name, file_name);

                // Copy the file
                fs::copy(&chat_path, target_path).await.map_err(|e| {
                    logger::error(&format!("Failed to export chat: {}", e));
                    DomainError::InternalError(format!("Failed to export chat: {}", e))
                })?;
            }
            ChatExportFormat::PlainText => {
                // Convert the chat to plain text
                let text = export_to_plain_text(&chat);

                // Write the file
                fs::write(target_path, text).await.map_err(|e| {
                    logger::error(&format!("Failed to write export file: {}", e));
                    DomainError::InternalError(format!("Failed to write export file: {}", e))
                })?;
            }
        }

        Ok(())
    }

    async fn backup_chat(&self, character_name: &str, file_name: &str) -> Result<(), DomainError> {
        self.backup_chat_file(character_name, file_name).await
    }

    async fn clear_cache(&self) -> Result<(), DomainError> {
        let mut cache = self.memory_cache.lock().await;
        cache.clear();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::FileChatRepository;

    #[test]
    fn backup_file_name_uses_windows_safe_timestamp() {
        let backup_file_name = FileChatRepository::backup_file_name("Alice");

        assert!(backup_file_name.starts_with(FileChatRepository::CHAT_BACKUP_PREFIX));
        assert!(backup_file_name.ends_with(".jsonl"));
        assert!(!backup_file_name.contains(':'));

        let stem = backup_file_name
            .strip_suffix(".jsonl")
            .expect("backup file should end with .jsonl");
        let (_chat_key, timestamp) = stem
            .rsplit_once('_')
            .expect("backup file should contain trailing timestamp");

        assert_eq!(timestamp.len(), 15);
        assert_eq!(timestamp.chars().nth(8), Some('-'));
        assert!(timestamp
            .chars()
            .enumerate()
            .all(|(index, ch)| (index == 8 && ch == '-') || ch.is_ascii_digit()));
    }

    #[test]
    fn backup_name_matches_sillytavern_sanitization() {
        let key = FileChatRepository::sanitize_backup_name_for_sillytavern("A:li*ce Name");
        assert_eq!(key, "alice_name");

        let unicode = FileChatRepository::sanitize_backup_name_for_sillytavern("角色-A");
        assert_eq!(unicode, "___a");
    }

    #[test]
    fn backup_name_reserved_windows_name_becomes_empty() {
        let key = FileChatRepository::sanitize_backup_name_for_sillytavern("CON");
        assert_eq!(key, "");
    }

    #[test]
    fn backup_file_prefix_matches_sillytavern_pattern() {
        let prefix = FileChatRepository::backup_file_prefix("A:li*ce Name");
        assert_eq!(prefix, "chat_alice_name_");
    }
}
