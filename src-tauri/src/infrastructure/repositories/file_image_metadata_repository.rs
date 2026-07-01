use async_trait::async_trait;
use mime_guess::from_path;
use std::collections::{HashMap, HashSet};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::fs;
use tokio::sync::Mutex;

use crate::infrastructure::persistence::file_system::write_json_file;
use crate::infrastructure::persistence::thumbnail_cache::is_animated_image;
use crate::infrastructure::thumbnails::{BACKGROUND_THUMBNAIL_HEIGHT, BACKGROUND_THUMBNAIL_WIDTH};
use tt_domain::errors::DomainError;
use tt_domain::models::background::BackgroundListEntry;
use tt_domain::models::image_metadata::{
    BackgroundFoldersPayload, ImageMetadata, ImageMetadataFolder, ImageMetadataIndex,
};
use tt_ports::repositories::image_metadata_repository::ImageMetadataRepository;

const METADATA_FILE_NAME: &str = "image-metadata.json";
const BACKGROUNDS_PREFIX: &str = "backgrounds/";
const THUMBNAIL_RESOLUTION: u32 = BACKGROUND_THUMBNAIL_WIDTH * BACKGROUND_THUMBNAIL_HEIGHT;

pub struct FileImageMetadataRepository {
    user_root: PathBuf,
    backgrounds_dir: PathBuf,
    lock: Mutex<()>,
}

#[derive(Debug)]
enum BackgroundMetadataBuildError {
    Skippable(String),
    Fatal(DomainError),
}

impl BackgroundMetadataBuildError {
    fn skippable(message: impl Into<String>) -> Self {
        Self::Skippable(message.into())
    }

    fn from_metadata_error(path: &Path, error: std::io::Error) -> Self {
        if error.kind() == ErrorKind::NotFound {
            return Self::skippable(format!(
                "Background disappeared before metadata refresh '{}': {}",
                path.display(),
                error
            ));
        }

        Self::Fatal(DomainError::InternalError(format!(
            "Failed to read background metadata '{}': {}",
            path.display(),
            error
        )))
    }

    fn from_dimension_reader_join(path: &Path, error: tokio::task::JoinError) -> Self {
        Self::Fatal(DomainError::InternalError(format!(
            "Failed to join background dimension reader '{}': {}",
            path.display(),
            error
        )))
    }

    fn from_dimension_error(path: &Path, error: image::ImageError) -> Self {
        match error {
            image::ImageError::IoError(error) => match error.kind() {
                ErrorKind::NotFound => Self::skippable(format!(
                    "Background disappeared before dimension refresh '{}': {}",
                    path.display(),
                    error
                )),
                ErrorKind::InvalidData | ErrorKind::UnexpectedEof => Self::skippable(format!(
                    "Invalid background image '{}': {}",
                    path.display(),
                    error
                )),
                _ => Self::Fatal(DomainError::InternalError(format!(
                    "Failed to read background dimensions '{}': {}",
                    path.display(),
                    error
                ))),
            },
            _ => Self::skippable(format!(
                "Failed to read background dimensions '{}': {}",
                path.display(),
                error
            )),
        }
    }

    fn invalid_dimensions(path: &Path) -> Self {
        Self::skippable(format!(
            "Invalid background dimensions '{}'",
            path.display()
        ))
    }

    fn from_animation_error(error: DomainError) -> Self {
        match error {
            DomainError::NotFound(message) | DomainError::InvalidData(message) => {
                Self::skippable(message)
            }
            error => Self::Fatal(error),
        }
    }
}

impl FileImageMetadataRepository {
    pub fn new(user_root: PathBuf, backgrounds_dir: PathBuf) -> Self {
        Self {
            user_root,
            backgrounds_dir,
            lock: Mutex::new(()),
        }
    }

    fn metadata_path(&self) -> PathBuf {
        self.user_root.join(METADATA_FILE_NAME)
    }

    async fn read_index(&self) -> Result<ImageMetadataIndex, DomainError> {
        let path = self.metadata_path();
        let raw = match fs::read_to_string(&path).await {
            Ok(value) => value,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(ImageMetadataIndex::default());
            }
            Err(error) => {
                return Err(DomainError::InternalError(format!(
                    "Failed to read image metadata '{}': {}",
                    path.display(),
                    error
                )));
            }
        };

