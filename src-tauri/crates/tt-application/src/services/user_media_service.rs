use std::path::{Path, PathBuf};
use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use serde::Serialize;

use crate::client_asset_paths::{UserDataAssetKind, parse_user_data_asset_request_path};
use tt_domain::errors::DomainError;
use tt_domain::models::filename::sanitize_filename;
#[cfg(test)]
use tt_ports::user_media::UserMediaStoreError;
pub use tt_ports::user_media::{UserMediaEntry, UserMediaStore};

const MEDIA_EXTENSIONS: &[&str] = &[
    "bmp", "png", "jpg", "webp", "jpeg", "jfif", "gif", "mp4", "avi", "mov", "wmv", "flv", "webm",
    "3gp", "mkv", "mpg", "mp3", "wav", "ogg", "flac", "aac", "m4a", "aiff",
];

const MEDIA_REQUEST_IMAGE: u32 = 0b001;
const MEDIA_REQUEST_VIDEO: u32 = 0b010;
const MEDIA_REQUEST_AUDIO: u32 = 0b100;

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
pub struct UserImageUploadResult {
    pub path: String,
}

#[derive(Debug, Clone)]
pub struct UploadUserImageInput {
    pub image_base64: String,
    pub format: String,
    pub filename: Option<String>,
    pub ch_name: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ListUserImagesInput {
    pub folder: String,
    pub sort_field: Option<String>,
    pub sort_order: Option<String>,
    pub media_type: Option<u32>,
}

#[derive(Clone)]
pub struct UserMediaService {
    store: Arc<dyn UserMediaStore>,
}

#[derive(Debug, Clone)]
struct ListedMediaFile {
    name: String,
    modified_ms: Option<i128>,
}

impl UserMediaService {
    pub fn new(store: Arc<dyn UserMediaStore>) -> Self {
        Self { store }
    }

    pub async fn upload_user_image(
        &self,
        input: UploadUserImageInput,
    ) -> Result<UserImageUploadResult, DomainError> {
        let image_base64 = input.image_base64.trim().to_string();
        if image_base64.is_empty() {
            return Err(DomainError::InvalidData(
                "No image data provided".to_string(),
            ));
        }

        let format = validate_media_format(&input.format)?;

        let raw_filename = input
            .filename
            .as_deref()
            .filter(|value| !value.is_empty())
            .map(|value| format!("{}.{}", remove_last_extension(value), format))
            .unwrap_or_else(|| format!("{}.{}", chrono::Utc::now().timestamp_millis(), format));
        let safe_filename = sanitize_filename(&raw_filename);
        if safe_filename.is_empty() {
            return Err(DomainError::InvalidData("Invalid filename".to_string()));
        }

        let safe_folder = input
            .ch_name
            .as_deref()
            .filter(|value| !value.is_empty())
            .map(sanitize_filename)
            .filter(|value| !value.is_empty());

        let mut relative_path = PathBuf::new();
        if let Some(folder) = safe_folder {
            relative_path.push(folder);
        }
        relative_path.push(safe_filename);

        let bytes = BASE64_STANDARD
            .decode(image_base64.as_bytes())
            .map_err(|error| DomainError::InvalidData(format!("Invalid image data: {}", error)))?;

        self.store
            .write_file(&relative_path, bytes)
            .await
            .map_err(DomainError::from)?;

        Ok(UserImageUploadResult {
            path: format!("user/images/{}", to_url_path(&relative_path)),
        })
    }

    pub async fn list_user_images(
        &self,
        input: ListUserImagesInput,
    ) -> Result<Vec<String>, DomainError> {
        if input.folder.is_empty() {
            return Err(DomainError::InvalidData("No folder specified".to_string()));
        }

        let sanitized_folder = sanitize_filename(&input.folder);
        if sanitized_folder.is_empty() {
            return Err(DomainError::InvalidData(
                "Invalid folder specified".to_string(),
            ));
        }

        let sort_field = input
            .sort_field
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("date")
            .to_ascii_lowercase();
        let sort_order = input
            .sort_order
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("asc")
            .to_ascii_lowercase();
        let media_type = input.media_type.unwrap_or(MEDIA_REQUEST_IMAGE);

        let target_folder = PathBuf::from(sanitized_folder);
        self.store
            .ensure_folder(&target_folder)
            .await
            .map_err(DomainError::from)?;

        let entries = self
            .store
            .list_files(&target_folder)
            .await
            .map_err(DomainError::from)?;

        Ok(filter_and_sort_media_files(
            entries,
            &sort_field,
            &sort_order,
            media_type,
        ))
    }

