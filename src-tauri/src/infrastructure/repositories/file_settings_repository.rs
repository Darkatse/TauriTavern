use async_trait::async_trait;
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
use crate::infrastructure::preset_file_naming::load_named_preset_files;
use crate::infrastructure::sillytavern_sorting::{
    sort_paths_by_file_name_js_default, sort_strings_sillytavern_name,
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
        let mut paths = list_files_with_extension(dir, "json").await?;
        sort_paths_by_file_name_js_default(&mut paths)?;

        for path in paths {
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

        let named_files = load_named_preset_files(&dir).await?;
        let mut settings = Vec::with_capacity(named_files.len());
        let mut names = Vec::with_capacity(named_files.len());

        for file in named_files {
            settings.push(file.raw_content);
            names.push(file.name);
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

        sort_strings_sillytavern_name(&mut world_names);

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
            second
                .updates
                .startup_popup
                .dismissed_release_token
                .as_deref(),
            Some("token")
        );
    }

    #[tokio::test]
    async fn get_openai_settings_uses_embedded_name_from_deprecated_legacy_file() {
        let dir = TestDir::new();
        let repository = FileSettingsRepository::new(dir.path().to_path_buf());
        let openai_dir = dir.path().join("OpenAI Settings");
        fs::create_dir_all(&openai_dir).expect("create OpenAI Settings dir");
        fs::write(
            openai_dir.join("_明月青秋_.json"),
            r#"{"name":"【明月青秋】","temperature":0.7}"#,
        )
        .expect("write legacy preset file");

        let (settings, names) = repository
            .get_openai_settings()
            .await
            .expect("load openai settings");

        assert_eq!(names, vec!["【明月青秋】".to_string()]);
        assert_eq!(settings.len(), 1);
        assert!(settings[0].contains(r#""temperature":0.7"#));
    }

    #[tokio::test]
    async fn get_openai_settings_prefers_canonical_file_over_deprecated_legacy_duplicate() {
        let dir = TestDir::new();
        let repository = FileSettingsRepository::new(dir.path().to_path_buf());
        let openai_dir = dir.path().join("OpenAI Settings");
        fs::create_dir_all(&openai_dir).expect("create OpenAI Settings dir");
        fs::write(
            openai_dir.join("_明月青秋_.json"),
            r#"{"name":"【明月青秋】","temperature":0.1}"#,
        )
        .expect("write legacy preset file");
        fs::write(
            openai_dir.join("【明月青秋】.json"),
            r#"{"name":"【明月青秋】","temperature":0.9}"#,
        )
        .expect("write canonical preset file");

        let (settings, names) = repository
            .get_openai_settings()
            .await
            .expect("load openai settings");

        assert_eq!(names, vec!["【明月青秋】".to_string()]);
        assert_eq!(settings.len(), 1);
        assert!(settings[0].contains(r#""temperature":0.9"#));
    }

    #[tokio::test]
    async fn get_openai_settings_sorts_like_upstream_locale_compare() {
        let dir = TestDir::new();
        let repository = FileSettingsRepository::new(dir.path().to_path_buf());
        let openai_dir = dir.path().join("OpenAI Settings");
        fs::create_dir_all(&openai_dir).expect("create OpenAI Settings dir");
        fs::write(
            openai_dir.join("😀Book.json"),
            r#"{"name":"😀Book","temperature":0.1}"#,
        )
        .expect("write emoji preset");
        fs::write(
            openai_dir.join("Abook.json"),
            r#"{"name":"Abook","temperature":0.2}"#,
        )
        .expect("write latin preset");
        fs::write(
            openai_dir.join("#Book.json"),
            r##"{"name":"#Book","temperature":0.3}"##,
        )
        .expect("write symbol preset");

        let (_settings, names) = repository
            .get_openai_settings()
            .await
            .expect("load openai settings");

        assert_eq!(
            names,
            vec![
                "#Book".to_string(),
                "😀Book".to_string(),
                "Abook".to_string(),
            ]
        );
    }

    #[tokio::test]
    async fn get_world_names_sorts_like_upstream_locale_compare() {
        let dir = TestDir::new();
        let repository = FileSettingsRepository::new(dir.path().to_path_buf());
        let worlds_dir = dir.path().join("worlds");
        fs::create_dir_all(&worlds_dir).expect("create worlds dir");
        fs::write(worlds_dir.join("😀Book.json"), "{}").expect("write emoji world");
        fs::write(worlds_dir.join("Abook.json"), "{}").expect("write latin world");
        fs::write(worlds_dir.join("#Book.json"), "{}").expect("write symbol world");
        fs::write(worlds_dir.join("🧠Lore.json"), "{}").expect("write brain world");
        fs::write(worlds_dir.join("✨Lore.json"), "{}").expect("write sparkles world");
        fs::write(worlds_dir.join("_Book.json"), "{}").expect("write underscore world");
        fs::write(worlds_dir.join("-Book.json"), "{}").expect("write dash world");

        let names = repository
            .get_world_names()
            .await
            .expect("load world names");

        assert_eq!(
            names,
            vec![
                "_Book".to_string(),
                "-Book".to_string(),
                "#Book".to_string(),
                "✨Lore".to_string(),
                "🧠Lore".to_string(),
                "😀Book".to_string(),
                "Abook".to_string(),
            ]
        );
    }

    #[tokio::test]
    async fn get_themes_preserves_upstream_js_default_file_name_order() {
        let dir = TestDir::new();
        let repository = FileSettingsRepository::new(dir.path().to_path_buf());
        let themes_dir = dir.path().join("themes");
        fs::create_dir_all(&themes_dir).expect("create themes dir");
        fs::write(themes_dir.join("😀Theme.json"), r#"{"id":"emoji"}"#).expect("write emoji theme");
        fs::write(themes_dir.join("ATheme.json"), r#"{"id":"latin"}"#).expect("write latin theme");
        fs::write(themes_dir.join("#Theme.json"), r#"{"id":"symbol"}"#)
            .expect("write symbol theme");

        let themes = repository.get_themes().await.expect("load themes");
        let ids: Vec<&str> = themes
            .iter()
            .map(|theme| {
                theme
                    .data
                    .get("id")
                    .and_then(|value| value.as_str())
                    .expect("theme id should be string")
            })
            .collect();

        assert_eq!(ids, vec!["symbol", "latin", "emoji"]);
    }
}