        serde_json::from_str::<ImageMetadataIndex>(&raw).map_err(|error| {
            DomainError::InvalidData(format!(
                "Invalid image metadata JSON '{}': {}",
                path.display(),
                error
            ))
        })
    }

    async fn write_index(&self, index: &ImageMetadataIndex) -> Result<(), DomainError> {
        write_json_file(&self.metadata_path(), index).await
    }

    async fn ensure_backgrounds_dir_exists(&self) -> Result<(), DomainError> {
        if fs::try_exists(&self.backgrounds_dir)
            .await
            .map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to check backgrounds directory '{}': {}",
                    self.backgrounds_dir.display(),
                    error
                ))
            })?
        {
            return Ok(());
        }

        fs::create_dir_all(&self.backgrounds_dir)
            .await
            .map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to create backgrounds directory '{}': {}",
                    self.backgrounds_dir.display(),
                    error
                ))
            })
    }

    fn is_image(path: &Path) -> bool {
        from_path(path)
            .first()
            .is_some_and(|mime| mime.type_() == "image")
    }

    fn normalize_background_filename(filename: &str) -> Result<String, DomainError> {
        let value = filename.trim();
        if value.is_empty()
            || value.contains('/')
            || value.contains('\\')
            || value.chars().any(char::is_control)
            || value.ends_with('.')
            || value.ends_with(' ')
        {
            return Err(DomainError::InvalidData(format!(
                "Invalid background filename: {}",
                filename
            )));
        }

        Ok(value.to_string())
    }

    fn strict_background_relative_path(filename: &str) -> Result<String, DomainError> {
        Ok(format!(
            "{}{}",
            BACKGROUNDS_PREFIX,
            Self::normalize_background_filename(filename)?
        ))
    }

    fn existing_background_asset_relative_path(filename: &str) -> Result<String, DomainError> {
        if !Self::validate_background_asset_path_segment(filename) {
            return Err(DomainError::InvalidData(format!(
                "Invalid background filename: {}",
                filename
            )));
        }

        Ok(format!("{BACKGROUNDS_PREFIX}{filename}"))
    }

    fn normalize_background_relative_path(path: &str) -> Result<String, DomainError> {
        let raw = path.trim().replace('\\', "/");
        if raw.is_empty() || raw.starts_with('/') {
            return Err(DomainError::InvalidData(format!(
                "Invalid background path: {}",
                path
            )));
        }

        let parts = raw.split('/').collect::<Vec<_>>();
        if parts.len() < 2
            || parts[0] != "backgrounds"
            || parts
                .iter()
                .any(|part| !Self::validate_background_asset_path_segment(part))
        {
            return Err(DomainError::InvalidData(format!(
                "Invalid background path: {}",
                path
            )));
        }

        Ok(parts.join("/"))
    }

    fn is_forbidden_background_asset_path_segment_char(character: char) -> bool {
        matches!(
            character,
            '\u{0000}'..='\u{001F}' | '\u{007F}' | ':' | '*' | '?' | '"' | '<' | '>' | '|'
        )
    }

    fn validate_background_asset_path_segment(segment: &str) -> bool {
        !(segment.is_empty()
            || segment == "."
            || segment == ".."
            || segment.contains('/')
            || segment.contains('\\')
            || segment
                .chars()
                .any(Self::is_forbidden_background_asset_path_segment_char))
    }

    fn filename_from_background_relative_path(relative_path: &str) -> String {
        relative_path
            .rsplit('/')
            .next()
            .unwrap_or(relative_path)
            .to_string()
    }

    async fn list_background_files(&self) -> Result<Vec<(String, PathBuf)>, DomainError> {
        self.ensure_backgrounds_dir_exists().await?;

        let mut entries = fs::read_dir(&self.backgrounds_dir).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read backgrounds directory '{}': {}",
                self.backgrounds_dir.display(),
                error
            ))
        })?;

        let mut files = Vec::new();
        while let Some(entry) = entries.next_entry().await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read backgrounds directory entry '{}': {}",
                self.backgrounds_dir.display(),
                error
            ))
        })? {
            let path = entry.path();
            let file_type = entry.file_type().await.map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to read backgrounds directory entry type '{}': {}",
                    path.display(),
                    error
                ))
            })?;
            if !file_type.is_file() || !Self::is_image(&path) {
                continue;
            }

            let Some(file_name) = path.file_name().and_then(|value| value.to_str()) else {
                tracing::warn!(
                    "[ImageMetadata] Skipping background with non UTF-8 filename: '{}'",
                    path.display()
                );
                continue;
            };

            let relative_path = match Self::existing_background_asset_relative_path(file_name) {
                Ok(value) => value,
                Err(error) => {
                    tracing::warn!(
                        "[ImageMetadata] Skipping background with unsupported filename '{}': {}",
                        path.display(),
                        error
                    );
                    continue;
                }
            };
            files.push((relative_path, path));
        }

        files.sort_by(|(left, _), (right, _)| left.cmp(right));
        Ok(files)
    }

    fn system_time_to_timestamp_millis(time: SystemTime) -> Option<i64> {
        let millis = time.duration_since(UNIX_EPOCH).ok()?.as_millis();
        i64::try_from(millis).ok()
    }

    fn system_time_to_mtime_millis(time: SystemTime) -> Option<f64> {
        let millis = time.duration_since(UNIX_EPOCH).ok()?.as_secs_f64() * 1000.0;
        Some(millis)
    }

    fn file_added_timestamp_millis(metadata: &std::fs::Metadata) -> i64 {
        metadata
            .created()
            .ok()
            .and_then(Self::system_time_to_timestamp_millis)
            .or_else(|| {
                metadata
                    .modified()
                    .ok()
                    .and_then(Self::system_time_to_timestamp_millis)
            })
            .unwrap_or_else(|| chrono::Utc::now().timestamp_millis())
    }

    fn round_aspect_ratio(value: f64) -> f64 {
        (value * 10_000.0).round() / 10_000.0
    }

    fn is_metadata_fresh(metadata: &ImageMetadata, mtime: f64) -> bool {
        let mtime_matches = metadata
            .mtime
            .is_some_and(|cached| (cached - mtime).abs() < 0.5);
        mtime_matches
            && metadata.aspect_ratio.is_some()
            && metadata.is_animated.is_some()
            && metadata.thumbnail_resolution == Some(THUMBNAIL_RESOLUTION)
    }

    async fn build_background_metadata(
        path: &Path,
        cached: Option<&ImageMetadata>,
    ) -> Result<ImageMetadata, BackgroundMetadataBuildError> {
        let file_metadata = fs::metadata(path)
            .await
            .map_err(|error| BackgroundMetadataBuildError::from_metadata_error(path, error))?;

        let mtime = file_metadata
            .modified()
            .ok()
            .and_then(Self::system_time_to_mtime_millis)
            .unwrap_or(0.0);

        if let Some(cached) = cached {
            if Self::is_metadata_fresh(cached, mtime) {
                return Ok(cached.clone());
            }
        }

        let dimensions_path = path.to_path_buf();
        let (width, height) =
            tokio::task::spawn_blocking(move || image::image_dimensions(&dimensions_path))
                .await
                .map_err(|error| {
                    BackgroundMetadataBuildError::from_dimension_reader_join(path, error)
                })?
                .map_err(|error| BackgroundMetadataBuildError::from_dimension_error(path, error))?;
        if width == 0 || height == 0 {
            return Err(BackgroundMetadataBuildError::invalid_dimensions(path));
        }

        let is_animated = is_animated_image(path)
            .await
            .map_err(BackgroundMetadataBuildError::from_animation_error)?;
        let same_file = cached
            .and_then(|metadata| metadata.mtime)
            .is_some_and(|cached_mtime| (cached_mtime - mtime).abs() < 0.5);
        let folder_ids = cached
            .map(|metadata| metadata.folder_ids.clone())
            .unwrap_or_default();
        let extra = cached
            .map(|metadata| metadata.extra.clone())
            .unwrap_or_default();

        Ok(ImageMetadata {
            hash: if same_file {
                cached.and_then(|metadata| metadata.hash.clone())
            } else {
                None
            },
            aspect_ratio: Some(Self::round_aspect_ratio((width as f64) / (height as f64))),
            is_animated: Some(is_animated),
            dominant_color: if is_animated {
                Some("#808080".to_string())
            } else if same_file {
                cached.and_then(|metadata| metadata.dominant_color.clone())
            } else {
                None
            },
            folder_ids,
            added_timestamp: Some(Self::file_added_timestamp_millis(&file_metadata)),
            thumbnail_resolution: Some(THUMBNAIL_RESOLUTION),
            mtime: Some(mtime),
            extra,
        })
    }

    async fn background_file_exists(&self, relative_path: &str) -> Result<bool, DomainError> {
        let full_path = self.user_root.join(relative_path);
        let metadata = match fs::metadata(&full_path).await {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => {
                return Err(DomainError::InternalError(format!(
                    "Failed to read background file '{}': {}",
                    full_path.display(),
                    error
                )));
            }
        };

        if metadata.is_file() {
            return Ok(true);
        }

        Err(DomainError::InvalidData(format!(
            "Background path is not a file: {}",
            relative_path
        )))
    }

    async fn ensure_background_file_exists(&self, relative_path: &str) -> Result<(), DomainError> {
        if self.background_file_exists(relative_path).await? {
            return Ok(());
        }

        Err(DomainError::NotFound(format!(
            "Background file not found: {}",
            relative_path
        )))
    }

    fn validate_folder_id(id: &str) -> Result<(), DomainError> {
        if id.is_empty() {
            return Err(DomainError::InvalidData(
                "Folder id is required".to_string(),
            ));
        }

        Ok(())
    }

    fn folder_mut<'a>(
        index: &'a mut ImageMetadataIndex,
        id: &str,
    ) -> Option<&'a mut ImageMetadataFolder> {
        index.folders.iter_mut().find(|folder| folder.id == id)
    }

    fn require_folder<'a>(
        index: &'a mut ImageMetadataIndex,
        id: &str,
    ) -> Result<&'a mut ImageMetadataFolder, DomainError> {
        Self::validate_folder_id(id)?;

        Self::folder_mut(index, id)
            .ok_or_else(|| DomainError::NotFound(format!("Folder not found: {}", id)))
    }

    fn clear_folder_thumbnail_references(index: &mut ImageMetadataIndex, filename: &str) -> bool {
        let mut modified = false;
        for folder in &mut index.folders {
            if folder.thumbnail_file == filename {
                folder.thumbnail_file.clear();
                modified = true;
            }
        }
        modified
    }

    fn cleanup_missing_backgrounds(
        index: &mut ImageMetadataIndex,
        existing_paths: &HashSet<String>,
    ) -> bool {
        let mut removed_filenames = Vec::new();
        index.images.retain(|relative_path, _metadata| {
            let should_keep = !relative_path.starts_with(BACKGROUNDS_PREFIX)
                || existing_paths.contains(relative_path);
            if !should_keep {
                removed_filenames.push(Self::filename_from_background_relative_path(relative_path));
            }
            should_keep
        });

        let mut cleared_thumbnail = false;
        for filename in &removed_filenames {
            cleared_thumbnail |= Self::clear_folder_thumbnail_references(index, filename);
        }

        !removed_filenames.is_empty() || cleared_thumbnail
    }

    fn filter_index_by_prefix(index: &mut ImageMetadataIndex, prefix: Option<&str>) {
        let prefix = prefix.unwrap_or_default().trim();
        if prefix.is_empty() {
            return;
        }

        index
            .images
            .retain(|relative_path, _metadata| relative_path.starts_with(prefix));
    }

    async fn refresh_backgrounds_in_index(
        &self,
        index: &mut ImageMetadataIndex,
    ) -> Result<(Vec<String>, bool), DomainError> {
        let files = self.list_background_files().await?;
        let existing_paths = files
            .iter()
            .map(|(relative_path, _)| relative_path.clone())
            .collect::<HashSet<_>>();

        let mut modified = Self::cleanup_missing_backgrounds(index, &existing_paths);
        let mut relative_paths = Vec::with_capacity(files.len());

        for (relative_path, path) in files {
            let metadata_result = {
                let cached = index.images.get(&relative_path);
                Self::build_background_metadata(&path, cached).await
            };

            match metadata_result {
                Ok(metadata) => {
                    if index.images.get(&relative_path) != Some(&metadata) {
                        index.images.insert(relative_path.clone(), metadata);
                        modified = true;
                    }
                }
                Err(BackgroundMetadataBuildError::Skippable(message)) => {
                    tracing::warn!(
                        "[ImageMetadata] Failed to refresh background metadata '{}': {}",
                        path.display(),
                        message
                    );
                }
                Err(BackgroundMetadataBuildError::Fatal(error)) => return Err(error),
            }

            relative_paths.push(relative_path);
        }

        Ok((relative_paths, modified))
    }
}

