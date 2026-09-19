mod archive;

#[cfg(test)]
mod tests;

use std::io;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tokio::fs;
use tokio::sync::Mutex;

use tt_adapter_storage_core::file_system::{replace_file, unique_temp_path};
use tt_contracts::client_asset_paths::validate_path_segment;
use tt_domain::errors::DomainError;
use tt_ports::repositories::sprite_repository::{
    SpriteName, SpriteRepository, SpriteSet, StoredSprite,
};

pub struct FileSpriteRepository {
    characters_dir: PathBuf,
    mutation_lock: Mutex<()>,
}

impl FileSpriteRepository {
    pub fn new(characters_dir: PathBuf) -> Self {
        Self {
            characters_dir,
            mutation_lock: Mutex::new(()),
        }
    }

    pub(super) fn set_dir(&self, set: &SpriteSet) -> PathBuf {
        set.segments()
            .iter()
            .fold(self.characters_dir.clone(), |path, segment| {
                path.join(segment)
            })
    }

    async fn ensure_set_dir(&self, set: &SpriteSet) -> Result<PathBuf, DomainError> {
        let directory = self.set_dir(set);
        match fs::metadata(&directory).await {
            Ok(metadata) if metadata.is_dir() => return Ok(directory),
            Ok(_) => {
                return Err(DomainError::NotFound(format!(
                    "Sprite set path is not a directory: {}",
                    directory.display()
                )));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(DomainError::InternalError(format!(
                    "Failed to inspect sprite set '{}': {}",
                    directory.display(),
                    error
                )));
            }
        }

        fs::create_dir_all(&directory).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to create sprite set '{}': {}",
                directory.display(),
                error
            ))
        })?;
        Ok(directory)
    }

    fn image_extension(original_filename: &str) -> Result<String, DomainError> {
        if !is_image_filename(original_filename) {
            return Err(DomainError::InvalidData(
                "Sprite upload must be an image".to_string(),
            ));
        }

        Path::new(original_filename)
            .extension()
            .and_then(|extension| extension.to_str())
            .filter(|extension| validate_path_segment(extension))
            .map(str::to_string)
            .ok_or_else(|| DomainError::InvalidData("Sprite image has no valid extension".into()))
    }

    async fn copy_atomic(source: &Path, target: &Path) -> Result<(), DomainError> {
        let temp = unique_temp_path(target);
        if let Err(error) = fs::copy(source, &temp).await {
            let _ = fs::remove_file(&temp).await;
            if error.kind() == io::ErrorKind::NotFound {
                return Err(DomainError::NotFound(format!(
                    "Sprite upload file not found: {}",
                    source.display()
                )));
            }
            return Err(DomainError::InternalError(format!(
                "Failed to stage sprite file '{}': {}",
                target.display(),
                error
            )));
        }

        if let Err(error) = replace_file(&temp, target).await {
            let _ = fs::remove_file(&temp).await;
            return Err(error);
        }
        Ok(())
    }

    async fn delete_variants(
        directory: &Path,
        sprite_name: &str,
        keep_file_name: Option<&str>,
    ) -> Result<(), DomainError> {
        let mut entries = fs::read_dir(directory).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read sprite set '{}': {}",
                directory.display(),
                error
            ))
        })?;

        while let Some(entry) = entries.next_entry().await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read sprite set entry '{}': {}",
                directory.display(),
                error
            ))
        })? {
            let file_name = entry.file_name();
            let Some(file_name) = file_name.to_str() else {
                continue;
            };
            if keep_file_name == Some(file_name) || file_stem(file_name) != Some(sprite_name) {
                continue;
            }

            let metadata = match entry.metadata().await {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => {
                    return Err(DomainError::InternalError(format!(
                        "Failed to inspect sprite '{}': {}",
                        entry.path().display(),
                        error
                    )));
                }
            };
            if metadata.is_file() {
                match fs::remove_file(entry.path()).await {
                    Ok(()) => {}
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Err(error) => {
                        return Err(DomainError::InternalError(format!(
                            "Failed to delete sprite '{}': {}",
                            entry.path().display(),
                            error
                        )));
                    }
                }
            }
        }
        Ok(())
    }
}

