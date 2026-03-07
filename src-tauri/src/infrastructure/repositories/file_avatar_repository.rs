use async_trait::async_trait;
use image::ImageFormat;
use mime_guess::from_path;
use std::fs;
use std::path::{Path, PathBuf};
use tokio::fs as tokio_fs;

use crate::domain::errors::DomainError;
use crate::domain::models::avatar::{Avatar, AvatarUploadResult, CropInfo};
use crate::domain::repositories::avatar_repository::AvatarRepository;

// Constants for avatar dimensions
const AVATAR_WIDTH: u32 = 400;
const AVATAR_HEIGHT: u32 = 600;

/// File-based implementation of AvatarRepository
pub struct FileAvatarRepository {
    avatars_dir: PathBuf,
}

impl FileAvatarRepository {
    /// Create a new FileAvatarRepository
    pub fn new(avatars_dir: PathBuf) -> Self {
        // Create directory if it doesn't exist
        fs::create_dir_all(&avatars_dir).expect("Failed to create avatars directory");

        Self { avatars_dir }
    }

    /// Process an image file with optional cropping
    async fn process_image(
        &self,
        file_path: &Path,
        crop_info: Option<CropInfo>,
    ) -> Result<Vec<u8>, DomainError> {
        // Read the image file
        let img_data = tokio_fs::read(file_path)
            .await
            .map_err(|e| DomainError::InternalError(format!("Failed to read image file: {}", e)))?;

        // Load the image
        let mut img = image::load_from_memory(&img_data)
            .map_err(|e| DomainError::InternalError(format!("Failed to load image: {}", e)))?;

        // Apply cropping if specified
        if let Some(crop) = crop_info {
            if crop.x >= 0
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
            if path.is_file() && Self::is_supported_avatar_file(&path) {
                if let Some(name) = path.file_name() {
                    let name_str = name.to_string_lossy().to_string();
                    avatars.push(Avatar {
                        name: name_str,
                        path: path.clone(),
                    });
                }
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

        // Process the image
        let image_data = self.process_image(file_path, crop_info).await?;

        // Generate a filename
        let filename = match overwrite_name {
            Some(name) => Self::sanitize_filename(&name),
            None => format!("{}.png", chrono::Utc::now().timestamp_millis()),
        };

        // Save the processed image
        let avatar_path = self.avatars_dir.join(&filename);
        tokio_fs::write(&avatar_path, &image_data)
            .await
            .map_err(|e| {
                tracing::error!("Failed to write avatar file: {}", e);
                DomainError::InternalError(format!("Failed to write avatar file: {}", e))
            })?;

        tracing::info!("Avatar uploaded: {}", filename);
        Ok(AvatarUploadResult { path: filename })
    }
}

#[cfg(test)]
mod tests {
    use super::FileAvatarRepository;
    use crate::domain::repositories::avatar_repository::AvatarRepository;
    use image::{ImageFormat, Rgba, RgbaImage};
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
                "tauritavern-avatar-test-{}-{}",
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
}