#[async_trait]
impl ImageMetadataRepository for FileImageMetadataRepository {
    async fn read_metadata_index(
        &self,
        prefix: Option<&str>,
    ) -> Result<ImageMetadataIndex, DomainError> {
        let _guard = self.lock.lock().await;
        let mut index = self.read_index().await?;

        Self::filter_index_by_prefix(&mut index, prefix);
        Ok(index)
    }

    async fn get_background_list_entries(&self) -> Result<Vec<BackgroundListEntry>, DomainError> {
        let _guard = self.lock.lock().await;
        let mut index = self.read_index().await?;
        let (relative_paths, modified) = self.refresh_backgrounds_in_index(&mut index).await?;

        if modified {
            self.write_index(&index).await?;
        }

        Ok(relative_paths
            .into_iter()
            .map(|relative_path| {
                let is_animated = index
                    .images
                    .get(&relative_path)
                    .and_then(|metadata| metadata.is_animated)
                    .unwrap_or(false);
                BackgroundListEntry {
                    filename: Self::filename_from_background_relative_path(&relative_path),
                    is_animated,
                }
            })
            .collect())
    }

    async fn get_background_folders(&self) -> Result<BackgroundFoldersPayload, DomainError> {
        let _guard = self.lock.lock().await;
        let index = self.read_index().await?;

        let mut image_folder_map = HashMap::new();
        for (relative_path, metadata) in &index.images {
            if !relative_path.starts_with(BACKGROUNDS_PREFIX) || metadata.folder_ids.is_empty() {
                continue;
            }

            image_folder_map.insert(
                Self::filename_from_background_relative_path(relative_path),
                metadata.folder_ids.clone(),
            );
        }

        Ok(BackgroundFoldersPayload {
            folders: index.folders,
            image_folder_map,
        })
    }