#[async_trait]
impl SpriteRepository for FileSpriteRepository {
    async fn list(&self, set: &SpriteSet) -> Result<Vec<StoredSprite>, DomainError> {
        let directory = self.set_dir(set);
        let mut entries = match fs::read_dir(&directory).await {
            Ok(entries) => entries,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
                ) =>
            {
                return Ok(Vec::new());
            }
            Err(error) => {
                return Err(DomainError::InternalError(format!(
                    "Failed to read sprite set '{}': {}",
                    directory.display(),
                    error
                )));
            }
        };

        let mut sprites = Vec::new();
        while let Some(entry) = entries.next_entry().await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read sprite set entry '{}': {}",
                directory.display(),
                error
            ))
        })? {
            let Ok(file_name) = entry.file_name().into_string() else {
                tracing::warn!(path = %entry.path().display(), "Ignoring non-UTF-8 sprite filename");
                continue;
            };
            if !validate_path_segment(&file_name) || !is_image_filename(&file_name) {
                continue;
            }

            let metadata = match entry.metadata().await {
                Ok(metadata) if metadata.is_file() => metadata,
                Ok(_) => continue,
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::NotFound | io::ErrorKind::PermissionDenied
                    ) =>
                {
                    tracing::warn!(path = %entry.path().display(), %error, "Ignoring unreadable sprite");
                    continue;
                }
                Err(error) => {
                    return Err(DomainError::InternalError(format!(
                        "Failed to inspect sprite '{}': {}",
                        entry.path().display(),
                        error
                    )));
                }
            };
            sprites.push(StoredSprite {
                file_name,
                modified_at: metadata.modified().ok().map(DateTime::<Utc>::from),
            });
        }
        sprites.sort_by(|left, right| left.file_name.cmp(&right.file_name));
        Ok(sprites)
    }

    async fn upload(
        &self,
        set: &SpriteSet,
        sprite_name: &SpriteName,
        original_filename: &str,
        source_path: &Path,
    ) -> Result<(), DomainError> {
        let _guard = self.mutation_lock.lock().await;
        let directory = self.ensure_set_dir(set).await?;
        let extension = Self::image_extension(original_filename)?;
        let file_name = format!("{}.{}", sprite_name.as_str(), extension);
        let target = directory.join(&file_name);

        Self::copy_atomic(source_path, &target).await?;
        Self::delete_variants(&directory, sprite_name.as_str(), Some(&file_name)).await
    }

    async fn upload_pack(
        &self,
        set: &SpriteSet,
        archive_path: &Path,
    ) -> Result<usize, DomainError> {
        let _guard = self.mutation_lock.lock().await;
        let directory = self.set_dir(set);
        let archive_path = archive_path.to_path_buf();
        tokio::task::spawn_blocking(move || archive::import_sprite_pack(&archive_path, &directory))
            .await
            .map_err(|error| {
                DomainError::InternalError(format!("Sprite pack import task failed: {error}"))
            })?
    }

    async fn delete(&self, set: &SpriteSet, sprite_name: &SpriteName) -> Result<(), DomainError> {
        let _guard = self.mutation_lock.lock().await;
        let directory = self.set_dir(set);
        match fs::metadata(&directory).await {
            Ok(metadata) if metadata.is_dir() => {}
            Ok(_) => {
                return Err(DomainError::NotFound(format!(
                    "Sprite set not found: {}",
                    directory.display()
                )));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(DomainError::NotFound(format!(
                    "Sprite set not found: {}",
                    directory.display()
                )));
            }
            Err(error) => {
                return Err(DomainError::InternalError(format!(
                    "Failed to inspect sprite set '{}': {}",
                    directory.display(),
                    error
                )));
            }
        }

        Self::delete_variants(&directory, sprite_name.as_str(), None).await
    }
}

pub(super) fn is_image_filename(file_name: impl AsRef<Path>) -> bool {
    mime_guess::from_path(file_name)
        .first_raw()
        .is_some_and(|mime| mime.starts_with("image/"))
}

pub(super) fn file_stem(file_name: &str) -> Option<&str> {
    Path::new(file_name).file_stem()?.to_str()
}