    pub async fn list_user_image_folders(&self) -> Result<Vec<String>, DomainError> {
        let mut folders = self.store.list_folders().await.map_err(DomainError::from)?;
        folders.sort();
        Ok(folders)
    }

    pub async fn delete_user_image(&self, path_or_url: &str) -> Result<(), DomainError> {
        let relative = normalize_user_image_reference(path_or_url)?;
        self.store
            .delete_file(&relative)
            .await
            .map_err(DomainError::from)
    }
}

fn remove_last_extension(filename: &str) -> &str {
    filename
        .rsplit_once('.')
        .map(|(base, _)| base)
        .unwrap_or(filename)
}

fn to_url_path(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn validate_media_format(raw: &str) -> Result<String, DomainError> {
    let format = raw.trim().to_ascii_lowercase();
    if format.is_empty() || !MEDIA_EXTENSIONS.contains(&format.as_str()) {
        return Err(DomainError::InvalidData("Invalid image format".to_string()));
    }
    Ok(format)
}

fn filter_and_sort_media_files(
    entries: Vec<UserMediaEntry>,
    sort_field: &str,
    sort_order: &str,
    media_type: u32,
) -> Vec<String> {
    let sort_by_date = sort_field == "date";
    let sort_by_name = sort_field == "name";

    let mut files = entries
        .into_iter()
        .filter(|entry| media_matches(entry.mime_type.as_deref(), media_type))
        .map(|entry| ListedMediaFile {
            name: entry.name,
            modified_ms: if sort_by_date {
                entry.modified_ms
            } else {
                None
            },
        })
        .collect::<Vec<_>>();

    if sort_by_name {
        files.sort_by(|left, right| left.name.cmp(&right.name));
    } else if sort_by_date {
        files.sort_by(|left, right| left.modified_ms.cmp(&right.modified_ms));
    }

    if sort_order == "desc" {
        files.reverse();
    }

    files.into_iter().map(|file| file.name).collect()
}

fn media_matches(mime_type: Option<&str>, media_type: u32) -> bool {
    let Some(mime_type) = mime_type else {
        return false;
    };

    ((media_type & MEDIA_REQUEST_IMAGE) != 0 && mime_type.starts_with("image/"))
        || ((media_type & MEDIA_REQUEST_VIDEO) != 0 && mime_type.starts_with("video/"))
        || ((media_type & MEDIA_REQUEST_AUDIO) != 0 && mime_type.starts_with("audio/"))
}

fn normalize_user_image_reference(raw: &str) -> Result<PathBuf, DomainError> {
    let mut value = raw.trim().to_string();
    if value.is_empty() {
        return Err(DomainError::InvalidData("No path specified".to_string()));
    }

    if let Ok(parsed_url) = url::Url::parse(&value) {
        value = parsed_url.path().to_string();
    }

    let normalized = value.replace('\\', "/");
    let normalized = if normalized.starts_with('/') {
        normalized
    } else {
        format!("/{}", normalized)
    };

    let parsed = parse_user_data_asset_request_path(normalized.as_str())
        .map_err(|_| DomainError::InvalidData("Invalid path".to_string()))?
        .ok_or_else(|| DomainError::InvalidData("Invalid path".to_string()))?;

    if parsed.kind != UserDataAssetKind::UserImage {
        return Err(DomainError::InvalidData("Invalid path".to_string()));
    }

    Ok(parsed.relative_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::Mutex;

    #[derive(Default)]
    struct FakeStore {
        files: Mutex<Vec<UserMediaEntry>>,
        folders: Mutex<Vec<String>>,
        writes: Mutex<Vec<(PathBuf, Vec<u8>)>>,
        deleted: Mutex<Vec<PathBuf>>,
    }

    #[async_trait]
    impl UserMediaStore for FakeStore {
        async fn write_file(
            &self,
            relative_path: &Path,
            bytes: Vec<u8>,
        ) -> Result<(), UserMediaStoreError> {
            self.writes
                .lock()
                .expect("writes lock")
                .push((relative_path.to_path_buf(), bytes));
            Ok(())
        }

        async fn ensure_folder(&self, _relative_folder: &Path) -> Result<(), UserMediaStoreError> {
            Ok(())
        }

        async fn list_files(
            &self,
            _relative_folder: &Path,
        ) -> Result<Vec<UserMediaEntry>, UserMediaStoreError> {
            Ok(self.files.lock().expect("files lock").clone())
        }

        async fn list_folders(&self) -> Result<Vec<String>, UserMediaStoreError> {
            Ok(self.folders.lock().expect("folders lock").clone())
        }

        async fn delete_file(&self, relative_path: &Path) -> Result<(), UserMediaStoreError> {
            self.deleted
                .lock()
                .expect("deleted lock")
                .push(relative_path.to_path_buf());
            Ok(())
        }
    }

    #[test]
    fn remove_last_extension_strips_only_final_suffix() {
        assert_eq!(remove_last_extension("a.b.c"), "a.b");
        assert_eq!(remove_last_extension("file"), "file");
    }

    #[tokio::test]
    async fn upload_user_image_writes_sanitized_path_and_returns_client_path() {
        let store = Arc::new(FakeStore::default());
        let service = UserMediaService::new(store.clone());

        let result = service
            .upload_user_image(UploadUserImageInput {
                image_base64: BASE64_STANDARD.encode(b"ok"),
                format: "PNG".to_string(),
                filename: Some("bad:name.old".to_string()),
                ch_name: Some("A/B".to_string()),
            })
            .await
            .expect("upload");

        assert_eq!(result.path, "user/images/AB/badname.png");
        assert_eq!(
            store.writes.lock().expect("writes lock").as_slice(),
            &[(PathBuf::from("AB").join("badname.png"), b"ok".to_vec())]
        );
    }

    #[tokio::test]
    async fn list_user_images_filters_and_sorts_by_date() {
        let store = Arc::new(FakeStore::default());
        store.files.lock().expect("files lock").extend([
            UserMediaEntry {
                name: "a.png".to_string(),
                mime_type: Some("image/png".to_string()),
                modified_ms: Some(100),
            },
            UserMediaEntry {
                name: "b.mp4".to_string(),
                mime_type: Some("video/mp4".to_string()),
                modified_ms: Some(200),
            },
            UserMediaEntry {
                name: "c.txt".to_string(),
                mime_type: Some("text/plain".to_string()),
                modified_ms: Some(50),
            },
        ]);
        let service = UserMediaService::new(store);

        let listed = service
            .list_user_images(ListUserImagesInput {
                folder: "gallery".to_string(),
                sort_field: Some("date".to_string()),
                sort_order: Some("asc".to_string()),
                media_type: Some(MEDIA_REQUEST_IMAGE | MEDIA_REQUEST_VIDEO),
            })
            .await
            .expect("list");

        assert_eq!(listed, vec!["a.png".to_string(), "b.mp4".to_string()]);
    }

    #[tokio::test]
    async fn delete_user_image_accepts_url_and_rejects_other_routes() {
        let store = Arc::new(FakeStore::default());
        let service = UserMediaService::new(store.clone());

        service
            .delete_user_image("https://example.test/user/images/folder/a.png")
            .await
            .expect("delete");

        assert_eq!(
            store.deleted.lock().expect("deleted lock").as_slice(),
            &[PathBuf::from("folder").join("a.png")]
        );

        let error = service
            .delete_user_image("/user/files/a.png")
            .await
            .expect_err("reject non-image route");
        assert!(matches!(error, DomainError::InvalidData(message) if message == "Invalid path"));
    }
}
