use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use std::fs::File;
use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;
use tokio::fs;
use tokio::io::AsyncReadExt;

use tt_adapter_storage_core::file_system::{replace_file_with_fallback_sync, unique_temp_path};
use tt_domain::errors::DomainError;

const ANIMATED_EXTENSIONS: &[&str] = &[".apng", ".mp4", ".webm", ".avi", ".mkv", ".flv", ".gif"];
const THUMBNAIL_CACHE_SCHEMA: &[u8] = b"thumbnail-jpeg-v2\0";
const JPEG_SOI_APP0_PREFIX: [u8; 4] = [0xff, 0xd8, 0xff, 0xe0];
const JPEG_COM_MARKER: [u8; 2] = [0xff, 0xfe];
static THUMBNAIL_COMMIT_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThumbnailResizeMode {
    PreserveArea,
    Cover,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

#[derive(Debug, Clone, Copy)]
pub(crate) struct ThumbnailSourceSnapshot {
    pub content_length: u64,
    pub last_modified: SystemTime,
}

pub(crate) fn thumbnail_cache_identity(source_revision: &[u8], config: ThumbnailConfig) -> Vec<u8> {
    let mut identity = Vec::with_capacity(source_revision.len() + 32);
    identity.extend_from_slice(THUMBNAIL_CACHE_SCHEMA);
    identity.extend_from_slice(&config.width.max(1).to_be_bytes());
    identity.extend_from_slice(&config.height.max(1).to_be_bytes());
    identity.push(config.quality.clamp(1, 100));
    identity.push(match config.resize_mode {
        ThumbnailResizeMode::PreserveArea => 0,
        ThumbnailResizeMode::Cover => 1,
    });
    identity.extend_from_slice(source_revision);
    identity
}

fn jpeg_comment_length(cache_identity: &[u8]) -> Result<u16, DomainError> {
    u16::try_from(cache_identity.len() + 2).map_err(|_| {
        DomainError::InternalError("Thumbnail cache identity exceeds JPEG COM capacity".to_string())
    })
}

fn embed_cache_identity(jpeg: &mut Vec<u8>, cache_identity: &[u8]) -> Result<(), DomainError> {
    let Some(header) = jpeg.get(..6) else {
        return Err(DomainError::InternalError(
            "JPEG encoder returned a truncated thumbnail".to_string(),
        ));
    };
    if header[..4] != JPEG_SOI_APP0_PREFIX {
        return Err(DomainError::InternalError(
            "JPEG encoder returned an unexpected header".to_string(),
        ));
    }

    let app0_length = usize::from(u16::from_be_bytes([header[4], header[5]]));
    let insert_at = 4usize.checked_add(app0_length).ok_or_else(|| {
        DomainError::InternalError("JPEG APP0 segment length overflowed".to_string())
    })?;
    if app0_length < 2 || insert_at > jpeg.len() {
        return Err(DomainError::InternalError(
            "JPEG encoder returned an invalid APP0 segment".to_string(),
        ));
    }

    let comment_length = jpeg_comment_length(cache_identity)?;
    let mut comment = Vec::with_capacity(cache_identity.len() + 4);
    comment.extend_from_slice(&JPEG_COM_MARKER);
    comment.extend_from_slice(&comment_length.to_be_bytes());
    comment.extend_from_slice(cache_identity);
    jpeg.splice(insert_at..insert_at, comment);
    Ok(())
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
    buffer.starts_with(b"RIFF")
        && buffer.get(8..16) == Some(b"WEBPVP8X")
        && buffer.get(20).is_some_and(|flags| flags & 0b10 != 0)
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

fn read_exact_or_cache_miss(
    file: &mut File,
    buffer: &mut [u8],
    thumbnail_path: &Path,
) -> Result<bool, DomainError> {
    match file.read_exact(buffer) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => Ok(false),
        Err(error) => Err(DomainError::InternalError(format!(
            "Failed to inspect thumbnail cache '{}': {}",
            thumbnail_path.display(),
            error
        ))),
    }
}

fn cache_identity_matches(
    file: &mut File,
    thumbnail_path: &Path,
    cache_identity: &[u8],
) -> Result<bool, DomainError> {
    let mut header = [0u8; 6];
    if !read_exact_or_cache_miss(file, &mut header, thumbnail_path)?
        || header[..4] != JPEG_SOI_APP0_PREFIX
    {
        return Ok(false);
    }

    let app0_length = u64::from(u16::from_be_bytes([header[4], header[5]]));
    if app0_length < 2 {
        return Ok(false);
    }
    file.seek(std::io::SeekFrom::Start(4 + app0_length))
        .map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to inspect thumbnail cache '{}': {}",
                thumbnail_path.display(),
                error
            ))
        })?;

    let mut comment_header = [0u8; 4];
    if !read_exact_or_cache_miss(file, &mut comment_header, thumbnail_path)?
        || comment_header[..2] != JPEG_COM_MARKER
        || u16::from_be_bytes([comment_header[2], comment_header[3]])
            != jpeg_comment_length(cache_identity)?
    {
        return Ok(false);
    }

    let mut stored_identity = vec![0u8; cache_identity.len()];
    if !read_exact_or_cache_miss(file, &mut stored_identity, thumbnail_path)?
        || stored_identity != cache_identity
    {
        return Ok(false);
    }

    file.rewind().map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to rewind thumbnail cache '{}': {}",
            thumbnail_path.display(),
            error
        ))
    })?;
    Ok(true)
}