    async fn create_folder(&self, name: &str) -> Result<ImageMetadataFolder, DomainError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(DomainError::InvalidData(
                "Folder name is required".to_string(),
            ));
        }

        let _guard = self.lock.lock().await;
        let mut index = self.read_index().await?;
        let folder = ImageMetadataFolder {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.to_string(),
            thumbnail_file: String::new(),
        };
        index.folders.push(folder.clone());
        self.write_index(&index).await?;
        Ok(folder)
    }

    async fn update_folder(
        &self,
        id: &str,
        name: Option<&str>,
        thumbnail_file: Option<&str>,
    ) -> Result<ImageMetadataFolder, DomainError> {
        let _guard = self.lock.lock().await;
        let mut index = self.read_index().await?;

        if let Some(thumbnail_file) = thumbnail_file {
            if !thumbnail_file.is_empty() {
                let relative_path = Self::existing_background_asset_relative_path(thumbnail_file)?;
                self.ensure_background_file_exists(&relative_path).await?;
            }
        }

        let folder = Self::require_folder(&mut index, id)?;
        let mut modified = false;
        if let Some(name) = name {
            if name.is_empty() {
                return Err(DomainError::InvalidData(
                    "Folder name is required".to_string(),
                ));
            }
            if folder.name != name {
                folder.name = name.to_string();
                modified = true;
            }
        }
        if let Some(thumbnail_file) = thumbnail_file {
            if folder.thumbnail_file != thumbnail_file {
                folder.thumbnail_file = thumbnail_file.to_string();
                modified = true;
            }
        }

        let updated = folder.clone();
        if modified {
            self.write_index(&index).await?;
        }
        Ok(updated)
    }

    async fn delete_folder(&self, id: &str) -> Result<(), DomainError> {
        let _guard = self.lock.lock().await;
        let mut index = self.read_index().await?;
        Self::require_folder(&mut index, id)?;

        index.folders.retain(|folder| folder.id != id);
        for metadata in index.images.values_mut() {
            metadata.folder_ids.retain(|folder_id| folder_id != id);
        }

        self.write_index(&index).await
    }

    async fn set_folder_thumbnails(
        &self,
        updates: Vec<(String, String)>,
    ) -> Result<(), DomainError> {
        let _guard = self.lock.lock().await;
        let mut index = self.read_index().await?;

        let mut modified = false;
        for (id, thumbnail_file) in updates {
            Self::validate_folder_id(&id)?;
            let Some(folder) = Self::folder_mut(&mut index, &id) else {
                continue;
            };
            if !thumbnail_file.trim().is_empty() {
                Self::existing_background_asset_relative_path(&thumbnail_file)?;
            }

            if folder.thumbnail_file != thumbnail_file {
                folder.thumbnail_file = thumbnail_file;
                modified = true;
            }
        }

        if modified {
            self.write_index(&index).await?;
        }
        Ok(())
    }

    async fn assign_images_to_folder(
        &self,
        id: &str,
        paths: Vec<String>,
    ) -> Result<(), DomainError> {
        let _guard = self.lock.lock().await;
        let mut index = self.read_index().await?;
        Self::require_folder(&mut index, id)?;

        let mut normalized_paths = Vec::with_capacity(paths.len());
        for path in paths {
            let relative_path = Self::normalize_background_relative_path(&path)?;
            if self.background_file_exists(&relative_path).await? {
                normalized_paths.push(relative_path);
            } else {
                tracing::warn!(
                    "[ImageMetadata] Skipping missing background file: '{}'",
                    path
                );
            }
        }

        let mut modified = false;
        for relative_path in normalized_paths {
            let metadata = index.images.entry(relative_path).or_default();
            if !metadata.folder_ids.iter().any(|folder_id| folder_id == id) {
                metadata.folder_ids.push(id.to_string());
                modified = true;
            }
        }

        if modified {
            self.write_index(&index).await?;
        }
        Ok(())
    }

    async fn unassign_images_from_folder(
        &self,
        id: &str,
        paths: Vec<String>,
    ) -> Result<(), DomainError> {
        let _guard = self.lock.lock().await;
        let mut index = self.read_index().await?;
        Self::validate_folder_id(id)?;

        let normalized_paths = paths
            .into_iter()
            .map(|path| Self::normalize_background_relative_path(&path))
            .collect::<Result<Vec<_>, _>>()?;

        let mut modified = false;
        for relative_path in normalized_paths {
            if let Some(metadata) = index.images.get_mut(&relative_path) {
                let old_len = metadata.folder_ids.len();
                metadata.folder_ids.retain(|folder_id| folder_id != id);
                if metadata.folder_ids.len() != old_len {
                    modified = true;
                }
            }
        }

        if modified {
            self.write_index(&index).await?;
        }
        Ok(())
    }

    async fn remove_background_metadata(&self, filename: &str) -> Result<(), DomainError> {
        let relative_path = Self::strict_background_relative_path(filename)?;
        let removed_filename = Self::filename_from_background_relative_path(&relative_path);

        let _guard = self.lock.lock().await;
        let mut index = self.read_index().await?;
        let removed = index.images.remove(&relative_path).is_some();
        let cleared_thumbnail =
            Self::clear_folder_thumbnail_references(&mut index, &removed_filename);

        if removed || cleared_thumbnail {
            self.write_index(&index).await?;
        }

        Ok(())
    }

    async fn rename_background_metadata(
        &self,
        old_filename: &str,
        new_filename: &str,
    ) -> Result<(), DomainError> {
        let old_relative_path = Self::strict_background_relative_path(old_filename)?;
        let new_relative_path = Self::strict_background_relative_path(new_filename)?;
        let old_basename = Self::filename_from_background_relative_path(&old_relative_path);
        let new_basename = Self::filename_from_background_relative_path(&new_relative_path);

        let _guard = self.lock.lock().await;
        let mut index = self.read_index().await?;
        let mut modified = false;
        if let Some(metadata) = index.images.remove(&old_relative_path) {
            index.images.insert(new_relative_path, metadata);
            modified = true;
        } else {
            tracing::debug!(
                "No image metadata entry to rename for background '{}'",
                old_filename
            );
        }

        for folder in &mut index.folders {
            if folder.thumbnail_file == old_basename {
                folder.thumbnail_file = new_basename.clone();
                modified = true;
            }
        }

        if modified {
            self.write_index(&index).await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::PathBuf;

    use base64::Engine;
    use serde_json::json;

    use tt_domain::errors::DomainError;
    use tt_domain::models::image_metadata::{
        ImageMetadata, ImageMetadataFolder, ImageMetadataIndex,
    };
    use tt_ports::repositories::image_metadata_repository::ImageMetadataRepository;

    use super::{BackgroundMetadataBuildError, FileImageMetadataRepository};

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

    fn tiny_png() -> Vec<u8> {
        base64::engine::general_purpose::STANDARD
            .decode(
            "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/p9sAAAAASUVORK5CYII=",
        )
        .expect("decode png")
    }

    async fn repository_with_background() -> (TempDirGuard, FileImageMetadataRepository) {
        let temp = TempDirGuard::new("image-metadata");
        let user_root = temp.path.join("default-user");
        let backgrounds_dir = user_root.join("backgrounds");
        tokio::fs::create_dir_all(&backgrounds_dir)
            .await
            .expect("create backgrounds");
        tokio::fs::write(backgrounds_dir.join("a.png"), tiny_png())
            .await
            .expect("write background");

        let repository = FileImageMetadataRepository::new(user_root, backgrounds_dir);
        (temp, repository)
    }

    #[test]
    fn background_asset_segments_keep_legacy_mojibake_contract() {
        assert!(
            FileImageMetadataRepository::validate_background_asset_path_segment(
                "ã\u{80}\u{90}.png"
            )
        );
        assert!(!FileImageMetadataRepository::validate_background_asset_path_segment(".."));
        assert!(!FileImageMetadataRepository::validate_background_asset_path_segment("a/b.png"));
        assert!(
            !FileImageMetadataRepository::validate_background_asset_path_segment("bad\u{001F}.png")
        );
    }

    #[test]
    fn metadata_dimension_permission_errors_are_fatal() {
        let error =
            image::ImageError::IoError(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
        let classified = BackgroundMetadataBuildError::from_dimension_error(
            &PathBuf::from("blocked.png"),
            error,
        );

        assert!(matches!(classified, BackgroundMetadataBuildError::Fatal(_)));
    }

    #[test]
    fn metadata_dimension_decode_errors_are_skippable() {
        let error =
            image::ImageError::IoError(std::io::Error::from(std::io::ErrorKind::UnexpectedEof));
        let classified =
            BackgroundMetadataBuildError::from_dimension_error(&PathBuf::from("broken.png"), error);

        assert!(matches!(
            classified,
            BackgroundMetadataBuildError::Skippable(_)
        ));
    }

    #[test]
    fn metadata_file_vanished_errors_are_skippable() {
        let error = std::io::Error::from(std::io::ErrorKind::NotFound);
        let classified =
            BackgroundMetadataBuildError::from_metadata_error(&PathBuf::from("missing.png"), error);

        assert!(matches!(
            classified,
            BackgroundMetadataBuildError::Skippable(_)
        ));
    }

    #[test]
    fn metadata_animation_not_found_errors_are_skippable() {
        let classified = BackgroundMetadataBuildError::from_animation_error(DomainError::NotFound(
            "Source image not found: missing.png".to_string(),
        ));

        assert!(matches!(
            classified,
            BackgroundMetadataBuildError::Skippable(_)
        ));
    }

    #[tokio::test]
    async fn metadata_index_read_does_not_scan_backgrounds() {
        let (_temp, repository) = repository_with_background().await;

        let index = repository
            .read_metadata_index(Some("backgrounds/"))
            .await
            .expect("read metadata");
        assert!(index.images.is_empty());

        let entries = repository
            .get_background_list_entries()
            .await
            .expect("refresh background list");
        assert_eq!(entries.len(), 1);

        let index = repository
            .read_metadata_index(Some("backgrounds/"))
            .await
            .expect("read refreshed metadata");
        assert!(index.images.contains_key("backgrounds/a.png"));
    }

    #[tokio::test]
    async fn folder_assignment_is_persisted_in_background_folder_payload() {
        let (_temp, repository) = repository_with_background().await;
        let folder = repository
            .create_folder("Scenes")
            .await
            .expect("create folder");

        repository
            .assign_images_to_folder(&folder.id, vec!["backgrounds/a.png".to_string()])
            .await
            .expect("assign");

        let payload = repository
            .get_background_folders()
            .await
            .expect("get folders");

        assert_eq!(payload.folders, vec![folder.clone()]);
        assert_eq!(
            payload.image_folder_map.get("a.png"),
            Some(&vec![folder.id])
        );
    }

    #[tokio::test]
    async fn background_metadata_generation_preserves_folder_ids() {
        let (_temp, repository) = repository_with_background().await;
        let folder = repository
            .create_folder("Scenes")
            .await
            .expect("create folder");
        repository
            .assign_images_to_folder(&folder.id, vec!["backgrounds/a.png".to_string()])
            .await
            .expect("assign");

        let entries = repository
            .get_background_list_entries()
            .await
            .expect("refresh metadata");
        let index = repository
            .read_metadata_index(Some("backgrounds/"))
            .await
            .expect("get metadata");
        let metadata = index.images.get("backgrounds/a.png").expect("metadata");

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].filename, "a.png");
        assert!(!entries[0].is_animated);
        assert_eq!(metadata.folder_ids, vec![folder.id]);
        assert_eq!(metadata.is_animated, Some(false));
        assert_eq!(metadata.thumbnail_resolution, Some(160 * 90));
    }

    #[tokio::test]
    async fn background_list_accepts_legacy_c1_filenames() {
        let (_temp, repository) = repository_with_background().await;
        let legacy_name = "ã\u{80}\u{90}.png";
        tokio::fs::write(repository.backgrounds_dir.join(legacy_name), tiny_png())
            .await
            .expect("write legacy background");

        let entries = repository
            .get_background_list_entries()
            .await
            .expect("refresh metadata");

        assert!(entries.iter().any(|entry| entry.filename == "a.png"));
        assert!(entries.iter().any(|entry| entry.filename == legacy_name));
    }

    #[tokio::test]
    async fn background_list_keeps_scanning_when_one_image_has_bad_metadata() {
        let (_temp, repository) = repository_with_background().await;
        tokio::fs::write(repository.backgrounds_dir.join("broken.png"), b"not a png")
            .await
            .expect("write broken background");

        let entries = repository
            .get_background_list_entries()
            .await
            .expect("refresh metadata");
        let filenames = entries
            .iter()
            .map(|entry| entry.filename.as_str())
            .collect::<Vec<_>>();

        assert!(filenames.contains(&"a.png"));
        assert!(filenames.contains(&"broken.png"));

        let index = repository
            .read_metadata_index(Some("backgrounds/"))
            .await
            .expect("read metadata");
        assert!(index.images.contains_key("backgrounds/a.png"));
        assert!(!index.images.contains_key("backgrounds/broken.png"));
    }

    #[tokio::test]
    async fn stale_background_metadata_drops_derived_fields_and_keeps_unknown_fields() {
        let (_temp, repository) = repository_with_background().await;
        let mut metadata = ImageMetadata {
            hash: Some("stale-hash".to_string()),
            dominant_color: Some("#112233".to_string()),
            mtime: Some(1.0),
            ..ImageMetadata::default()
        };
        metadata
            .extra
            .insert("customField".to_string(), json!("kept"));

        repository
            .write_index(&ImageMetadataIndex {
                version: 1,
                images: HashMap::from([("backgrounds/a.png".to_string(), metadata)]),
                folders: Vec::new(),
            })
            .await
            .expect("seed metadata");

        repository
            .get_background_list_entries()
            .await
            .expect("refresh background list");

        let index = repository
            .read_metadata_index(Some("backgrounds/"))
            .await
            .expect("read metadata");
        let metadata = index.images.get("backgrounds/a.png").expect("metadata");

        assert_eq!(metadata.hash, None);
        assert_eq!(metadata.dominant_color, None);
        assert_eq!(metadata.extra.get("customField"), Some(&json!("kept")));
    }

    #[tokio::test]
    async fn invalid_folder_assignment_path_fails() {
        let (_temp, repository) = repository_with_background().await;
        let folder = repository
            .create_folder("Scenes")
            .await
            .expect("create folder");

        let error = repository
            .assign_images_to_folder(&folder.id, vec!["../a.png".to_string()])
            .await
            .expect_err("invalid path should fail");

        assert!(error.to_string().contains("Invalid background path"));
    }

    #[tokio::test]
    async fn folder_thumbnail_batch_skips_stale_folder_and_keeps_missing_cover() {
        let (_temp, repository) = repository_with_background().await;
        let folder = repository
            .create_folder("Scenes")
            .await
            .expect("create folder");

        repository
            .set_folder_thumbnails(vec![
                (folder.id.clone(), "missing.png".to_string()),
                ("stale-folder".to_string(), "../bad.png".to_string()),
            ])
            .await
            .expect("set thumbnails");

        let payload = repository
            .get_background_folders()
            .await
            .expect("get folders");
        assert_eq!(payload.folders[0].thumbnail_file, "missing.png");
    }

    #[tokio::test]
    async fn folder_assignment_skips_missing_background_files() {
        let (_temp, repository) = repository_with_background().await;
        let folder = repository
            .create_folder("Scenes")
            .await
            .expect("create folder");

        repository
            .assign_images_to_folder(
                &folder.id,
                vec![
                    "backgrounds/a.png".to_string(),
                    "backgrounds/missing.png".to_string(),
                ],
            )
            .await
            .expect("assign");

        let payload = repository
            .get_background_folders()
            .await
            .expect("get folders");
        assert_eq!(
            payload.image_folder_map.get("a.png"),
            Some(&vec![folder.id])
        );
        assert!(!payload.image_folder_map.contains_key("missing.png"));
    }

    #[tokio::test]
    async fn folder_unassignment_is_idempotent_for_stale_folder_and_missing_file() {
        let (_temp, repository) = repository_with_background().await;
        let metadata = ImageMetadata {
            folder_ids: vec!["stale-folder".to_string()],
            ..ImageMetadata::default()
        };
        repository
            .write_index(&ImageMetadataIndex {
                version: 1,
                images: HashMap::from([("backgrounds/missing.png".to_string(), metadata)]),
                folders: vec![ImageMetadataFolder {
                    id: "other-folder".to_string(),
                    name: "Other".to_string(),
                    thumbnail_file: String::new(),
                }],
            })
            .await
            .expect("seed metadata");

        repository
            .unassign_images_from_folder(
                "stale-folder",
                vec!["backgrounds/missing.png".to_string()],
            )
            .await
            .expect("unassign");

        let index = repository
            .read_metadata_index(Some("backgrounds/"))
            .await
            .expect("read metadata");
        assert_eq!(
            index
                .images
                .get("backgrounds/missing.png")
                .expect("metadata")
                .folder_ids,
            Vec::<String>::new()
        );
    }

    #[tokio::test]
    async fn renamed_background_metadata_updates_folder_thumbnail() {
        let (_temp, repository) = repository_with_background().await;
        let folder = repository
            .create_folder("Scenes")
            .await
            .expect("create folder");
        repository
            .update_folder(&folder.id, None, Some("a.png"))
            .await
            .expect("set thumbnail");

        repository
            .rename_background_metadata("a.png", "b.png")
            .await
            .expect("rename metadata");

        let payload = repository
            .get_background_folders()
            .await
            .expect("get folders");
        assert_eq!(payload.folders[0].thumbnail_file, "b.png");
    }

    #[tokio::test]
    async fn removed_background_metadata_clears_folder_thumbnail_without_image_entry() {
        let (_temp, repository) = repository_with_background().await;
        let folder = repository
            .create_folder("Scenes")
            .await
            .expect("create folder");
        repository
            .update_folder(&folder.id, None, Some("a.png"))
            .await
            .expect("set thumbnail");

        repository
            .remove_background_metadata("a.png")
            .await
            .expect("remove metadata");

        let payload = repository
            .get_background_folders()
            .await
            .expect("get folders");
        assert!(payload.folders[0].thumbnail_file.is_empty());
    }
}
