use async_trait::async_trait;
use chrono::{DateTime, Datelike, Timelike, Utc};
use serde_json::{Number, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::fs;
use tokio::sync::Mutex;

use crate::domain::errors::DomainError;
use crate::domain::models::group::Group;
use crate::domain::repositories::group_repository::GroupRepository;
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
            tracing::debug!("Initializing group cache");

            // Ensure directories exist
            if !self.groups_dir.exists() {
                fs::create_dir_all(&self.groups_dir).await.map_err(|e| {
                    tracing::error!("Failed to create groups directory: {}", e);
                    DomainError::InternalError(format!("Failed to create groups directory: {}", e))
                })?;
            }

            if !self.group_chats_dir.exists() {
                fs::create_dir_all(&self.group_chats_dir)
                    .await
                    .map_err(|e| {
                        tracing::error!("Failed to create group chats directory: {}", e);
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
                        tracing::error!(
                            target: crate::observability_targets::USER_VISIBLE_ERROR,
                            "Failed to load group from {:?}: {}",
                            file_path,
                            e
                        );
                    }
                }
            }

            *initialized = true;
            tracing::debug!("Group cache initialized with {} groups", cache.len());
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
        let mut group = read_group_manifest_compat(file_path).await?;

        // Get file stats for metadata
        let metadata = fs::metadata(file_path).await.map_err(|e| {
            tracing::error!("Failed to get metadata for {:?}: {}", file_path, e);
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

async fn read_group_manifest_compat(file_path: &Path) -> Result<Group, DomainError> {
    let value: Value = read_json_file(file_path).await?;
    decode_group_manifest_compat(value, file_path)
}

fn decode_group_manifest_compat(mut value: Value, file_path: &Path) -> Result<Group, DomainError> {
    normalize_group_manifest_compat(&mut value, file_path)?;

    serde_json::from_value(value).map_err(|error| {
        tracing::error!(
            "Failed to decode group JSON from file {:?}: {}",
            file_path,
            error
        );
        DomainError::InvalidData(format!("Invalid group JSON: {}", error))
    })
}

fn normalize_group_manifest_compat(value: &mut Value, file_path: &Path) -> Result<(), DomainError> {
    let object = value.as_object_mut().ok_or_else(|| {
        DomainError::InvalidData(format!(
            "Group JSON root must be an object in '{}'",
            file_path.display()
        ))
    })?;

    normalize_activation_strategy_field(object, file_path)?;
    normalize_optional_timestamp_field(object, "date_added", file_path)?;
    normalize_optional_timestamp_field(object, "date_last_chat", file_path)?;

    Ok(())
}

fn normalize_activation_strategy_field(
    object: &mut serde_json::Map<String, Value>,
    file_path: &Path,
) -> Result<(), DomainError> {
    let Some(value) = object.get("activation_strategy").cloned() else {
        return Ok(());
    };

    match value {
        Value::Null => {
            warn_group_manifest_normalization(file_path, "activation_strategy", "null", "0");
            object.insert(
                "activation_strategy".to_string(),
                Value::Number(Number::from(0)),
            );
            Ok(())
        }
        Value::Bool(value) => {
            let normalized = if value { 1 } else { 0 };
            warn_group_manifest_normalization(
                file_path,
                "activation_strategy",
                if value { "true" } else { "false" },
                &normalized.to_string(),
            );
            object.insert(
                "activation_strategy".to_string(),
                Value::Number(Number::from(normalized)),
            );
            Ok(())
        }
        Value::Number(number) => {
            let value = number.as_i64().ok_or_else(|| {
                invalid_group_manifest_field(
                    file_path,
                    "activation_strategy",
                    "an i32 integer or legacy boolean",
                    &Value::Number(number.clone()),
                )
            })?;
            i32::try_from(value).map_err(|_| {
                DomainError::InvalidData(format!(
                    "Invalid group JSON in '{}': field 'activation_strategy' value {} is outside i32 range",
                    file_path.display(),
                    value
                ))
            })?;
            Ok(())
        }
        other => Err(invalid_group_manifest_field(
            file_path,
            "activation_strategy",
            "an i32 integer or legacy boolean",
            &other,
        )),
    }
}

fn normalize_optional_timestamp_field(
    object: &mut serde_json::Map<String, Value>,
    field: &'static str,
    file_path: &Path,
) -> Result<(), DomainError> {
    let Some(value) = object.get(field).cloned() else {
        return Ok(());
    };

    match value {
        Value::Null => Ok(()),
        Value::Number(number) if number.as_i64().is_some() => Ok(()),
        Value::Number(number) if number.as_u64().is_some() => {
            Err(DomainError::InvalidData(format!(
                "Invalid group JSON in '{}': field '{}' is outside i64 range",
                file_path.display(),
                field
            )))
        }
        Value::Number(number) => {
            let value = number.as_f64().ok_or_else(|| {
                invalid_group_manifest_field(
                    file_path,
                    field,
                    "an i64 timestamp, floating-point timestamp, or null",
                    &Value::Number(number.clone()),
                )
            })?;
            let normalized = truncate_timestamp_float(value, field, file_path)?;
            warn_group_manifest_normalization(
                file_path,
                field,
                &number.to_string(),
                &normalized.to_string(),
            );
            object.insert(field.to_string(), Value::Number(Number::from(normalized)));
            Ok(())
        }
        other => Err(invalid_group_manifest_field(
            file_path,
            field,
            "an i64 timestamp, floating-point timestamp, or null",
            &other,
        )),
    }
}

fn truncate_timestamp_float(value: f64, field: &str, file_path: &Path) -> Result<i64, DomainError> {
    let truncated = value.trunc();
    if !truncated.is_finite() || truncated < i64::MIN as f64 || truncated > i64::MAX as f64 {
        return Err(DomainError::InvalidData(format!(
            "Invalid group JSON in '{}': field '{}' value {} is outside i64 range",
            file_path.display(),
            field,
            value
        )));
    }

    Ok(truncated as i64)
}

fn warn_group_manifest_normalization(
    file_path: &Path,
    field: &str,
    original: &str,
    normalized: &str,
) {
    tracing::warn!(
        "Normalizing legacy group JSON field '{}' in {:?}: {} -> {}",
        field,
        file_path,
        original,
        normalized
    );
}

fn invalid_group_manifest_field(
    file_path: &Path,
    field: &str,
    expected: &str,
    actual: &Value,
) -> DomainError {
    DomainError::InvalidData(format!(
        "Invalid group JSON in '{}': field '{}' expected {}, got {}",
        file_path.display(),
        field,
        expected,
        json_value_type_name(actual)
    ))
}

fn json_value_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(number) if number.is_i64() || number.is_u64() => "integer",
        Value::Number(_) => "floating-point number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
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
        let mut group_to_write = group.clone();
        group_to_write.date_added = None;
        group_to_write.create_date = None;
        group_to_write.chat_size = None;
        group_to_write.date_last_chat = None;
        write_json_file(&file_path, &group_to_write).await?;

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

        let mut group_to_write = group.clone();
        group_to_write.date_added = None;
        group_to_write.create_date = None;
        group_to_write.chat_size = None;
        group_to_write.date_last_chat = None;
        write_json_file(&file_path, &group_to_write).await?;

        // Update cache with preserved metadata
        let mut cache = self.cache.lock().await;
        let mut updated_group = group.clone();

        // Preserve metadata from cache if available
        if let Some(cached_group) = cache.get(&group.id) {
            updated_group.date_added = cached_group.date_added;
            updated_group.create_date = cached_group.create_date.clone();
            updated_group.chat_size = cached_group.chat_size;
            updated_group.date_last_chat = cached_group.date_last_chat;
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
            tracing::error!("Failed to delete group file {:?}: {}", file_path, e);
            DomainError::InternalError(format!("Failed to delete group file: {}", e))
        })?;

        // Delete associated chat files
        if let Some(group) = group {
            for chat_id in group.chats {
                let chat_file_path = self.group_chats_dir.join(format!("{}.jsonl", chat_id));
                if chat_file_path.exists() {
                    fs::remove_file(&chat_file_path).await.map_err(|e| {
                        tracing::error!(
                            "Failed to delete group chat file {:?}: {}",
                            chat_file_path,
                            e
                        );
                        DomainError::InternalError(format!(
                            "Failed to delete group chat file: {}",
                            e
                        ))
                    })?;
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

        tracing::debug!("Group cache cleared");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use uuid::Uuid;

    fn base_group_manifest() -> Value {
        json!({
            "id": "legacy-group",
            "name": "Legacy Group",
            "members": [],
            "disabled_members": [],
            "chat_id": "legacy-chat",
            "chats": ["legacy-chat"]
        })
    }

    fn decode_test_group(value: Value) -> Result<Group, DomainError> {
        decode_group_manifest_compat(value, Path::new("legacy-group.json"))
    }

    #[test]
    fn group_manifest_compat_accepts_legacy_activation_strategy_bool() {
        let mut false_manifest = base_group_manifest();
        false_manifest["activation_strategy"] = json!(false);
        let group = decode_test_group(false_manifest).expect("decode false activation strategy");
        assert_eq!(group.activation_strategy, 0);

        let mut true_manifest = base_group_manifest();
        true_manifest["activation_strategy"] = json!(true);
        let group = decode_test_group(true_manifest).expect("decode true activation strategy");
        assert_eq!(group.activation_strategy, 1);
    }

    #[test]
    fn group_manifest_compat_truncates_legacy_float_timestamps() {
        let mut manifest = base_group_manifest();
        manifest["date_added"] = json!(1_752_578_685_443.4028_f64);
        manifest["date_last_chat"] = json!(1_752_578_685_999.9875_f64);

        let group = decode_test_group(manifest).expect("decode legacy float timestamps");

        assert_eq!(group.date_added, Some(1_752_578_685_443));
        assert_eq!(group.date_last_chat, Some(1_752_578_685_999));
    }

    #[test]
    fn group_manifest_compat_preserves_unknown_fields() {
        let mut manifest = base_group_manifest();
        manifest["vendor_extra"] = json!({ "kept": true });

        let group = decode_test_group(manifest).expect("decode group with unknown field");

        assert_eq!(
            group.additional.get("vendor_extra"),
            Some(&json!({ "kept": true }))
        );
    }

    #[test]
    fn group_manifest_compat_rejects_unrelated_dirty_field() {
        let mut manifest = base_group_manifest();
        manifest["auto_mode_delay"] = json!(false);

        let error = decode_test_group(manifest).expect_err("dirty unrelated field must fail");

        assert!(error.to_string().contains("expected i32"));
    }

    #[test]
    fn group_manifest_compat_rejects_string_activation_strategy() {
        let mut manifest = base_group_manifest();
        manifest["activation_strategy"] = json!("0");

        let error = decode_test_group(manifest).expect_err("string activation strategy must fail");

        assert!(error.to_string().contains("activation_strategy"));
    }

    #[tokio::test]
    async fn group_repository_loads_and_rewrites_legacy_group_manifest() {
        let root = std::env::temp_dir().join(format!(
            "tauritavern-file-group-repository-{}",
            Uuid::new_v4()
        ));
        let groups_dir = root.join("groups");
        let group_chats_dir = root.join("group chats");
        let group_path = groups_dir.join("legacy-group.json");

        tokio::fs::create_dir_all(&groups_dir)
            .await
            .expect("create groups dir");
        tokio::fs::create_dir_all(&group_chats_dir)
            .await
            .expect("create group chats dir");

        let mut manifest = base_group_manifest();
        manifest["activation_strategy"] = json!(false);
        manifest["date_added"] = json!(1_752_578_685_443.4028_f64);
        manifest["date_last_chat"] = json!(1_752_578_685_999.9875_f64);
        manifest["vendor_extra"] = json!("kept");
        tokio::fs::write(
            &group_path,
            serde_json::to_string_pretty(&manifest).expect("serialize manifest"),
        )
        .await
        .expect("write manifest");

        let repository = FileGroupRepository::new(groups_dir, group_chats_dir);
        let groups = repository.get_all_groups().await.expect("load groups");
        assert_eq!(groups.len(), 1);

        let mut group = groups.into_iter().next().expect("loaded group");
        assert_eq!(group.activation_strategy, 0);
        assert_eq!(group.additional.get("vendor_extra"), Some(&json!("kept")));

        group.name = "Renamed Group".to_string();
        repository
            .update_group(&group)
            .await
            .expect("rewrite group manifest");

        let persisted_raw = tokio::fs::read_to_string(&group_path)
            .await
            .expect("read persisted manifest");
        let persisted: Value =
            serde_json::from_str(&persisted_raw).expect("parse persisted manifest");

        assert_eq!(persisted["activation_strategy"], json!(0));
        assert_eq!(persisted["vendor_extra"], json!("kept"));
        assert!(persisted.get("date_added").is_none());
        assert!(persisted.get("date_last_chat").is_none());

        let _ = tokio::fs::remove_dir_all(root).await;
    }
}