fn open_fresh_thumbnail_unlocked(
    thumbnail_path: &Path,
    cache_identity: &[u8],
) -> Result<Option<File>, DomainError> {
    let mut file = match File::open(thumbnail_path) {
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
    if !cache_identity_matches(&mut file, thumbnail_path, cache_identity)? {
        return Ok(None);
    }
    Ok(Some(file))
}

fn ensure_source_unchanged(
    original_file: &File,
    original_path: &Path,
    source_snapshot: ThumbnailSourceSnapshot,
) -> Result<(), DomainError> {
    let metadata = original_file.metadata().map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to inspect source image '{}': {}",
            original_path.display(),
            error
        ))
    })?;
    let last_modified = metadata.modified().map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to read source image mtime '{}': {}",
            original_path.display(),
            error
        ))
    })?;
    if metadata.len() != source_snapshot.content_length
        || last_modified != source_snapshot.last_modified
    {
        return Err(DomainError::InternalError(format!(
            "Source image changed while materializing thumbnail: {}",
            original_path.display()
        )));
    }
    Ok(())
}

fn open_fresh_thumbnail_sync(
    thumbnail_path: &Path,
    cache_identity: &[u8],
) -> Result<Option<File>, DomainError> {
    let _cache = THUMBNAIL_COMMIT_LOCK.lock().map_err(|_| {
        DomainError::InternalError("Thumbnail cache commit lock is poisoned".to_string())
    })?;
    open_fresh_thumbnail_unlocked(thumbnail_path, cache_identity)
}

fn write_temp_file(target_path: &Path, bytes: &[u8]) -> Result<PathBuf, DomainError> {
    let temp_path = unique_temp_path(target_path);
    let mut temp_file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp_path)
        .map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to create temporary thumbnail file '{}': {}",
                temp_path.display(),
                error
            ))
        })?;
    temp_file.write_all(bytes).map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to write temporary thumbnail file '{}': {}",
            temp_path.display(),
            error
        ))
    })?;
    Ok(temp_path)
}

