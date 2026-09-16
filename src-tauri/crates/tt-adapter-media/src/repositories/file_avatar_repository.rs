use async_trait::async_trait;
use image::ImageFormat;
use mime_guess::from_path;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::fs as tokio_fs;

use crate::persona_cards;
use tt_domain::errors::DomainError;
use tt_domain::models::avatar::{Avatar, AvatarUploadResult, CropInfo};
use tt_domain::models::persona::{Persona, Personas};
use tt_ports::repositories::avatar_repository::AvatarRepository;

// Constants for avatar dimensions
const AVATAR_WIDTH: u32 = 400;
const AVATAR_HEIGHT: u32 = 600;

/// File-based implementation of AvatarRepository
pub struct FileAvatarRepository {
    avatars_dir: PathBuf,
    persona_cache: Arc<Mutex<persona_cards::PersonaCache>>,
}

impl FileAvatarRepository {
    /// Create a new FileAvatarRepository
    pub fn new(avatars_dir: PathBuf) -> Self {
        // Create directory if it doesn't exist
        fs::create_dir_all(&avatars_dir).expect("Failed to create avatars directory");

        Self {
            avatars_dir,
            persona_cache: Arc::default(),
        }
    }

    /// Process an image file with optional cropping
    fn process_image(img_data: &[u8], crop_info: Option<CropInfo>) -> Result<Vec<u8>, DomainError> {
        // Load the image
        let mut img = image::load_from_memory(img_data)
            .map_err(|e| DomainError::InternalError(format!("Failed to load image: {}", e)))?;

        // Apply cropping if specified
        if let Some(crop) = crop_info
            && crop.x >= 0
            && crop.y >= 0
            && crop.width > 0
            && crop.height > 0
            && (crop.x as u32) < img.width()
            && (crop.y as u32) < img.height()
        {
            img = img.crop_imm(
                crop.x as u32,
                crop.y as u32,
                crop.width as u32,
                crop.height as u32,
            );
        }

        // Resize the image to the standard avatar dimensions
        let resized_img = img.resize_exact(
            AVATAR_WIDTH,
            AVATAR_HEIGHT,
            image::imageops::FilterType::Lanczos3,
        );

        // Convert the image to PNG format
        let mut buffer = Vec::new();
        let mut cursor = std::io::Cursor::new(&mut buffer);
        resized_img
            .write_to(&mut cursor, ImageFormat::Png)
            .map_err(|e| DomainError::InternalError(format!("Failed to encode image: {}", e)))?;

        Ok(buffer)
    }

    /// Sanitize a filename
    fn sanitize_filename(filename: &str) -> String {
        let sanitized = filename
            .chars()
            .map(|c| match c {
                '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
                _ if c.is_control() => '_',
                _ => c,
            })
            .collect::<String>();

        sanitized.trim().trim_end_matches(['.', ' ']).to_string()
    }

    fn is_supported_avatar_file(path: &Path) -> bool {
        from_path(path)
            .first()
            .is_some_and(|mime| mime.type_() == "image")
    }
}

#[async_trait]
impl AvatarRepository for FileAvatarRepository {
    async fn get_personas(&self) -> Result<Personas, DomainError> {
        let root = self
            .avatars_dir
            .parent()
            .expect("user directory")
            .to_path_buf();
        let cache = Arc::clone(&self.persona_cache);
        tokio::task::spawn_blocking(move || {
            // ponytail: library reads serialize here; split only if contention becomes measurable.
            let mut cache = cache.lock().map_err(|error| {
                DomainError::InternalError(format!("Persona cache lock poisoned: {error}"))
            })?;
            cache.read_personas(&root)
        })
        .await
        .map_err(|error| DomainError::InternalError(error.to_string()))?
    }

    async fn save_persona(&self, avatar: &str, persona: &Persona) -> Result<(), DomainError> {
        let directory = self.avatars_dir.clone();
        let avatar = avatar.to_owned();
        let persona = persona.clone();
        tokio::task::spawn_blocking(move || persona_cards::save(&directory, &avatar, &persona))
            .await
            .map_err(|error| DomainError::InternalError(error.to_string()))?
    }

    async fn import_personas(&self, personas: &Personas) -> Result<(), DomainError> {
        let directory = self.avatars_dir.clone();
        let personas = personas.clone();
        tokio::task::spawn_blocking(move || {
            for (id, persona) in personas {
                persona_cards::import_persona(&directory, &id, &persona)?;
            }
            Ok(())
        })
        .await
        .map_err(|error| DomainError::InternalError(error.to_string()))?
    }

