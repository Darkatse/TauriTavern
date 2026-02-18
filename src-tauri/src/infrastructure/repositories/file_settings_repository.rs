use async_trait::async_trait;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::fs;
use tokio::sync::Mutex;

use crate::domain::errors::DomainError;
use crate::domain::models::settings::{AppSettings, SettingsSnapshot, UserSettings};
use crate::domain::repositories::settings_repository::SettingsRepository;
use crate::infrastructure::logging::logger;
use crate::infrastructure::persistence::file_system::{
    list_files_with_extension, read_json_file, write_json_file,
};

pub struct FileSettingsRepository {
    settings_file: PathBuf,
    settings: Arc<Mutex<Option<AppSettings>>>,
    user_settings_file: PathBuf,
    user_settings: Arc<Mutex<Option<UserSettings>>>,
    base_directory: PathBuf,
}

impl FileSettingsRepository {
    pub fn new(settings_dir: PathBuf) -> Self {
        // 在default-user目录下创建settings.json文件
        let settings_file = settings_dir.join("settings.json");
        let user_settings_file = settings_dir.join("settings.json");
        let base_directory = settings_dir;

        Self {
            settings_file,
            settings: Arc::new(Mutex::new(None)),
            user_settings_file,
            user_settings: Arc::new(Mutex::new(None)),
            base_directory,
        }
    }

    async fn ensure_directory_exists(&self) -> Result<(), DomainError> {
        if let Some(parent) = self.settings_file.parent() {
            if !parent.exists() {
                tracing::info!("Creating settings directory: {:?}", parent);
                fs::create_dir_all(parent).await.map_err(|e| {
                    tracing::error!("Failed to create settings directory: {}", e);
                    DomainError::InternalError(format!(
                        "Failed to create settings directory: {}",
                        e
                    ))
                })?;
            }
        }
        Ok(())
    }

    /// 确保快照目录存在
    async fn ensure_snapshots_directory_exists(&self) -> Result<PathBuf, DomainError> {
        let snapshots_dir = self.base_directory.join("snapshots");

        if !snapshots_dir.exists() {
            tracing::info!("Creating snapshots directory: {:?}", snapshots_dir);
            fs::create_dir_all(&snapshots_dir).await.map_err(|e| {
                tracing::error!("Failed to create snapshots directory: {}", e);
                DomainError::InternalError(format!("Failed to create snapshots directory: {}", e))
            })?;
        }

        Ok(snapshots_dir)
    }

    /// 获取当前时间戳（毫秒）
    fn get_timestamp_ms(&self) -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64
    }

    /// 从目录中读取 JSON 文件列表
    async fn read_json_files_from_directory(
        &self,
        dir: &Path,
    ) -> Result<Vec<UserSettings>, DomainError> {
        let mut result = Vec::new();

        if !dir.exists() {
            return Ok(result);
        }

        let mut entries = fs::read_dir(dir).await.map_err(|e| {
            DomainError::InternalError(format!("Failed to read directory {}: {}", dir.display(), e))
        })?;

        let mut entries_vec = Vec::new();

        // 收集所有条目
        while let Some(entry) = entries.next_entry().await.map_err(|e| {
            DomainError::InternalError(format!("Failed to read directory entry: {}", e))
        })? {
            entries_vec.push(entry);
        }

        // 处理每个条目
        for entry in entries_vec {
            let path = entry.path();

            if path.is_file() && path.extension().map_or(false, |ext| ext == "json") {
                match read_json_file::<UserSettings>(&path).await {
                    Ok(settings) => {
                        result.push(settings);
                    }
                    Err(e) => {
                        logger::warn(&format!(
                            "Failed to read settings file {}: {}",
                            path.display(),
                            e
                        ));
                        // 继续处理其他文件
                    }
                }
            }
        }

        Ok(result)
    }

    /// 从目录中读取预设文件
    async fn read_presets_from_directory(
        &self,
        dir_name: &str,
    ) -> Result<Vec<UserSettings>, DomainError> {
        let dir = self.base_directory.join(dir_name);
        self.read_json_files_from_directory(&dir).await
    }

    /// 从目录中读取 AI 设置
    async fn read_ai_settings(
        &self,
        dir_name: &str,
    ) -> Result<(Vec<String>, Vec<String>), DomainError> {
        let dir = self.base_directory.join(dir_name);

        if !dir.exists() {
            return Ok((Vec::new(), Vec::new()));
        }

        let mut files = Vec::new();
        files.extend(list_files_with_extension(&dir, "json").await?);
        files.sort();

        let mut settings = Vec::new();
        let mut names = Vec::new();
        let mut seen_names = HashSet::new();

        for file in files {
            let file_name = file
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default();

            if !seen_names.insert(file_name.to_string()) {
                continue;
            }

            let content = fs::read_to_string(&file).await.map_err(|e| {
                DomainError::InternalError(format!("Failed to read file {}: {}", file.display(), e))
            })?;

            settings.push(content);
            names.push(file_name.to_string());
        }

        Ok((settings, names))
    }

    #[cfg(any(target_os = "android", target_os = "ios"))]
    fn enforce_mobile_theme_chat_width(theme: &mut UserSettings) {
        if let Some(theme_obj) = theme.data.as_object_mut() {
            theme_obj.insert("chat_width".to_string(), serde_json::Value::from(100));
        }
    }

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    fn enforce_mobile_theme_chat_width(_theme: &mut UserSettings) {}
}