fn generate_thumbnail_sync(
    original_file: &mut File,
    original_path: &Path,
    source_snapshot: ThumbnailSourceSnapshot,
    thumbnail_path: &Path,
    config: ThumbnailConfig,
    cache_identity: &[u8],
) -> Result<(), DomainError> {
    ensure_source_unchanged(original_file, original_path, source_snapshot)?;
    original_file.rewind().map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to seek source image '{}': {}",
            original_path.display(),
            error
        ))
    })?;
    let mut source_bytes = Vec::new();
    (&mut *original_file)
        .take(source_snapshot.content_length.saturating_add(1))
        .read_to_end(&mut source_bytes)
        .map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read source image '{}': {}",
                original_path.display(),
                error
            ))
        })?;
    ensure_source_unchanged(original_file, original_path, source_snapshot)?;
    if source_bytes.len() as u64 != source_snapshot.content_length {
        return Err(DomainError::InternalError(format!(
            "Source image changed while materializing thumbnail: {}",
            original_path.display()
        )));
    }

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
    embed_cache_identity(&mut encoded, cache_identity)?;

    if let Some(parent) = thumbnail_path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to ensure thumbnail directory '{}': {}",
                parent.display(),
                error
            ))
        })?;
    }

    let thumbnail_temp_path = write_temp_file(thumbnail_path, &encoded)?;

    let _commit = THUMBNAIL_COMMIT_LOCK.lock().map_err(|_| {
        DomainError::InternalError("Thumbnail cache commit lock is poisoned".to_string())
    })?;
    if open_fresh_thumbnail_unlocked(thumbnail_path, cache_identity)?.is_some() {
        let _ = std::fs::remove_file(thumbnail_temp_path);
        return Ok(());
    }

    replace_file_with_fallback_sync(&thumbnail_temp_path, thumbnail_path)?;
    Ok(())
}

