use async_trait::async_trait;
use std::path::PathBuf;
use tauri::AppHandle;
use tauri::Manager;

use crate::domain::errors::DomainError;
use crate::domain::models::theme::{sanitize_filename, Theme};
use crate::domain::repositories::theme_repository::ThemeRepository;
use crate::infrastructure::logging::logger;
use crate::infrastructure::persistence::file_system::{
    delete_file, list_files_with_extension, read_json_file, write_json_file,
};

/// File-based implementation of the ThemeRepository
pub struct FileThemeRepository {
    /// The directory where themes are stored
    themes_dir: PathBuf,
}

impl FileThemeRepository {
    /// Create a new FileThemeRepository
    pub fn new(app_handle: AppHandle) -> Self {
        // Get the themes directory from the app data directory
        let app_data_dir = app_handle
            .path()
            .app_data_dir()
            .expect("Failed to get app data directory");
        let themes_dir = app_data_dir
            .join("data")
            .join("default-user")
            .join("themes");

        Self { themes_dir }
    }

    /// Ensure the themes directory exists
    async fn ensure_directory_exists(&self) -> Result<(), DomainError> {
        if !self.themes_dir.exists() {
            tokio::fs::create_dir_all(&self.themes_dir)
                .await
                .map_err(|e| {
                    logger::error(&format!("Failed to create themes directory: {}", e));
                    DomainError::InternalError(format!("Failed to create themes directory: {}", e))
                })?;
        }

        Ok(())
    }

    /// Get the path to a theme file
    fn get_theme_path(&self, name: &str) -> PathBuf {
        let filename = format!("{}.json", sanitize_filename(name));
        self.themes_dir.join(filename)
    }
}

#[async_trait]
impl ThemeRepository for FileThemeRepository {
    async fn save_theme(&self, theme: &Theme) -> Result<(), DomainError> {
        logger::debug(&format!("Saving theme: {}", theme.name));

        // Ensure the directory exists
        self.ensure_directory_exists().await?;

        // Get the path to the theme file
        let path = self.get_theme_path(&theme.name);

        // Create a new JSON object that includes the name
        let mut theme_data = theme.data.clone();

        // Ensure theme_data is an object
        if !theme_data.is_object() {
            theme_data = serde_json::json!({});
        }

        // Add the name to the theme data
        if let Some(obj) = theme_data.as_object_mut() {
            obj.insert("name".to_string(), serde_json::json!(theme.name));
        }

        // Write the theme data to the file
        write_json_file(&path, &theme_data).await?;

        Ok(())
    }

    async fn delete_theme(&self, name: &str) -> Result<(), DomainError> {
        logger::debug(&format!("Deleting theme: {}", name));

        let path = self.get_theme_path(name);

        if !path.exists() {
            return Err(DomainError::NotFound(format!("Theme not found: {}", name)));
        }

        delete_file(&path).await?;

        Ok(())
    }
}