#[async_trait]
impl SettingsRepository for FileSettingsRepository {
    async fn save(&self, settings: &AppSettings) -> Result<(), DomainError> {
        self.ensure_directory_exists().await?;

        write_json_file(&self.settings_file, settings).await?;

        // Update cache
        let mut cached_settings = self.settings.lock().await;
        *cached_settings = Some(settings.clone());

        Ok(())
    }

    async fn load(&self) -> Result<AppSettings, DomainError> {
        // Try to get from cache first
        {
            let cached_settings = self.settings.lock().await;
            if let Some(settings) = cached_settings.clone() {
                return Ok(settings);
            }
        }

        // If not in cache, load from file
        if !self.settings_file.exists() {
            // If settings file doesn't exist, create default settings
            let default_settings = AppSettings::default();
            self.save(&default_settings).await?;
            return Ok(default_settings);
        }

        let settings = read_json_file::<AppSettings>(&self.settings_file).await?;

        // Update cache
        let mut cached_settings = self.settings.lock().await;
        *cached_settings = Some(settings.clone());

        Ok(settings)
    }

    // 用户设置

    async fn save_user_settings(&self, settings: &UserSettings) -> Result<(), DomainError> {
        self.ensure_directory_exists().await?;

        tracing::info!(
            "Saving user settings to {}",
            self.user_settings_file.display()
        );
        write_json_file(&self.user_settings_file, settings).await?;

        // Update cache
        let mut cached_settings = self.user_settings.lock().await;
        *cached_settings = Some(settings.clone());

        Ok(())
    }

    async fn load_user_settings(&self) -> Result<UserSettings, DomainError> {
        // Try to get from cache first
        {
            let cached_settings = self.user_settings.lock().await;
            if let Some(settings) = cached_settings.clone() {
                return Ok(settings);
            }
        }

        // If not in cache, load from file
        if !self.user_settings_file.exists() {
            // If settings file doesn't exist, create default settings
            let default_settings = UserSettings::default();
            self.save_user_settings(&default_settings).await?;
            return Ok(default_settings);
        }

        tracing::info!(
            "Loading user settings from {}",
            self.user_settings_file.display()
        );
        let settings = read_json_file::<UserSettings>(&self.user_settings_file).await?;

        // Update cache
        let mut cached_settings = self.user_settings.lock().await;
        *cached_settings = Some(settings.clone());

        Ok(settings)
    }

    // 设置快照

    async fn create_snapshot(&self) -> Result<(), DomainError> {
        // 确保快照目录存在
        let snapshots_dir = self.ensure_snapshots_directory_exists().await?;

        // 加载当前设置
        let settings = self.load_user_settings().await?;

        // 创建快照文件名
        let timestamp = self.get_timestamp_ms();
        let snapshot_file = snapshots_dir.join(format!("settings_{}.json", timestamp));

        // 保存快照
        tracing::info!("Creating settings snapshot: {}", snapshot_file.display());
        write_json_file(&snapshot_file, &settings).await?;

        Ok(())
    }