    async fn get_avatars(&self) -> Result<Vec<Avatar>, DomainError> {
        tracing::debug!("Getting all avatars");

        let mut avatars = Vec::new();

        // Read the avatars directory
        let entries = fs::read_dir(&self.avatars_dir).map_err(|e| {
            tracing::error!("Failed to read avatars directory: {}", e);
            DomainError::InternalError(format!("Failed to read avatars directory: {}", e))
        })?;

        // Process each entry
        for entry in entries {
            let entry = entry.map_err(|e| {
                tracing::error!("Failed to read directory entry: {}", e);
                DomainError::InternalError(format!("Failed to read directory entry: {}", e))
            })?;

            let path = entry.path();
            if path.is_file()
                && Self::is_supported_avatar_file(&path)
                && let Some(name) = path.file_name()
            {
                let name_str = name.to_string_lossy().to_string();
                avatars.push(Avatar {
                    name: name_str,
                    path: path.clone(),
                });
            }
        }

        avatars.sort_by(|left, right| left.name.cmp(&right.name));

        tracing::debug!("Found {} avatars", avatars.len());
        Ok(avatars)
    }

    async fn delete_avatar(&self, avatar_name: &str) -> Result<(), DomainError> {
        tracing::debug!("Deleting avatar: {}", avatar_name);

        // Sanitize the avatar name
        let sanitized_name = Self::sanitize_filename(avatar_name);
        let avatar_path = self.avatars_dir.join(&sanitized_name);

        // Check if the avatar exists
        if !avatar_path.exists() {
            return Err(DomainError::NotFound(format!(
                "Avatar not found: {}",
                avatar_name
            )));
        }

        // Delete the avatar file
        tokio_fs::remove_file(&avatar_path).await.map_err(|e| {
            tracing::error!("Failed to delete avatar: {}", e);
            DomainError::InternalError(format!("Failed to delete avatar: {}", e))
        })?;

        tracing::info!("Avatar deleted: {}", avatar_name);
        Ok(())
    }

    async fn upload_avatar(
        &self,
        file_path: &Path,
        overwrite_name: Option<String>,
        crop_info: Option<CropInfo>,
    ) -> Result<AvatarUploadResult, DomainError> {
        tracing::debug!("Uploading avatar: {:?}", file_path);

        let filename = match overwrite_name {
            Some(name) => Self::sanitize_filename(&name),
            None => format!("{}.png", chrono::Utc::now().timestamp_millis()),
        };
        let avatar_path = persona_cards::avatar_path(&self.avatars_dir, &filename)?;
        let file_path = file_path.to_path_buf();
        tokio::task::spawn_blocking(move || {
            let input = fs::read(file_path)
                .map_err(|error| DomainError::InternalError(error.to_string()))?;
            // Changing an avatar changes its image, not its Persona identity.
            let persona = match fs::read(&avatar_path) {
                Ok(previous) => persona_cards::from_image(&previous)?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    persona_cards::from_image(&input)?
                }
                Err(error) => return Err(DomainError::InternalError(error.to_string())),
            }
            .unwrap_or_default();
            let image = Self::process_image(&input, crop_info)?;
            persona_cards::publish(
                &avatar_path,
                &persona_cards::with_persona(&image, &persona)?,
            )
        })
        .await
        .map_err(|error| DomainError::InternalError(error.to_string()))??;

        tracing::info!("Avatar uploaded: {}", filename);
        Ok(AvatarUploadResult { path: filename })
    }
}

