//! Private files shared by WebView byte staging and native file transfers.
//!
//! The staging root is cleared at startup and has two areas. The page area
//! holds every file whose path the page holds, and only content the page may
//! read: its own bytes, what the user picked or shared, and content downloaded
//! under an import-source grant. The host area holds files the host keeps for
//! the page without naming them to it.

use std::path::{Component, Path, PathBuf};

use async_trait::async_trait;
use tauri::{AppHandle, Manager};
use tokio::fs;
use tokio::io::AsyncReadExt;
use tt_domain::errors::DomainError;
use tt_ports::byte_reader::ByteReader;

const STAGING_ROOT_NAME: &str = "tauritavern-staging";
const PAGE_AREA: &str = "page";
const HOST_AREA: &str = "host";

fn staging_root(app: &AppHandle) -> Result<PathBuf, DomainError> {
    app.path()
        .app_cache_dir()
        .map(|path| path.join(STAGING_ROOT_NAME))
        .map_err(|error| {
            DomainError::InternalError(format!("Failed to resolve file staging directory: {error}"))
        })
}

fn page_area(app: &AppHandle) -> Result<PathBuf, DomainError> {
    Ok(staging_root(app)?.join(PAGE_AREA))
}

fn kind_directory(app: &AppHandle, kind: &str) -> Result<PathBuf, DomainError> {
    if kind.is_empty()
        || kind.len() > 48
        || !kind
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-' || ch == '_')
    {
        return Err(DomainError::InvalidData("Invalid file staging kind".into()));
    }
    Ok(page_area(app)?.join(kind))
}

fn file_name(extension: &str) -> Result<String, DomainError> {
    let extension = extension.trim_start_matches('.').to_ascii_lowercase();
    if extension.is_empty()
        || extension.len() > 12
        || !extension.chars().all(|ch| ch.is_ascii_alphanumeric())
    {
        return Err(DomainError::InvalidData(
            "Invalid file staging extension".into(),
        ));
    }
    Ok(format!("{}.{}", uuid::Uuid::new_v4().simple(), extension))
}

async fn create_directory(directory: &Path) -> Result<(), DomainError> {
    fs::create_dir_all(directory).await.map_err(|error| {
        DomainError::file_io(
            "Create staging directory",
            directory.display().to_string(),
            error,
        )
    })
}

async fn create_file(directory: PathBuf, extension: &str) -> Result<PathBuf, DomainError> {
    let path = directory.join(file_name(extension)?);
    create_directory(&directory).await?;
    fs::File::create(&path).await.map_err(|error| {
        DomainError::file_io("Create staged file", path.display().to_string(), error)
    })?;
    Ok(path)
}

/// An empty file in the page area.
pub async fn create_staged_file(
    app: &AppHandle,
    kind: &str,
    extension: &str,
) -> Result<PathBuf, DomainError> {
    create_file(kind_directory(app, kind)?, extension).await
}

/// An empty directory in the page area.
#[cfg(desktop)]
pub async fn create_staged_directory(app: &AppHandle, kind: &str) -> Result<PathBuf, DomainError> {
    let directory = kind_directory(app, kind)?.join(uuid::Uuid::new_v4().simple().to_string());
    create_directory(&directory).await?;
    Ok(directory)
}

/// An empty file in the host area.
pub async fn create_host_file(app: &AppHandle, extension: &str) -> Result<PathBuf, DomainError> {
    create_file(staging_root(app)?.join(HOST_AREA), extension).await
}

/// The path a page names, when it lies in the page area.
pub async fn validate_staged_path(
    app: &AppHandle,
    file_path: &str,
) -> Result<PathBuf, DomainError> {
    resolve_in_area(&page_area(app)?, file_path).await
}

async fn resolve_in_area(area: &Path, requested: &str) -> Result<PathBuf, DomainError> {
    let requested = PathBuf::from(requested);
    if !requested.is_absolute()
        || requested
            .components()
            .any(|component| component == Component::ParentDir)
    {
        return Err(DomainError::InvalidData(
            "File staging path must be absolute and free of '..'".into(),
        ));
    }
    let parent = requested.parent().ok_or_else(|| {
        DomainError::InvalidData("File staging path is missing a parent directory".into())
    })?;
    let canonical_area = fs::canonicalize(area).await.map_err(|error| {
        DomainError::file_io("Resolve staging area", area.display().to_string(), error)
    })?;
    let canonical_parent = fs::canonicalize(parent).await.map_err(|error| {
        DomainError::file_io(
            "Resolve staging parent",
            parent.display().to_string(),
            error,
        )
    })?;
    if !canonical_parent.starts_with(canonical_area) {
        return Err(DomainError::InvalidData(
            "File is outside the staging directory".into(),
        ));
    }
    Ok(requested)
}

/// Opens a file the page names for the page to read.
pub async fn open_staged_file(
    app: &AppHandle,
    file_path: &str,
) -> Result<Box<dyn ByteReader>, DomainError> {
    let path = validate_staged_path(app, file_path).await?;
    let file = fs::File::open(&path).await.map_err(|error| {
        DomainError::file_io("Open staged file", path.display().to_string(), error)
    })?;
    Ok(Box::new(StagedFileReader { file, path }))
}

struct StagedFileReader {
    file: fs::File,
    path: PathBuf,
}

#[async_trait]
impl ByteReader for StagedFileReader {
    async fn read(&mut self, buffer: &mut [u8]) -> Result<usize, DomainError> {
        self.file.read(buffer).await.map_err(|error| {
            DomainError::file_io("Read staged file", self.path.display().to_string(), error)
        })
    }
}

/// Removes a staged file or directory; one already gone counts as removed.
pub async fn discard(path: &Path) -> Result<(), DomainError> {
    let removed = match fs::symlink_metadata(path).await {
        Ok(metadata) if metadata.is_dir() => fs::remove_dir_all(path).await,
        Ok(_) => fs::remove_file(path).await,
        Err(error) => Err(error),
    };
    match removed {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(DomainError::file_io(
            "Remove staged file",
            path.display().to_string(),
            error,
        )),
    }
}

pub async fn clear_staging_root(app: &AppHandle) -> Result<(), DomainError> {
    let root = staging_root(app)?;
    match fs::remove_dir_all(&root).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(DomainError::file_io(
            "Clear file staging directory",
            root.display().to_string(),
            error,
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn only_paths_inside_the_page_area_resolve() {
        let root =
            std::env::temp_dir().join(format!("tauritavern-staging-{}", uuid::Uuid::new_v4()));
        let area = root.join(PAGE_AREA);
        let kind = area.join("avatar");
        let host = root.join(HOST_AREA);
        std::fs::create_dir_all(&kind).unwrap();
        std::fs::create_dir_all(&host).unwrap();
        for file in [kind.join("a.png"), host.join("b.bin"), root.join("c.bin")] {
            std::fs::write(file, b"x").unwrap();
        }
        let accepted = kind.join("a.png");

        assert!(
            resolve_in_area(&area, accepted.to_str().unwrap())
                .await
                .is_ok()
        );
        for rejected in [
            host.join("b.bin"),
            root.join("c.bin"),
            area.clone(),
            kind.join("../../host/b.bin"),
            kind.join(".."),
            PathBuf::from("avatar/a.png"),
        ] {
            assert!(
                resolve_in_area(&area, rejected.to_str().unwrap())
                    .await
                    .is_err(),
                "{}",
                rejected.display()
            );
        }
        std::fs::remove_dir_all(&root).unwrap();
    }
}
