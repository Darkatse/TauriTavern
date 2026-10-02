use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde_json::value::RawValue;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use tokio::fs;

use crate::png_card_metadata::read_text_chunks_from_png;
use tt_adapter_storage_core::commit_stage::CommitSessions;
use tt_adapter_storage_core::file_system::{delete_file, list_files_with_extension};
use tt_adapter_storage_core::sillytavern_sorting::sort_strings_sillytavern_name;
use tt_contracts::byte_commit::CommitBegin;
use tt_domain::errors::DomainError;
use tt_domain::models::world_info::{
    WORLD_INFO_EXTENSION, sanitize_world_info_file_name, sanitize_world_info_import_name,
};
use tt_ports::repositories::world_info_repository::WorldInfoRepository;

mod commit;
mod document;

pub struct FileWorldInfoRepository {
    worlds_dir: PathBuf,
    commit_sessions: CommitSessions<()>,
}

impl FileWorldInfoRepository {
    pub fn new(worlds_dir: PathBuf) -> Self {
        let staging_dir = worlds_dir.with_file_name(".staging").join("world-info");
        Self {
            worlds_dir,
            commit_sessions: CommitSessions::new(staging_dir),
        }
    }

    fn get_world_path(&self, file_name: &str) -> PathBuf {
        self.worlds_dir.join(file_name)
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

    fn parse_world_info_png(&self, image_data: &[u8]) -> Result<Vec<u8>, DomainError> {
        let text_chunks = read_text_chunks_from_png(image_data)?;

        for chunk in text_chunks.iter().rev() {
            if !chunk.keyword.eq_ignore_ascii_case("naidata") {
                continue;
            }

            let decoded = BASE64.decode(chunk.text.trim()).map_err(|e| {
                DomainError::InvalidData(format!("Failed to decode world info PNG data: {}", e))
            })?;

            return Ok(decoded);
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
    ) -> Result<Vec<u8>, DomainError> {
        if let Some(converted) = converted_data {
            return Ok(converted.as_bytes().to_vec());
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

        let bytes = fs::read(file_path).await.map_err(|e| {
            DomainError::InternalError(format!(
                "Failed to read world info import file {}: {}",
                file_path.display(),
                e
            ))
        })?;

        Ok(bytes)
    }
}

fn world_file_name(name: &str) -> Result<String, DomainError> {
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

#[async_trait]
impl WorldInfoRepository for FileWorldInfoRepository {
    async fn read_world_info_json(&self, name: &str) -> Result<Option<Vec<u8>>, DomainError> {
        if name.is_empty() {
            return Ok(None);
        }
        let path = self.get_world_path(&world_file_name(name)?);
        tokio::task::spawn_blocking(move || {
            let bytes = match std::fs::read(&path) {
                Ok(bytes) => bytes,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(error) => {
                    return Err(DomainError::InternalError(format!(
                        "Failed to read world info {}: {error}",
                        path.display()
                    )));
                }
            };
            serde_json::from_slice::<&RawValue>(&bytes).map_err(|error| {
                DomainError::InvalidData(format!("World info {}: {error}", path.display()))
            })?;
            Ok(Some(bytes))
        })
        .await
        .map_err(|error| {
            DomainError::InternalError(format!("World info read task failed: {error}"))
        })?
    }

    async fn write_world_info_json(&self, name: &str, json: Vec<u8>) -> Result<(), DomainError> {
        let target = self.get_world_path(&world_file_name(name)?);
        self.publish_document(target, json).await
    }

    async fn begin_commit(&self) -> Result<CommitBegin, DomainError> {
        self.commit_sessions.begin(()).await
    }

    async fn append_commit(
        &self,
        session_id: &str,
        offset: u64,
        bytes: &[u8],
    ) -> Result<u64, DomainError> {
        self.commit_sessions
            .append(session_id, offset, bytes, |()| {})
            .await
    }

    async fn finish_commit(&self, session_id: &str, expected_size: u64) -> Result<(), DomainError> {
        self.commit_sessions
            .finish(session_id, expected_size, |session| async move {
                let source = session.stage.path().to_owned();
                let publish_path = session.stage.publish_path();
                drop(session);
                let worlds = self.worlds_dir.clone();
                tokio::task::spawn_blocking(move || {
                    let bytes = std::fs::read(&source)
                        .map_err(|error| commit::io_error(&source, "read", error))?;
                    let (name, document) = document::replacement(&bytes)?;
                    let target = worlds.join(world_file_name(&name)?);
                    commit::publish(&publish_path, &target, document)
                })
                .await
                .map_err(commit::task_error)?
            })
            .await
    }

    async fn abort_commit(&self, session_id: &str) -> Result<(), DomainError> {
        self.commit_sessions.abort(session_id).await
    }

    async fn delete_world_info(&self, name: &str) -> Result<(), DomainError> {
        let file_name = world_file_name(name)?;
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
        let world_name = self.normalize_import_world_name(original_filename)?;

        let data = self
            .read_import_payload(file_path, original_filename, converted_data)
            .await?;

        self.write_world_info_json(&world_name, data).await?;

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
    use tt_ports::repositories::world_info_repository::WorldInfoRepository;

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "tauritavern-world-info-repo-test-{}",
                uuid::Uuid::new_v4()
            ));
            let path = path.join("worlds");
            std::fs::create_dir_all(&path).expect("create temp dir");
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(self.path.parent().expect("test root"));
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
                .get_world_info("Lore")
                .await
                .expect("get plain world"),
            Some(plain)
        );
        assert_eq!(
            repository
                .get_world_info(" Lore")
                .await
                .expect("get leading-space world"),
            Some(leading)
        );
        assert_eq!(
            repository
                .get_world_info("Lore ")
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
        let json = r#"{ "z":1,"entries":{},"originalData":{"b":2,"a":3} }"#;
        std::fs::write(&source, json).expect("write import source");

        let imported_name = repository
            .import_world_info(&source, " Pinned.json", None)
            .await
            .expect("import world info");

        assert_eq!(imported_name, " Pinned");
        assert_eq!(
            std::fs::read_to_string(dir.path().join(" Pinned.json")).unwrap(),
            json
        );
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
