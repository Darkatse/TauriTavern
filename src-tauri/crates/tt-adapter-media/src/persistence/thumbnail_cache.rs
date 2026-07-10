use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use std::fs::{File, FileTimes};
use std::io::{Read, Seek, Write};
use std::path::Path;
use std::sync::Mutex;
use std::time::SystemTime;
use tokio::fs;
use tokio::io::AsyncReadExt;

use tt_adapter_storage_core::file_system::{replace_file_with_fallback_sync, unique_temp_path};
use tt_domain::errors::DomainError;

const ANIMATED_EXTENSIONS: &[&str] = &[".apng", ".mp4", ".webm", ".avi", ".mkv", ".flv", ".gif"];
static THUMBNAIL_COMMIT_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Copy)]
pub enum ThumbnailResizeMode {
    PreserveArea,
    Cover,
}

#[derive(Debug, Clone, Copy)]
pub struct ThumbnailConfig {
    pub width: u32,
    pub height: u32,
    pub quality: u8,
    pub resize_mode: ThumbnailResizeMode,
}

#[derive(Debug)]
pub enum OpenThumbnailSource {
    Original(File),
    CachedJpeg(File),
}

fn extension_lowercase(path: &Path) -> String {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| format!(".{}", extension.to_ascii_lowercase()))
        .unwrap_or_default()
}

fn is_apng_header(buffer: &[u8]) -> bool {
    buffer.windows(4).any(|chunk| chunk == b"acTL")
}

fn is_animated_webp_header(buffer: &[u8]) -> bool {
    buffer
        .windows(4)
        .any(|chunk| chunk == b"ANIM" || chunk == b"ANMF")
}

async fn read_image_header(path: &Path) -> Result<Vec<u8>, DomainError> {
    let mut file = fs::File::open(path)
        .await
        .map_err(|error| match error.kind() {
            std::io::ErrorKind::NotFound => {
                DomainError::NotFound(format!("Source image not found: {}", path.display()))
            }
            _ => DomainError::InternalError(format!(
                "Failed to inspect image header '{}': {}",
                path.display(),
                error
            )),
        })?;
    let mut header = vec![0u8; 512];
    let read_len = file.read(&mut header).await.map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to inspect image header '{}': {}",
            path.display(),
            error
        ))
    })?;
    header.truncate(read_len);
    Ok(header)
}

fn read_image_header_sync(file: &mut File, path: &Path) -> Result<Vec<u8>, DomainError> {
    file.rewind().map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to seek image header '{}': {}",
            path.display(),
            error
        ))
    })?;
    let mut header = vec![0u8; 512];
    let read_len = file.read(&mut header).map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to inspect image header '{}': {}",
            path.display(),
            error
        ))
    })?;
    header.truncate(read_len);
    file.rewind().map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to rewind image '{}': {}",
            path.display(),
            error
        ))
    })?;

    Ok(header)
}

pub async fn is_animated_image(path: &Path) -> Result<bool, DomainError> {
    let extension = extension_lowercase(path);
    if ANIMATED_EXTENSIONS.contains(&extension.as_str()) {
        return Ok(true);
    }

    if extension == ".png" {
        let header = read_image_header(path).await?;
        return Ok(is_apng_header(&header));
    }

    if extension == ".webp" {
        let header = read_image_header(path).await?;
        return Ok(is_animated_webp_header(&header));
    }

    Ok(false)
}

fn is_animated_image_sync(file: &mut File, path: &Path) -> Result<bool, DomainError> {
    let extension = extension_lowercase(path);
    if ANIMATED_EXTENSIONS.contains(&extension.as_str()) {
        return Ok(true);
    }

    if extension == ".png" {
        let header = read_image_header_sync(file, path)?;
        return Ok(is_apng_header(&header));
    }

    if extension == ".webp" {
        let header = read_image_header_sync(file, path)?;
        return Ok(is_animated_webp_header(&header));
    }

    Ok(false)
}