    async fn get_snapshots(&self) -> Result<Vec<SettingsSnapshot>, DomainError> {
        // 确保快照目录存在
        let snapshots_dir = self.ensure_snapshots_directory_exists().await?;

        let mut snapshots = Vec::new();

        // 读取快照目录
        let mut entries = fs::read_dir(&snapshots_dir).await.map_err(|e| {
            DomainError::InternalError(format!("Failed to read snapshots directory: {}", e))
        })?;

        // 处理每个条目
        while let Some(entry) = entries.next_entry().await.map_err(|e| {
            DomainError::InternalError(format!("Failed to read directory entry: {}", e))
        })? {
            let path = entry.path();

            if path.is_file() && path.extension().map_or(false, |ext| ext == "json") {
                let file_name = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or_default();

                // 解析时间戳
                if let Some(timestamp_str) = file_name.strip_prefix("settings_") {
                    if let Ok(timestamp) = timestamp_str.parse::<i64>() {
                        // 获取文件大小
                        let metadata = fs::metadata(&path).await.map_err(|e| {
                            DomainError::InternalError(format!(
                                "Failed to get file metadata: {}",
                                e
                            ))
                        })?;

                        snapshots.push(SettingsSnapshot {
                            date: timestamp,
                            name: file_name.to_string(),
                            size: metadata.len(),
                        });
                    }
                }
            }
        }

        // 按时间戳排序（降序）
        snapshots.sort_by(|a, b| b.date.cmp(&a.date));

        Ok(snapshots)
    }

    async fn load_snapshot(&self, name: &str) -> Result<UserSettings, DomainError> {
        // 确保快照目录存在
        let snapshots_dir = self.ensure_snapshots_directory_exists().await?;

        // 构建快照文件路径
        let snapshot_file = snapshots_dir.join(format!("{}.json", name));

        // 检查文件是否存在
        if !snapshot_file.exists() {
            return Err(DomainError::NotFound(format!(
                "Snapshot {} not found",
                name
            )));
        }

        // 读取快照文件
        tracing::info!("Loading settings snapshot: {}", snapshot_file.display());
        let settings = read_json_file::<UserSettings>(&snapshot_file).await?;

        Ok(settings)
    }

    async fn restore_snapshot(&self, name: &str) -> Result<(), DomainError> {
        // 加载快照
        let settings = self.load_snapshot(name).await?;

        // 保存为当前设置
        self.save_user_settings(&settings).await?;

        Ok(())
    }

    // 预设和主题

    async fn get_themes(&self) -> Result<Vec<UserSettings>, DomainError> {
        let mut themes = self.read_presets_from_directory("themes").await?;

        for theme in &mut themes {
            Self::enforce_mobile_theme_chat_width(theme);
        }

        Ok(themes)
    }

    async fn get_moving_ui_presets(&self) -> Result<Vec<UserSettings>, DomainError> {
        self.read_presets_from_directory("movingUI").await
    }

    async fn get_quick_reply_presets(&self) -> Result<Vec<UserSettings>, DomainError> {
        self.read_presets_from_directory("QuickReplies").await
    }

    async fn get_instruct_presets(&self) -> Result<Vec<UserSettings>, DomainError> {
        self.read_presets_from_directory("instruct").await
    }

    async fn get_context_presets(&self) -> Result<Vec<UserSettings>, DomainError> {
        self.read_presets_from_directory("context").await
    }

    async fn get_sysprompt_presets(&self) -> Result<Vec<UserSettings>, DomainError> {
        self.read_presets_from_directory("sysprompt").await
    }

    async fn get_reasoning_presets(&self) -> Result<Vec<UserSettings>, DomainError> {
        self.read_presets_from_directory("reasoning").await
    }

    // AI 设置

    async fn get_koboldai_settings(&self) -> Result<(Vec<String>, Vec<String>), DomainError> {
        self.read_ai_settings("KoboldAI Settings").await
    }

    async fn get_novelai_settings(&self) -> Result<(Vec<String>, Vec<String>), DomainError> {
        self.read_ai_settings("NovelAI Settings").await
    }

    async fn get_openai_settings(&self) -> Result<(Vec<String>, Vec<String>), DomainError> {
        self.read_ai_settings("OpenAI Settings").await
    }

    async fn get_textgen_settings(&self) -> Result<(Vec<String>, Vec<String>), DomainError> {
        self.read_ai_settings("TextGen Settings").await
    }

    // 世界信息

    async fn get_world_names(&self) -> Result<Vec<String>, DomainError> {
        let worlds_dir = self.base_directory.join("worlds");

        if !worlds_dir.exists() {
            return Ok(Vec::new());
        }

        let mut world_names = list_files_with_extension(&worlds_dir, "json")
            .await?
            .into_iter()
            .filter_map(|path| {
                path.file_stem()
                    .and_then(|stem| stem.to_str())
                    .map(|name| name.to_string())
            })
            .collect::<Vec<_>>();

        world_names.sort();

        Ok(world_names)
    }
}