#[cfg(test)]
mod tests {
    use super::FileAvatarRepository;
    use image::{ImageFormat, Rgba, RgbaImage};
    use std::fs;
    use std::path::{Path, PathBuf};
    use tt_ports::repositories::avatar_repository::AvatarRepository;

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new() -> Self {
            let path = std::env::temp_dir()
                .join(format!("tauritavern-avatar-test-{}", uuid::Uuid::new_v4()));
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

    fn write_png(path: &Path) {
        let image = RgbaImage::from_pixel(1, 1, Rgba([255, 0, 0, 255]));
        image
            .save_with_format(path, ImageFormat::Png)
            .expect("failed to write test png");
    }

    #[tokio::test]
    async fn get_avatars_ignores_non_images_and_sorts_names() {
        let dir = TestDir::new();
        write_png(&dir.path().join("b.png"));
        write_png(&dir.path().join("a.png"));
        fs::write(dir.path().join("notes.txt"), "not an image")
            .expect("failed to write test text file");

        let repository = FileAvatarRepository::new(dir.path().to_path_buf());
        let avatars = repository.get_avatars().await.expect("get avatars failed");
        let names = avatars
            .into_iter()
            .map(|avatar| avatar.name)
            .collect::<Vec<_>>();

        assert_eq!(names, vec!["a.png".to_string(), "b.png".to_string()]);
    }

    #[test]
    fn sanitize_filename_matches_expected_avatar_rules() {
        assert_eq!(
            FileAvatarRepository::sanitize_filename(" test:/name?.png. "),
            "test__name_.png"
        );
        assert_eq!(
            FileAvatarRepository::sanitize_filename("control\u{0000}"),
            "control_"
        );
    }

    #[test]
    fn persona_migration_preserves_data_and_images() {
        use crate::persona_cards;
        use serde_json::{Value, json};
        use tt_adapter_storage_core::png_metadata::replace_text_chunks;
        use tt_domain::models::persona::insert_personas;

        let dir = TestDir::new();
        let root = dir.path();
        let avatars = root.join("User Avatars");
        fs::create_dir_all(&avatars).unwrap();
        let card_path = avatars.join("alice.png");
        write_png(&card_path);
        let original_image = fs::read(&card_path).unwrap();
        fs::write(avatars.join("broken.png"), b"broken image").unwrap();
        let legacy = json!({
            "other_setting": "keep",
            "power_user": {
                "personas": {"alice.png": "爱丽丝", "broken.png": "Recovered", "missing.png": "Missing"},
                "persona_descriptions": {
                    "alice.png": {"description": "原文 🌸", "extension_field": {"keep": [true]}},
                    "broken.png": {"description": "Keep this text"}
                }
            }
        });
        let settings_path = root.join("settings.json");
        fs::write(&settings_path, legacy.to_string()).unwrap();
        persona_cards::migrate_personas(root, root).unwrap();
        let mut settings: Value =
            serde_json::from_slice(&fs::read(settings_path).unwrap()).unwrap();
        assert!(settings["power_user"].get("personas").is_none());
        assert!(settings["power_user"].get("persona_descriptions").is_none());
        insert_personas(&mut settings, &persona_cards::read_personas(root).unwrap());
        assert_eq!(settings, legacy);
        assert!(
            fs::read_dir(&avatars)
                .unwrap()
                .any(|entry| fs::read(entry.unwrap().path()).unwrap() == b"broken image")
        );
        assert_eq!(
            replace_text_chunks(&fs::read(card_path).unwrap(), &["persona"], &[]).unwrap(),
            original_image
        );
    }

    #[tokio::test]
    async fn avatar_changes_preserve_personas_and_refresh_from_disk() {
        use crate::persona_cards;
        use tt_domain::models::persona::Persona;
        let dir = TestDir::new();
        let root = dir.path().join("default-user");
        let repository = FileAvatarRepository::new(root.join("User Avatars"));
        let source = dir.path().join("incoming.png");
        write_png(&source);
        let mut persona = Persona {
            name: Some("中文 🌸".into()),
            description: Some(serde_json::json!({"description":"Keep", "title":"Title"})),
        };
        let tagged = persona_cards::with_persona(&fs::read(&source).unwrap(), &persona).unwrap();
        fs::write(&source, tagged).unwrap();
        repository
            .upload_avatar(&source, Some("one.png".into()), None)
            .await
            .unwrap();
        assert_eq!(repository.get_personas().await.unwrap()["one.png"], persona);
        write_png(&source);
        repository
            .upload_avatar(&source, Some("one.png".into()), None)
            .await
            .unwrap();
        assert_eq!(repository.get_personas().await.unwrap()["one.png"], persona);
        let card_path = root.join("User Avatars/one.png");
        let modified = fs::metadata(&card_path).unwrap().modified().unwrap();
        // First change only the size, then only mtime; neither needs a cache notification.
        for (name, modified) in [
            ("Alice", modified),
            ("Robin", modified + std::time::Duration::from_secs(2)),
        ] {
            persona.name = Some(name.into());
            let image = fs::read(&card_path).unwrap();
            fs::write(
                &card_path,
                persona_cards::with_persona(&image, &persona).unwrap(),
            )
            .unwrap();
            fs::File::options()
                .write(true)
                .open(&card_path)
                .unwrap()
                .set_modified(modified)
                .unwrap();
            assert_eq!(repository.get_personas().await.unwrap()["one.png"], persona);
        }
        // One broken card must not hide or rewrite the other cards.
        fs::copy(&card_path, root.join("User Avatars/broken.png")).unwrap();
        assert_eq!(repository.get_personas().await.unwrap().len(), 2);
        fs::write(
            root.join("User Avatars/broken.png"),
            b"\x89PNG\r\n\x1a\n\0\0\0\x10iTXt",
        )
        .unwrap();
        let personas = repository.get_personas().await.unwrap();
        assert_eq!(personas.len(), 1);
        assert_eq!(personas["one.png"], persona);
        repository.delete_avatar("one.png").await.unwrap();
        assert!(repository.save_persona("one.png", &persona).await.is_err());
        assert!(!card_path.exists());
        assert!(repository.get_personas().await.unwrap().is_empty());
    }
}