fn open_fresh_thumbnail_sync(
    thumbnail_path: &Path,
    original_modified: SystemTime,
) -> Result<Option<File>, DomainError> {
    let file = match File::open(thumbnail_path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(DomainError::InternalError(format!(
                "Failed to open thumbnail cache '{}': {}",
                thumbnail_path.display(),
                error
            )));
        }
    };
    let metadata = file.metadata().map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to inspect thumbnail cache '{}': {}",
            thumbnail_path.display(),
            error
        ))
    })?;
    if !metadata.is_file() {
        return Ok(None);
    }
    let thumbnail_modified = metadata.modified().map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to read thumbnail mtime '{}': {}",
            thumbnail_path.display(),
            error
        ))
    })?;

    Ok((original_modified == thumbnail_modified).then_some(file))
}

fn generate_thumbnail_sync(
    original_file: &mut File,
    original_path: &Path,
    original_modified: SystemTime,
    thumbnail_path: &Path,
    config: ThumbnailConfig,
) -> Result<(), DomainError> {
    original_file.rewind().map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to seek source image '{}': {}",
            original_path.display(),
            error
        ))
    })?;
    let mut source_bytes = Vec::new();
    original_file
        .read_to_end(&mut source_bytes)
        .map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read source image '{}': {}",
                original_path.display(),
                error
            ))
        })?;

    let source_image = image::load_from_memory(&source_bytes).map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to decode source image '{}': {}",
            original_path.display(),
            error
        ))
    })?;

    let width = config.width.max(1);
    let height = config.height.max(1);
    let thumbnail_image = match config.resize_mode {
        ThumbnailResizeMode::PreserveArea => {
            let source_width = source_image.width().max(1);
            let source_height = source_image.height().max(1);
            let aspect_ratio = source_width as f64 / source_height as f64;
            let target_area = (width as f64) * (height as f64);
            let thumbnail_width = ((target_area * aspect_ratio).sqrt().round() as u32).max(1);
            let thumbnail_height = ((target_area / aspect_ratio).sqrt().round() as u32).max(1);
            source_image.resize(thumbnail_width, thumbnail_height, FilterType::Triangle)
        }
        ThumbnailResizeMode::Cover => {
            source_image.resize_to_fill(width, height, FilterType::Triangle)
        }
    };

    let quality = config.quality.clamp(1, 100);
    let mut encoded = Vec::new();
    let mut encoder = JpegEncoder::new_with_quality(&mut encoded, quality);
    encoder.encode_image(&thumbnail_image).map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to encode thumbnail for '{}': {}",
            original_path.display(),
            error
        ))
    })?;

    if let Some(parent) = thumbnail_path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to ensure thumbnail directory '{}': {}",
                parent.display(),
                error
            ))
        })?;
    }

    let temp_path = unique_temp_path(thumbnail_path, "thumbnail");
    let mut temp_file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp_path)
        .map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to create temporary thumbnail '{}': {}",
                temp_path.display(),
                error
            ))
        })?;

    temp_file.write_all(&encoded).map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to write temporary thumbnail '{}': {}",
            temp_path.display(),
            error
        ))
    })?;
    temp_file
        .set_times(FileTimes::new().set_modified(original_modified))
        .map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to bind temporary thumbnail '{}' to source mtime: {}",
                temp_path.display(),
                error
            ))
        })?;
    drop(temp_file);

    let _commit = THUMBNAIL_COMMIT_LOCK.lock().map_err(|_| {
        DomainError::InternalError("Thumbnail cache commit lock is poisoned".to_string())
    })?;
    replace_file_with_fallback_sync(&temp_path, thumbnail_path)?;

    let thumbnail_file = File::open(thumbnail_path).map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to open generated thumbnail '{}': {}",
            thumbnail_path.display(),
            error
        ))
    })?;
    thumbnail_file
        .set_times(FileTimes::new().set_modified(original_modified))
        .map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to bind generated thumbnail '{}' to source mtime: {}",
                thumbnail_path.display(),
                error
            ))
        })?;
    Ok(())
}

