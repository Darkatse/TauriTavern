//! Private files shared by WebView byte staging and native file transfers.

use std::path::{Path, PathBuf};

use tauri::{AppHandle, Manager};
use tokio::fs;
use tt_domain::errors::DomainError;

const STAGING_ROOT_NAME: &str = "tauritavern-staging";

fn staging_root(app: &AppHandle) -> Result<PathBuf, DomainError> {
    app.path()
        .app_cache_dir()
        .map(|path| path.join(STAGING_ROOT_NAME))
        .map_err(|error| {
            DomainError::InternalError(format!("Failed to resolve file staging directory: {error}"))
        })
}

pub async fn create_staged_file(
    app: &AppHandle,
    kind: &str,
    extension: &str,
) -> Result<PathBuf, DomainError> {
    if kind.is_empty()
        || kind.len() > 48
        || !kind
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-' || ch == '_')
    {
        return Err(DomainError::InvalidData("Invalid file staging kind".into()));
    }
    let extension = extension.trim_start_matches('.').to_ascii_lowercase();
    if extension.is_empty()
        || extension.len() > 12
        || !extension.chars().all(|ch| ch.is_ascii_alphanumeric())
    {
        return Err(DomainError::InvalidData(
            "Invalid file staging extension".into(),
        ));
    }

    let directory = staging_root(app)?.join(kind);
    fs::create_dir_all(&directory).await.map_err(|error| {
        DomainError::file_io(
            "Create staging directory",
            directory.display().to_string(),
            error,
        )
    })?;
    let path = directory.join(format!("{}.{}", uuid::Uuid::new_v4().simple(), extension));
    fs::File::create(&path).await.map_err(|error| {
        DomainError::file_io("Create staged file", path.display().to_string(), error)
    })?;
    Ok(path)
}

pub async fn validate_staged_path(
    app: &AppHandle,
    file_path: &str,
) -> Result<PathBuf, DomainError> {
    let requested = PathBuf::from(file_path);
    if !requested.is_absolute() {
        return Err(DomainError::InvalidData(
            "File staging path must be absolute".into(),
        ));
    }
    let parent = requested.parent().ok_or_else(|| {
        DomainError::InvalidData("File staging path is missing a parent directory".into())
    })?;
    let root = staging_root(app)?;
    let canonical_root = fs::canonicalize(&root).await.map_err(|error| {
        DomainError::file_io("Resolve staging root", root.display().to_string(), error)
    })?;
    let canonical_parent = fs::canonicalize(parent).await.map_err(|error| {
        DomainError::file_io(
            "Resolve staging parent",
            parent.display().to_string(),
            error,
        )
    })?;
    if !canonical_parent.starts_with(canonical_root) {
        return Err(DomainError::InvalidData(
            "File is outside the staging directory".into(),
        ));
    }
    Ok(requested)
}

pub async fn discard_file(path: &Path) -> Result<(), DomainError> {
    match fs::remove_file(path).await {
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
