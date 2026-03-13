use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use mime_guess::from_path;
use std::path::Path;
use tokio::fs;
use tokio::io::AsyncReadExt;

use crate::domain::errors::DomainError;

const ANIMATED_EXTENSIONS: &[&str] = &[".apng", ".mp4", ".webm", ".avi", ".mkv", ".flv", ".gif"];

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

#[derive(Debug, Clone)]
pub struct ThumbnailAsset {
    pub bytes: Vec<u8>,
    pub mime_type: String,
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
    let mut file = fs::File::open(path).await.map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to inspect image header '{}': {}",
            path.display(),
            error
        ))
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

fn read_image_header_sync(path: &Path) -> Result<Vec<u8>, DomainError> {
    use std::io::Read;

    let mut file = std::fs::File::open(path).map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to inspect image header '{}': {}",
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

pub fn is_animated_image_sync(path: &Path) -> Result<bool, DomainError> {
    let extension = extension_lowercase(path);
    if ANIMATED_EXTENSIONS.contains(&extension.as_str()) {
        return Ok(true);
    }

    if extension == ".png" {
        let header = read_image_header_sync(path)?;
        return Ok(is_apng_header(&header));
    }

    if extension == ".webp" {
        let header = read_image_header_sync(path)?;
        return Ok(is_animated_webp_header(&header));
    }

    Ok(false)
}

fn read_original_asset_sync(original_path: &Path) -> Result<ThumbnailAsset, DomainError> {
    let bytes = std::fs::read(original_path).map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to read original image '{}': {}",
            original_path.display(),
            error
        ))
    })?;

    let mime_type = from_path(original_path)
        .first_or_octet_stream()
        .essence_str()
        .to_string();

    Ok(ThumbnailAsset { bytes, mime_type })
}

fn thumbnail_is_fresh_sync(thumbnail_path: &Path, original_path: &Path) -> Result<bool, DomainError> {
    let thumbnail_metadata = match std::fs::metadata(thumbnail_path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(DomainError::InternalError(format!(
                "Failed to read thumbnail metadata '{}': {}",
                thumbnail_path.display(),
                error
            )));
        }
    };

    let original_metadata = std::fs::metadata(original_path).map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to read original image metadata '{}': {}",
            original_path.display(),
            error
        ))
    })?;

    let Some(original_modified) = original_metadata.modified().ok() else {
        return Ok(false);
    };
    let Some(thumbnail_modified) = thumbnail_metadata.modified().ok() else {
        return Ok(false);
    };

    Ok(original_modified <= thumbnail_modified)
}

fn generate_thumbnail_sync(
    original_path: &Path,
    thumbnail_path: &Path,
    config: ThumbnailConfig,
) -> Result<(), DomainError> {
    let source_bytes = std::fs::read(original_path).map_err(|error| {
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
        ThumbnailResizeMode::Cover => source_image.resize_to_fill(width, height, FilterType::Triangle),
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

    let temp_path = thumbnail_path.with_extension("tmp");
    std::fs::write(&temp_path, &encoded).map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to write temporary thumbnail '{}': {}",
            temp_path.display(),
            error
        ))
    })?;

    match std::fs::remove_file(thumbnail_path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(DomainError::InternalError(format!(
                "Failed to replace thumbnail '{}': {}",
                thumbnail_path.display(),
                error
            )));
        }
    }

    std::fs::rename(&temp_path, thumbnail_path).map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to finalize thumbnail '{}': {}",
            thumbnail_path.display(),
            error
        ))
    })
}

fn ensure_thumbnail_sync(
    original_path: &Path,
    thumbnail_path: &Path,
    config: ThumbnailConfig,
) -> Result<(), DomainError> {
    if thumbnail_is_fresh_sync(thumbnail_path, original_path)? {
        return Ok(());
    }

    generate_thumbnail_sync(original_path, thumbnail_path, config)
}

pub async fn read_thumbnail_or_original(
    original_path: &Path,
    thumbnail_path: &Path,
    config: ThumbnailConfig,
) -> Result<ThumbnailAsset, DomainError> {
    let original_path = original_path.to_path_buf();
    let thumbnail_path = thumbnail_path.to_path_buf();
    let task = tokio::task::spawn_blocking(move || {
        read_thumbnail_or_original_sync(&original_path, &thumbnail_path, config)
    });

    task.await.map_err(|error| {
        DomainError::InternalError(format!("Thumbnail worker failed: {}", error))
    })?
}

pub fn read_thumbnail_or_original_sync(
    original_path: &Path,
    thumbnail_path: &Path,
    config: ThumbnailConfig,
) -> Result<ThumbnailAsset, DomainError> {
    let original_metadata = std::fs::metadata(original_path).map_err(|error| match error.kind() {
        std::io::ErrorKind::NotFound => DomainError::NotFound(format!(
            "Source image not found: {}",
            original_path.display()
        )),
        _ => DomainError::InternalError(format!(
            "Failed to read source image metadata '{}': {}",
            original_path.display(),
            error
        )),
    })?;

    if !original_metadata.is_file() {
        return Err(DomainError::NotFound(format!(
            "Source image not found: {}",
            original_path.display()
        )));
    }

    if is_animated_image_sync(original_path)? {
        return read_original_asset_sync(original_path);
    }

    if ensure_thumbnail_sync(original_path, thumbnail_path, config).is_err() {
        return read_original_asset_sync(original_path);
    }

    match std::fs::read(thumbnail_path) {
        Ok(bytes) => Ok(ThumbnailAsset {
            bytes,
            mime_type: "image/jpeg".to_string(),
        }),
        Err(_) => read_original_asset_sync(original_path),
    }
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
