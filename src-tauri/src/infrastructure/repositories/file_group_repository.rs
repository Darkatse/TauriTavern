use async_trait::async_trait;
use chrono::{DateTime, Datelike, Timelike, Utc};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::fs;
use tokio::sync::Mutex;

use crate::domain::errors::DomainError;
use crate::domain::models::group::Group;
use crate::domain::repositories::group_repository::GroupRepository;
use crate::infrastructure::logging::logger;
use crate::infrastructure::persistence::file_system::{
    list_files_with_extension, read_json_file, write_json_file,
};

/// File-based implementation of the GroupRepository
pub struct FileGroupRepository {
    /// Directory where group files are stored
    groups_dir: PathBuf,

    /// Directory where group chat files are stored
    group_chats_dir: PathBuf,

    /// Cache for groups to improve performance
    cache: Arc<Mutex<HashMap<String, Group>>>,

    /// Flag to indicate if the cache is initialized
    cache_initialized: Arc<Mutex<bool>>,
}

impl FileGroupRepository {
    /// Create a new FileGroupRepository
    pub fn new(groups_dir: PathBuf, group_chats_dir: PathBuf) -> Self {
        Self {
            groups_dir,
            group_chats_dir,
            cache: Arc::new(Mutex::new(HashMap::new())),
            cache_initialized: Arc::new(Mutex::new(false)),
        }
    }

    /// Format a timestamp as a human-readable date string
    fn format_timestamp(&self, timestamp: i64) -> String {
        let dt = DateTime::<Utc>::from_timestamp(timestamp / 1000, 0).unwrap_or_else(Utc::now);
        format!(
            "{}-{}-{} @{}h {}m {}s {}ms",
            dt.year(),
            dt.month(),
            dt.day(),
            dt.hour(),
            dt.minute(),
            dt.second(),
            dt.timestamp_subsec_millis()
        )
    }

    /// Get the file path for a group
    fn get_group_file_path(&self, id: &str) -> PathBuf {
        self.groups_dir.join(format!("{}.json", id))
    }

    /// Initialize the cache with all groups
    async fn initialize_cache_if_needed(&self) -> Result<(), DomainError> {
        let mut initialized = self.cache_initialized.lock().await;
        if !*initialized {
            logger::debug("Initializing group cache");

            // Ensure directories exist
            if !self.groups_dir.exists() {
                fs::create_dir_all(&self.groups_dir).await.map_err(|e| {
                    logger::error(&format!("Failed to create groups directory: {}", e));
                    DomainError::InternalError(format!("Failed to create groups directory: {}", e))
                })?;
            }

            if !self.group_chats_dir.exists() {
                fs::create_dir_all(&self.group_chats_dir)
                    .await
                    .map_err(|e| {
                        logger::error(&format!("Failed to create group chats directory: {}", e));
                        DomainError::InternalError(format!(
                            "Failed to create group chats directory: {}",
                            e
                        ))
                    })?;
            }

            // Load all groups into cache
            let group_files = list_files_with_extension(&self.groups_dir, "json").await?;
            let chat_files = list_files_with_extension(&self.group_chats_dir, "jsonl").await?;

            let mut cache = self.cache.lock().await;

            for file_path in group_files {
                match self.load_group_with_metadata(&file_path, &chat_files).await {
                    Ok(group) => {
                        cache.insert(group.id.clone(), group);
                    }
                    Err(e) => {
                        logger::error(&format!("Failed to load group from {:?}: {}", file_path, e));
                    }
                }
            }

            *initialized = true;
            logger::debug(&format!(
                "Group cache initialized with {} groups",
                cache.len()
            ));
        }

        Ok(())
    }

    /// Load a group from a file and add metadata
    async fn load_group_with_metadata(
        &self,
        file_path: &Path,
        chat_files: &[PathBuf],
    ) -> Result<Group, DomainError> {
        // Read the group file
        let mut group: Group = read_json_file(file_path).await?;

        // Get file stats for metadata
        let metadata = fs::metadata(file_path).await.map_err(|e| {
            logger::error(&format!(
                "Failed to get metadata for {:?}: {}",
                file_path, e
            ));
            DomainError::InternalError(format!("Failed to get file metadata: {}", e))
        })?;

        // Set creation time
        if let Ok(created) = metadata.created() {
            if let Ok(timestamp) = created.duration_since(UNIX_EPOCH) {
                let timestamp_millis = timestamp.as_millis() as i64;
                group.date_added = Some(timestamp_millis);
                group.create_date = Some(self.format_timestamp(timestamp_millis));
            }
        }

        // Calculate chat size and last chat date
        let mut chat_size: u64 = 0;
        let mut date_last_chat: i64 = 0;

        // 直接使用 group.chats，因为它是 Vec<String> 而不是 Option<Vec<String>>
        for chat_file in chat_files {
            let file_name = chat_file.file_stem().and_then(|s| s.to_str()).unwrap_or("");

            if group.chats.contains(&file_name.to_string()) {
                if let Ok(chat_metadata) = fs::metadata(chat_file).await {
                    chat_size += chat_metadata.len();

                    if let Ok(modified) = chat_metadata.modified() {
                        if let Ok(timestamp) = modified.duration_since(UNIX_EPOCH) {
                            let timestamp_millis = timestamp.as_millis() as i64;
                            date_last_chat = date_last_chat.max(timestamp_millis);
                        }
                    }
                }
            }
        }

        group.chat_size = Some(chat_size);
        group.date_last_chat = Some(date_last_chat);

        Ok(group)
    }
}

