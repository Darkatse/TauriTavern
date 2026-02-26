use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde_json::{Value, json};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use tokio::fs;

use crate::domain::errors::DomainError;
use crate::domain::models::world_info::{sanitize_world_info_name, validate_world_info_data};
use crate::domain::repositories::world_info_repository::WorldInfoRepository;
use crate::infrastructure::persistence::file_system::{
    delete_file, list_files_with_extension, read_json_file, write_json_file,
};
use crate::infrastructure::persistence::png_utils::read_text_chunks_from_png;

pub struct FileWorldInfoRepository {
    worlds_dir: PathBuf,
}

impl FileWorldInfoRepository {
    pub fn new(worlds_dir: PathBuf) -> Self {
        Self { worlds_dir }
    }

    async fn ensure_directory_exists(&self) -> Result<(), DomainError> {
        if !self.worlds_dir.exists() {
            fs::create_dir_all(&self.worlds_dir).await.map_err(|e| {
                DomainError::InternalError(format!(
                    "Failed to create worlds directory {}: {}",
                    self.worlds_dir.display(),
                    e
                ))
            })?;
        }

        Ok(())
    }

    fn get_world_path(&self, name: &str) -> PathBuf {
        self.worlds_dir.join(format!("{}.json", name))
    }

    fn normalize_world_name(&self, name: &str) -> Result<String, DomainError> {
        let normalized = sanitize_world_info_name(name);
        if normalized.is_empty() {
            return Err(DomainError::InvalidData(
                "World file must have a name".to_string(),
            ));
        }

        Ok(normalized)
    }

    fn parse_world_info_json(&self, json_text: &str) -> Result<Value, DomainError> {
        let parsed = serde_json::from_str::<Value>(json_text).map_err(|e| {
            DomainError::InvalidData(format!("Is not a valid world info file: {}", e))
        })?;

        validate_world_info_data(&parsed).map_err(DomainError::InvalidData)?;
        Ok(parsed)
    }

    fn parse_world_info_png(&self, image_data: &[u8]) -> Result<Value, DomainError> {
        let text_chunks = read_text_chunks_from_png(image_data)?;

        for chunk in text_chunks.iter().rev() {
            if !chunk.keyword.eq_ignore_ascii_case("naidata") {
                continue;
            }

            let decoded = BASE64.decode(chunk.text.trim()).map_err(|e| {
                DomainError::InvalidData(format!("Failed to decode world info PNG data: {}", e))
            })?;

            let decoded_json = String::from_utf8(decoded).map_err(|e| {
                DomainError::InvalidData(format!("Failed to parse world info PNG data: {}", e))
            })?;

            return self.parse_world_info_json(&decoded_json);
        }

        Err(DomainError::InvalidData(
            "PNG Image contains no world info data".to_string(),
        ))
    }

    async fn read_import_payload(
        &self,
        file_path: &Path,
        original_filename: &str,
        converted_data: Option<&str>,
    ) -> Result<Value, DomainError> {
        if let Some(converted) = converted_data {
            return self.parse_world_info_json(converted);
        }

        let is_png = Path::new(original_filename)
            .extension()
            .and_then(OsStr::to_str)
            .map(|ext| ext.eq_ignore_ascii_case("png"))
            .or_else(|| {
                file_path
                    .extension()
                    .and_then(OsStr::to_str)
                    .map(|ext| ext.eq_ignore_ascii_case("png"))
            })
            .unwrap_or(false);

        if is_png {
            let image_data = fs::read(file_path).await.map_err(|e| {
                DomainError::InternalError(format!(
                    "Failed to read world info import file {}: {}",
                    file_path.display(),
                    e
                ))
            })?;

            return self.parse_world_info_png(&image_data);
        }

        let text_data = fs::read_to_string(file_path).await.map_err(|e| {
            DomainError::InternalError(format!(
                "Failed to read world info import file {}: {}",
                file_path.display(),
                e
            ))
        })?;

        self.parse_world_info_json(&text_data)
    }
}

#[async_trait]
impl WorldInfoRepository for FileWorldInfoRepository {
    async fn get_world_info(
        &self,
        name: &str,
        allow_dummy: bool,
    ) -> Result<Option<Value>, DomainError> {
        if name.trim().is_empty() {
            return Ok(if allow_dummy {
                Some(json!({ "entries": {} }))
            } else {
                None
            });
        }

        let world_name = self.normalize_world_name(name)?;
        let world_path = self.get_world_path(&world_name);

        if !world_path.exists() {
            return Ok(if allow_dummy {
                Some(json!({ "entries": {} }))
            } else {
                None
            });
        }

        let data = read_json_file::<Value>(&world_path).await?;
        Ok(Some(data))
    }

    async fn save_world_info(&self, name: &str, data: &Value) -> Result<(), DomainError> {
        self.ensure_directory_exists().await?;

        let world_name = self.normalize_world_name(name)?;
        validate_world_info_data(data).map_err(DomainError::InvalidData)?;

        let world_path = self.get_world_path(&world_name);
        write_json_file(&world_path, data).await
    }

    async fn delete_world_info(&self, name: &str) -> Result<(), DomainError> {
        let world_name = self.normalize_world_name(name)?;
        let world_path = self.get_world_path(&world_name);

        if !world_path.exists() {
            return Err(DomainError::NotFound(format!(
                "World info file {} doesn't exist",
                world_name
            )));
        }

        delete_file(&world_path).await
    }

    async fn import_world_info(
        &self,
        file_path: &Path,
        original_filename: &str,
        converted_data: Option<&str>,
    ) -> Result<String, DomainError> {
        self.ensure_directory_exists().await?;

        let world_name_raw = Path::new(original_filename)
            .file_stem()
            .and_then(OsStr::to_str)
            .unwrap_or_default();
        let world_name = self.normalize_world_name(world_name_raw)?;

        let data = self
            .read_import_payload(file_path, original_filename, converted_data)
            .await?;

        let target = self.get_world_path(&world_name);
        write_json_file(&target, &data).await?;

        Ok(world_name)
    }

    async fn list_world_names(&self) -> Result<Vec<String>, DomainError> {
        if !self.worlds_dir.exists() {
            return Ok(Vec::new());
        }

        let files = list_files_with_extension(&self.worlds_dir, "json").await?;
        let mut names: Vec<String> = files
            .into_iter()
            .filter_map(|file| {
                file.file_stem()
                    .and_then(OsStr::to_str)
                    .map(|name| name.to_string())
            })
            .collect();
        names.sort();

        Ok(names)
    }
}
