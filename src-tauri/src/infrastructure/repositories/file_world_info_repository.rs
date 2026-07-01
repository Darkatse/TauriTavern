use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde_json::{Value, json};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use tokio::fs;

use crate::infrastructure::persistence::file_system::{
    delete_file, list_files_with_extension, read_json_file, write_json_file,
};
use crate::infrastructure::persistence::png_utils::read_text_chunks_from_png;
use crate::infrastructure::sillytavern_sorting::sort_strings_sillytavern_name;
use tt_domain::errors::DomainError;
use tt_domain::models::world_info::{
    WORLD_INFO_EXTENSION, sanitize_world_info_file_name, sanitize_world_info_import_name,
    validate_world_info_data,
};
use tt_ports::repositories::world_info_repository::WorldInfoRepository;

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

    fn get_world_path(&self, file_name: &str) -> PathBuf {
        self.worlds_dir.join(file_name)
    }

    fn normalize_world_file_name(&self, name: &str) -> Result<String, DomainError> {
        let file_name = sanitize_world_info_file_name(name);
        if file_name.is_empty()
            || Path::new(&file_name)
                .extension()
                .and_then(OsStr::to_str)
                .is_none_or(|ext| !ext.eq_ignore_ascii_case(WORLD_INFO_EXTENSION))
        {
            return Err(DomainError::InvalidData(
                "World file must have a name".to_string(),
            ));
        }

        Ok(file_name)
    }

    fn normalize_import_world_name(&self, original_filename: &str) -> Result<String, DomainError> {
        let world_name = sanitize_world_info_import_name(original_filename);
        if world_name.is_empty() {
            return Err(DomainError::InvalidData(
                "World file must have a name".to_string(),
            ));
        }

        Ok(world_name.to_string())
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
        if name.is_empty() {
            return Ok(if allow_dummy {
                Some(json!({ "entries": {} }))
            } else {
                None
            });
        }

        let file_name = self.normalize_world_file_name(name)?;
        let world_path = self.get_world_path(&file_name);

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

        let file_name = self.normalize_world_file_name(name)?;
        validate_world_info_data(data).map_err(DomainError::InvalidData)?;

        let world_path = self.get_world_path(&file_name);
        write_json_file(&world_path, data).await
    }

    async fn delete_world_info(&self, name: &str) -> Result<(), DomainError> {
        let file_name = self.normalize_world_file_name(name)?;
        let world_path = self.get_world_path(&file_name);

        if !world_path.exists() {
            return Err(DomainError::NotFound(format!(
                "World info file {} doesn't exist",
                file_name
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

        let world_name = self.normalize_import_world_name(original_filename)?;

        let data = self
            .read_import_payload(file_path, original_filename, converted_data)
            .await?;

        let file_name = self.normalize_world_file_name(&world_name)?;
        let target = self.get_world_path(&file_name);
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
        sort_strings_sillytavern_name(&mut names);

        Ok(names)
    }
}

#[cfg(test)]
mod tests {
    use super::FileWorldInfoRepository;
    use serde_json::json;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};
    use tt_ports::repositories::world_info_repository::WorldInfoRepository;

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
                "tauritavern-world-info-repo-test-{}-{}",
                std::process::id(),
                suffix
            ));
            std::fs::create_dir_all(&path).expect("create temp dir");
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    #[tokio::test]
    async fn save_get_delete_keeps_spaced_world_names_distinct() {
        let dir = TestDir::new();
        let repository = FileWorldInfoRepository::new(dir.path().to_path_buf());
        let plain = json!({ "entries": { "0": { "uid": 0, "content": "plain" } } });
        let leading = json!({ "entries": { "0": { "uid": 0, "content": "leading" } } });
        let trailing = json!({ "entries": { "0": { "uid": 0, "content": "trailing" } } });

        repository
            .save_world_info("Lore", &plain)
            .await
            .expect("save plain world");
        repository
            .save_world_info(" Lore", &leading)
            .await
            .expect("save leading-space world");
        repository
            .save_world_info("Lore ", &trailing)
            .await
            .expect("save trailing-space world");

        assert!(dir.path().join("Lore.json").exists());
        assert!(dir.path().join(" Lore.json").exists());
        assert!(dir.path().join("Lore .json").exists());
        assert_eq!(
            repository
                .get_world_info("Lore", false)
                .await
                .expect("get plain world"),
            Some(plain)
        );
        assert_eq!(
            repository
                .get_world_info(" Lore", false)
                .await
                .expect("get leading-space world"),
            Some(leading)
        );
        assert_eq!(
            repository
                .get_world_info("Lore ", false)
                .await
                .expect("get trailing-space world"),
            Some(trailing)
        );

        repository
            .delete_world_info("Lore")
            .await
            .expect("delete plain world");

        assert!(!dir.path().join("Lore.json").exists());
        assert!(dir.path().join(" Lore.json").exists());
        assert!(dir.path().join("Lore .json").exists());
    }

    #[tokio::test]
    async fn import_world_info_preserves_leading_space_from_original_filename() {
        let dir = TestDir::new();
        let repository = FileWorldInfoRepository::new(dir.path().to_path_buf());
        let source = dir.path().join("upload.json");
        std::fs::write(&source, r#"{"entries":{}}"#).expect("write import source");

        let imported_name = repository
            .import_world_info(&source, " Pinned.json", None)
            .await
            .expect("import world info");

        assert_eq!(imported_name, " Pinned");
        assert!(dir.path().join(" Pinned.json").exists());
    }

    #[tokio::test]
    async fn list_world_names_sorts_like_upstream_locale_compare() {
        let dir = TestDir::new();
        let repository = FileWorldInfoRepository::new(dir.path().to_path_buf());

        std::fs::write(dir.path().join("😀Book.json"), "{}").expect("write emoji world");
        std::fs::write(dir.path().join("Abook.json"), "{}").expect("write latin world");
        std::fs::write(dir.path().join("#Book.json"), "{}").expect("write symbol world");
        std::fs::write(dir.path().join("🧠Lore.json"), "{}").expect("write brain world");
        std::fs::write(dir.path().join("✨Lore.json"), "{}").expect("write sparkles world");
        std::fs::write(dir.path().join("_Book.json"), "{}").expect("write underscore world");
        std::fs::write(dir.path().join("-Book.json"), "{}").expect("write dash world");

        let names = repository
            .list_world_names()
            .await
            .expect("list world names");

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
}