#[async_trait]
impl GroupRepository for FileGroupRepository {
    async fn get_all_groups(&self) -> Result<Vec<Group>, DomainError> {
        self.initialize_cache_if_needed().await?;

        let cache = self.cache.lock().await;
        let groups: Vec<Group> = cache.values().cloned().collect();

        Ok(groups)
    }

    async fn get_group(&self, id: &str) -> Result<Option<Group>, DomainError> {
        self.initialize_cache_if_needed().await?;

        let cache = self.cache.lock().await;
        let group = cache.get(id).cloned();

        Ok(group)
    }

    async fn create_group(&self, group: &Group) -> Result<Group, DomainError> {
        self.initialize_cache_if_needed().await?;

        let file_path = self.get_group_file_path(&group.id);
        write_json_file(&file_path, group).await?;

        // Update cache
        let mut cache = self.cache.lock().await;

        // Add metadata
        let mut group_with_metadata = group.clone();
        let now = SystemTime::now();
        if let Ok(timestamp) = now.duration_since(UNIX_EPOCH) {
            let timestamp_millis = timestamp.as_millis() as i64;
            group_with_metadata.date_added = Some(timestamp_millis);
            group_with_metadata.create_date = Some(self.format_timestamp(timestamp_millis));
            group_with_metadata.chat_size = Some(0);
            group_with_metadata.date_last_chat = Some(timestamp_millis);
        }

        cache.insert(group.id.clone(), group_with_metadata.clone());

        Ok(group_with_metadata)
    }

    async fn update_group(&self, group: &Group) -> Result<Group, DomainError> {
        self.initialize_cache_if_needed().await?;

        let file_path = self.get_group_file_path(&group.id);

        // Check if the group exists
        if !file_path.exists() {
            return Err(DomainError::NotFound(format!(
                "Group not found: {}",
                group.id
            )));
        }

        write_json_file(&file_path, group).await?;

        // Update cache with preserved metadata
        let mut cache = self.cache.lock().await;
        let mut updated_group = group.clone();

        // Preserve metadata from cache if available
        if let Some(cached_group) = cache.get(&group.id) {
            updated_group.date_added = cached_group.date_added;
            updated_group.create_date = cached_group.create_date.clone();
            updated_group.chat_size = cached_group.chat_size;

            // Update last chat date to now
            let now = SystemTime::now();
            if let Ok(timestamp) = now.duration_since(UNIX_EPOCH) {
                updated_group.date_last_chat = Some(timestamp.as_millis() as i64);
            } else {
                updated_group.date_last_chat = cached_group.date_last_chat;
            }
        }

        cache.insert(group.id.clone(), updated_group.clone());

        Ok(updated_group)
    }

    async fn delete_group(&self, id: &str) -> Result<(), DomainError> {
        self.initialize_cache_if_needed().await?;

        let file_path = self.get_group_file_path(id);

        // Check if the group exists
        if !file_path.exists() {
            return Err(DomainError::NotFound(format!("Group not found: {}", id)));
        }

        // Get the group to find associated chats
        let group = self.get_group(id).await?;

        // Delete the group file
        fs::remove_file(&file_path).await.map_err(|e| {
            logger::error(&format!(
                "Failed to delete group file {:?}: {}",
                file_path, e
            ));
            DomainError::InternalError(format!("Failed to delete group file: {}", e))
        })?;

        // Delete associated chat files
        if let Some(group) = group {
            for chat_id in group.chats {
                let chat_file_path = self.group_chats_dir.join(format!("{}.jsonl", chat_id));
                if chat_file_path.exists() {
                    if let Err(e) = fs::remove_file(&chat_file_path).await {
                        logger::error(&format!(
                            "Failed to delete group chat file {:?}: {}",
                            chat_file_path, e
                        ));
                    }
                }
            }
        }

        // Update cache
        let mut cache = self.cache.lock().await;
        cache.remove(id);

        Ok(())
    }

    async fn get_group_chat_paths(&self) -> Result<Vec<String>, DomainError> {
        let chat_files = list_files_with_extension(&self.group_chats_dir, "jsonl").await?;

        let paths: Vec<String> = chat_files
            .iter()
            .filter_map(|path| {
                path.file_stem()
                    .and_then(|stem| stem.to_str())
                    .map(|s| s.to_string())
            })
            .collect();

        Ok(paths)
    }

    async fn clear_cache(&self) -> Result<(), DomainError> {
        let mut cache = self.cache.lock().await;
        cache.clear();

        let mut initialized = self.cache_initialized.lock().await;
        *initialized = false;

        logger::debug("Group cache cleared");
        Ok(())
    }
}