pub fn open_thumbnail_or_original_sync(
    mut original_file: File,
    original_path: &Path,
    original_modified: SystemTime,
    thumbnail_path: &Path,
    config: ThumbnailConfig,
) -> Result<OpenThumbnailSource, DomainError> {
    if is_animated_image_sync(&mut original_file, original_path)? {
        return Ok(OpenThumbnailSource::Original(original_file));
    }

    match open_fresh_thumbnail_sync(thumbnail_path, original_modified) {
        Ok(Some(file)) => return Ok(OpenThumbnailSource::CachedJpeg(file)),
        Ok(None) => {}
        Err(error) => {
            tracing::warn!(
                "Failed to inspect thumbnail cache '{}'; serving original '{}': {}",
                thumbnail_path.display(),
                original_path.display(),
                error
            );
            return rewind_original(original_file, original_path);
        }
    }

    if let Err(error) = generate_thumbnail_sync(
        &mut original_file,
        original_path,
        original_modified,
        thumbnail_path,
        config,
    ) {
        tracing::warn!(
            "Failed to materialize thumbnail '{}'; serving original '{}': {}",
            thumbnail_path.display(),
            original_path.display(),
            error
        );
        return rewind_original(original_file, original_path);
    }

    match open_fresh_thumbnail_sync(thumbnail_path, original_modified) {
        Ok(Some(file)) => return Ok(OpenThumbnailSource::CachedJpeg(file)),
        Ok(None) => tracing::warn!(
            "Generated thumbnail cache does not match source mtime '{}'; serving original '{}'",
            thumbnail_path.display(),
            original_path.display()
        ),
        Err(error) => tracing::warn!(
            "Failed to open generated thumbnail '{}'; serving original '{}': {}",
            thumbnail_path.display(),
            original_path.display(),
            error
        ),
    }

    rewind_original(original_file, original_path)
}

fn rewind_original(
    mut original_file: File,
    original_path: &Path,
) -> Result<OpenThumbnailSource, DomainError> {
    original_file.rewind().map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to rewind original image '{}': {}",
            original_path.display(),
            error
        ))
    })?;
    Ok(OpenThumbnailSource::Original(original_file))
}

pub async fn invalidate_thumbnail_cache(thumbnail_path: &Path) -> Result<(), DomainError> {
    match fs::remove_file(thumbnail_path).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(DomainError::InternalError(format!(
            "Failed to remove thumbnail cache '{}': {}",
            thumbnail_path.display(),
            error
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgb};
    use std::path::PathBuf;

    struct TempDirGuard {
        path: PathBuf,
    }

    impl TempDirGuard {
        fn new(test_name: &str) -> Self {
            let mut path = std::env::temp_dir();
            path.push(format!("tauritavern-{test_name}-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&path).expect("create temp dir");
            Self { path }
        }
    }

    impl Drop for TempDirGuard {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    #[cfg(unix)]
    #[test]
    fn thumbnail_generation_does_not_follow_existing_temp_symlink() {
        let temp = TempDirGuard::new("thumbnail-cache-temp-symlink");
        let external = TempDirGuard::new("thumbnail-cache-temp-external");
        let original_path = temp.path.join("source.png");
        let thumbnail_path = temp.path.join("thumb.jpg");
        let old_temp_path = thumbnail_path.with_extension("tmp");
        let outside_path = external.path.join("outside.txt");

        let image = ImageBuffer::from_pixel(2, 2, Rgb([255u8, 0, 0]));
        image.save(&original_path).expect("write source image");
        std::fs::write(&outside_path, b"keep").expect("write outside target");
        std::os::unix::fs::symlink(&outside_path, &old_temp_path).expect("temp symlink");

        let mut original_file = File::open(&original_path).expect("open source image");
        let original_modified = original_file
            .metadata()
            .expect("source metadata")
            .modified()
            .expect("source mtime");
        generate_thumbnail_sync(
            &mut original_file,
            &original_path,
            original_modified,
            &thumbnail_path,
            ThumbnailConfig {
                width: 1,
                height: 1,
                quality: 90,
                resize_mode: ThumbnailResizeMode::Cover,
            },
        )
        .expect("generate thumbnail");

        assert_eq!(std::fs::read(&outside_path).expect("read outside"), b"keep");
        assert!(thumbnail_path.is_file());
        assert_eq!(
            std::fs::metadata(&thumbnail_path)
                .expect("thumbnail metadata")
                .modified()
                .expect("thumbnail mtime"),
            original_modified
        );
    }
}