pub fn open_thumbnail_or_original_sync(
    mut original_file: File,
    original_path: &Path,
    source_snapshot: ThumbnailSourceSnapshot,
    thumbnail_path: &Path,
    config: ThumbnailConfig,
    cache_identity: &[u8],
    require_generated: bool,
) -> Result<OpenThumbnailSource, DomainError> {
    if !require_generated && is_animated_image_sync(&mut original_file, original_path)? {
        return Ok(OpenThumbnailSource::Original(original_file));
    }

    match open_fresh_thumbnail_sync(thumbnail_path, cache_identity) {
        Ok(Some(file)) => {
            if let Err(error) =
                ensure_source_unchanged(&original_file, original_path, source_snapshot)
            {
                if require_generated {
                    return Err(error);
                }
                tracing::warn!("{}; serving original", error);
                return rewind_original(original_file, original_path);
            }
            return Ok(OpenThumbnailSource::CachedJpeg(file));
        }
        Ok(None) => {}
        Err(error) => {
            if require_generated {
                return Err(error);
            }
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
        source_snapshot,
        thumbnail_path,
        config,
        cache_identity,
    ) {
        if require_generated {
            return Err(error);
        }
        tracing::warn!(
            "Failed to materialize thumbnail '{}'; serving original '{}': {}",
            thumbnail_path.display(),
            original_path.display(),
            error
        );
        return rewind_original(original_file, original_path);
    }

    match open_fresh_thumbnail_sync(thumbnail_path, cache_identity) {
        Ok(Some(file)) => return Ok(OpenThumbnailSource::CachedJpeg(file)),
        Ok(None) => {
            let error = DomainError::InternalError(format!(
                "Generated thumbnail cache identity was not committed: {}",
                thumbnail_path.display()
            ));
            if require_generated {
                return Err(error);
            }
            tracing::warn!("{}; serving original '{}'", error, original_path.display());
        }
        Err(error) => {
            if require_generated {
                return Err(error);
            }
            tracing::warn!(
                "Failed to open generated thumbnail '{}'; serving original '{}': {}",
                thumbnail_path.display(),
                original_path.display(),
                error
            );
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgb};
    use std::path::PathBuf;

    fn source_snapshot(path: &Path) -> ThumbnailSourceSnapshot {
        let metadata = std::fs::metadata(path).expect("source metadata");
        ThumbnailSourceSnapshot {
            content_length: metadata.len(),
            last_modified: metadata.modified().expect("source mtime"),
        }
    }

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
        let config = ThumbnailConfig {
            width: 1,
            height: 1,
            quality: 90,
            resize_mode: ThumbnailResizeMode::Cover,
        };
        let identity = thumbnail_cache_identity(b"source-v1", config);
        generate_thumbnail_sync(
            &mut original_file,
            &original_path,
            source_snapshot(&original_path),
            &thumbnail_path,
            config,
            &identity,
        )
        .expect("generate thumbnail");

        assert_eq!(std::fs::read(&outside_path).expect("read outside"), b"keep");
        assert!(thumbnail_path.is_file());
        let thumbnail_bytes = std::fs::read(&thumbnail_path).expect("read thumbnail");
        image::load_from_memory(&thumbnail_bytes).expect("decode thumbnail with identity comment");
        assert!(
            open_fresh_thumbnail_sync(&thumbnail_path, &identity)
                .expect("open thumbnail cache")
                .is_some()
        );
        assert!(
            open_fresh_thumbnail_sync(&thumbnail_path, b"different identity")
                .expect("inspect mismatched thumbnail cache")
                .is_none()
        );
    }

    #[test]
    fn external_jpeg_without_embedded_identity_is_a_cache_miss() {
        let temp = TempDirGuard::new("thumbnail-cache-external-jpeg");
        let thumbnail_path = temp.path.join("thumb.jpg");
        ImageBuffer::from_pixel(2, 2, Rgb([0u8, 0, 255]))
            .save(&thumbnail_path)
            .expect("write external jpeg");

        assert!(
            open_fresh_thumbnail_sync(&thumbnail_path, b"expected identity")
                .expect("inspect external jpeg")
                .is_none()
        );
    }

    #[test]
    fn source_change_prevents_thumbnail_commit() {
        let temp = TempDirGuard::new("thumbnail-cache-source-change");
        let original_path = temp.path.join("source.png");
        let thumbnail_path = temp.path.join("thumb.jpg");
        ImageBuffer::from_pixel(2, 2, Rgb([255u8, 0, 0]))
            .save(&original_path)
            .expect("write source image");
        let stale_snapshot = source_snapshot(&original_path);
        ImageBuffer::from_pixel(3, 2, Rgb([0u8, 0, 255]))
            .save(&original_path)
            .expect("replace source image");

        let mut original_file = File::open(&original_path).expect("open replacement source");
        let config = ThumbnailConfig {
            width: 1,
            height: 1,
            quality: 90,
            resize_mode: ThumbnailResizeMode::Cover,
        };
        let error = generate_thumbnail_sync(
            &mut original_file,
            &original_path,
            stale_snapshot,
            &thumbnail_path,
            config,
            &thumbnail_cache_identity(b"stale-source", config),
        )
        .expect_err("changed source must not be cached under a stale identity");

        assert!(error.to_string().contains("Source image changed"));
        assert!(!thumbnail_path.exists());
    }

    #[test]
    fn cache_identity_binds_source_and_effective_recipe() {
        let config = ThumbnailConfig {
            width: 160,
            height: 90,
            quality: 90,
            resize_mode: ThumbnailResizeMode::PreserveArea,
        };
        let original = thumbnail_cache_identity(b"source-a", config);

        assert_ne!(original, thumbnail_cache_identity(b"source-b", config));
        assert_ne!(
            original,
            thumbnail_cache_identity(
                b"source-a",
                ThumbnailConfig {
                    width: 320,
                    ..config
                }
            )
        );
        assert_ne!(
            original,
            thumbnail_cache_identity(
                b"source-a",
                ThumbnailConfig {
                    resize_mode: ThumbnailResizeMode::Cover,
                    ..config
                }
            )
        );
    }

    #[test]
    fn animated_webp_detection_uses_the_vp8x_feature_flag() {
        let mut header = vec![0u8; 512];
        header[..4].copy_from_slice(b"RIFF");
        header[8..16].copy_from_slice(b"WEBPVP8X");
        header[20] = 0b10;

        assert!(is_animated_webp_header(&header));
        header[20] = 0;
        assert!(!is_animated_webp_header(&header));
    }
}
