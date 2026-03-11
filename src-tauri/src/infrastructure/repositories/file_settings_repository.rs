use async_trait::async_trait;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::fs;

use crate::domain::errors::DomainError;
use crate::domain::models::settings::{SettingsSnapshot, TauriTavernSettings, UserSettings};
use crate::domain::repositories::settings_repository::SettingsRepository;
use crate::infrastructure::logging::logger;
use crate::infrastructure::persistence::file_system::{
    list_files_with_extension, read_json_file, write_json_file,
};

pub struct FileSettingsRepository {
    tauritavern_settings_file: PathBuf,
    user_settings_file: PathBuf,
    base_directory: PathBuf,
}

impl FileSettingsRepository {
    pub fn new(settings_dir: PathBuf) -> Self {
        let tauritavern_settings_file = settings_dir.join("tauritavern-settings.json");
        let user_settings_file = settings_dir.join("settings.json");
        let base_directory = settings_dir;

        Self {
            tauritavern_settings_file,
            user_settings_file,
            base_directory,
        }
    }

    async fn ensure_directory_exists(&self) -> Result<(), DomainError> {
        if let Some(parent) = self.tauritavern_settings_file.parent() {
            if !parent.exists() {
                tracing::debug!("Creating settings directory: {:?}", parent);
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

    fn get_timestamp_ms(&self) -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64
    }

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
        while let Some(entry) = entries.next_entry().await.map_err(|e| {
            DomainError::InternalError(format!("Failed to read directory entry: {}", e))
        })? {
            entries_vec.push(entry);
        }

        for entry in entries_vec {
            let path = entry.path();

            if path.is_file() && path.extension().is_some_and(|ext| ext == "json") {
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
                    }
                }
            }
        }

        Ok(result)
    }

    async fn read_presets_from_directory(
        &self,
        dir_name: &str,
    ) -> Result<Vec<UserSettings>, DomainError> {
        let dir = self.base_directory.join(dir_name);
        self.read_json_files_from_directory(&dir).await
    }

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
    async fn save_tauritavern_settings(
        &self,
        settings: &TauriTavernSettings,
    ) -> Result<(), DomainError> {
        self.ensure_directory_exists().await?;

        write_json_file(&self.tauritavern_settings_file, settings).await?;
        Ok(())
    }

    async fn load_tauritavern_settings(&self) -> Result<TauriTavernSettings, DomainError> {
        if !self.tauritavern_settings_file.exists() {
            let default_settings = TauriTavernSettings::default();
            self.save_tauritavern_settings(&default_settings).await?;
            return Ok(default_settings);
        }

        read_json_file::<TauriTavernSettings>(&self.tauritavern_settings_file).await
    }

    async fn save_user_settings(&self, settings: &UserSettings) -> Result<(), DomainError> {
        self.ensure_directory_exists().await?;

        tracing::info!(
            "Saving user settings to {}",
            self.user_settings_file.display()
        );
        write_json_file(&self.user_settings_file, settings).await?;
        Ok(())
    }

    async fn load_user_settings(&self) -> Result<UserSettings, DomainError> {
        if !self.user_settings_file.exists() {
            let default_settings = UserSettings::default();
            self.save_user_settings(&default_settings).await?;
            return Ok(default_settings);
        }

        tracing::info!(
            "Loading user settings from {}",
            self.user_settings_file.display()
        );
        read_json_file::<UserSettings>(&self.user_settings_file).await
    }

    async fn create_snapshot(&self) -> Result<(), DomainError> {
        let snapshots_dir = self.ensure_snapshots_directory_exists().await?;
        let settings = self.load_user_settings().await?;
        let timestamp = self.get_timestamp_ms();
        let snapshot_file = snapshots_dir.join(format!("settings_{}.json", timestamp));

        tracing::info!("Creating settings snapshot: {}", snapshot_file.display());
        write_json_file(&snapshot_file, &settings).await?;

        Ok(())
    }

    async fn get_snapshots(&self) -> Result<Vec<SettingsSnapshot>, DomainError> {
        let snapshots_dir = self.ensure_snapshots_directory_exists().await?;

        let mut snapshots = Vec::new();
        let mut entries = fs::read_dir(&snapshots_dir).await.map_err(|e| {
            DomainError::InternalError(format!("Failed to read snapshots directory: {}", e))
        })?;

        while let Some(entry) = entries.next_entry().await.map_err(|e| {
            DomainError::InternalError(format!("Failed to read directory entry: {}", e))
        })? {
            let path = entry.path();

            if path.is_file() && path.extension().is_some_and(|ext| ext == "json") {
                let file_name = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or_default();

                if let Some(timestamp_str) = file_name.strip_prefix("settings_") {
                    if let Ok(timestamp) = timestamp_str.parse::<i64>() {
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

        snapshots.sort_by(|a, b| b.date.cmp(&a.date));

        Ok(snapshots)
    }

    async fn load_snapshot(&self, name: &str) -> Result<UserSettings, DomainError> {
        let snapshots_dir = self.ensure_snapshots_directory_exists().await?;
        let snapshot_file = snapshots_dir.join(format!("{}.json", name));

        if !snapshot_file.exists() {
            return Err(DomainError::NotFound(format!(
                "Snapshot {} not found",
                name
            )));
        }

        tracing::info!("Loading settings snapshot: {}", snapshot_file.display());
        let settings = read_json_file::<UserSettings>(&snapshot_file).await?;

        Ok(settings)
    }

    async fn restore_snapshot(&self, name: &str) -> Result<(), DomainError> {
        let settings = self.load_snapshot(name).await?;
        self.save_user_settings(&settings).await?;

        Ok(())
    }

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

#[cfg(test)]
mod tests {
    use super::FileSettingsRepository;
    use crate::domain::repositories::settings_repository::SettingsRepository;
    use serde_json::json;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new() -> Self {
            let suffix = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time should be after unix epoch")
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "tauritavern-settings-repo-test-{}-{}",
                std::process::id(),
                suffix
            ));
            fs::create_dir_all(&path).expect("failed to create temp dir");

            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[tokio::test]
    async fn load_user_settings_reads_disk_each_time() {
        let dir = TestDir::new();
        let repository = FileSettingsRepository::new(dir.path().to_path_buf());

        let first = repository
            .load_user_settings()
            .await
            .expect("load default user settings");
        assert_eq!(first.data, json!({}));

        fs::write(dir.path().join("settings.json"), r#"{"hello":"world"}"#)
            .expect("write external settings.json");

        let second = repository
            .load_user_settings()
            .await
            .expect("load externally updated user settings");
        assert_eq!(second.data, json!({"hello":"world"}));
    }

    #[tokio::test]
    async fn load_tauritavern_settings_reads_disk_each_time() {
        let dir = TestDir::new();
        let repository = FileSettingsRepository::new(dir.path().to_path_buf());

        let _ = repository
            .load_tauritavern_settings()
            .await
            .expect("load default tauritavern settings");

        fs::write(
            dir.path().join("tauritavern-settings.json"),
            r#"{"updates":{"startup_popup":{"dismissed_release_token":"token"}}}"#,
        )
        .expect("write external tauritavern-settings.json");

        let second = repository
            .load_tauritavern_settings()
            .await
            .expect("load externally updated tauritavern settings");
        assert_eq!(
            second.updates.startup_popup.dismissed_release_token.as_deref(),
            Some("token")
        );
    }
}
