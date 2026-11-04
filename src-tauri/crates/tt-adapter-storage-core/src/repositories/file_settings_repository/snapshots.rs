//! Settings snapshots use SillyTavern's settings backup layout:
//! `backups/settings_<handle>_<timestamp>.json`, newest 50 kept.

use std::cmp::Reverse;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::time::UNIX_EPOCH;

use chrono::{DateTime, Local};
use tt_domain::errors::DomainError;
use tt_domain::models::settings::{SettingsSnapshot, UserSettings};

use super::FileSettingsRepository;
use crate::file_system::{BACKUP_TIMESTAMP_FORMAT, persist_json_file_blocking};

const BACKUPS_DIR: &str = "backups";
const PREFIX: &str = "settings_default-user_";
const LIMIT: usize = 50;
/// Legacy snapshots, named `settings_<unix_ms>.json`.
const LEGACY_DIR: &str = "snapshots";

impl FileSettingsRepository {
    /// Moves legacy snapshots into `backups/`, keeping a source whose target
    /// name is already taken.
    pub async fn migrate_legacy_snapshots(&self) {
        let root = self.base_directory.clone();
        if let Err(error) = tokio::task::spawn_blocking(move || migrate_legacy(&root)).await {
            tracing::warn!(%error, "Settings snapshot migration task failed");
        }
    }
}

pub(super) fn create(root: &Path, settings: &UserSettings) -> Result<String, DomainError> {
    let dir = root.join(BACKUPS_DIR);
    let name = format!(
        "{PREFIX}{}.json",
        Local::now().format(BACKUP_TIMESTAMP_FORMAT)
    );
    persist_json_file_blocking(&dir.join(&name), settings)?;

    let mut entries = entries(&dir)?;
    entries.sort_by_key(|entry| Reverse(entry.snapshot.date));
    for entry in entries.iter().skip(LIMIT) {
        fs::remove_file(&entry.path).map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to remove old settings snapshot {}: {error}",
                entry.path.display()
            ))
        })?;
    }
    Ok(name)
}

pub(super) fn list(root: &Path) -> Result<Vec<SettingsSnapshot>, DomainError> {
    let mut snapshots: Vec<_> = entries(&root.join(BACKUPS_DIR))?
        .into_iter()
        .map(|entry| entry.snapshot)
        .collect();
    snapshots.sort_by_key(|snapshot| Reverse(snapshot.date));
    Ok(snapshots)
}

/// Like upstream, accepts a single path component under the snapshot prefix.
pub(super) fn path(root: &Path, name: &str) -> Result<PathBuf, DomainError> {
    let mut components = Path::new(name).components();
    let plain = matches!(
        (components.next(), components.next()),
        (Some(Component::Normal(_)), None)
    );
    if !plain || !name.starts_with(PREFIX) {
        return Err(DomainError::InvalidData(format!(
            "Invalid settings snapshot name: {name}"
        )));
    }
    Ok(root.join(BACKUPS_DIR).join(name))
}

struct Entry {
    path: PathBuf,
    snapshot: SettingsSnapshot,
}

fn entries(dir: &Path) -> Result<Vec<Entry>, DomainError> {
    let read_dir = match fs::read_dir(dir) {
        Ok(read_dir) => read_dir,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(io_error("read", dir, error)),
    };

    let mut entries = Vec::new();
    for entry in read_dir {
        let entry = entry.map_err(|error| io_error("read", dir, error))?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if !name.starts_with(PREFIX) {
            continue;
        }
        let path = entry.path();
        let metadata = entry
            .metadata()
            .map_err(|error| io_error("inspect", &path, error))?;
        if !metadata.is_file() {
            continue;
        }
        let modified = metadata
            .modified()
            .map_err(|error| io_error("inspect", &path, error))?;
        let date = modified
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_millis() as i64);
        entries.push(Entry {
            path,
            snapshot: SettingsSnapshot {
                date,
                name,
                size: metadata.len(),
            },
        });
    }
    Ok(entries)
}

fn migrate_legacy(root: &Path) {
    let legacy_dir = root.join(LEGACY_DIR);
    let read_dir = match fs::read_dir(&legacy_dir) {
        Ok(read_dir) => read_dir,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return,
        Err(error) => {
            tracing::warn!(%error, "Failed to read legacy settings snapshots");
            return;
        }
    };
    let backups_dir = root.join(BACKUPS_DIR);
    if let Err(error) = fs::create_dir_all(&backups_dir) {
        tracing::warn!(%error, "Failed to create {}", backups_dir.display());
        return;
    }

    for entry in read_dir {
        let source = match entry {
            Ok(entry) => entry.path(),
            Err(error) => {
                tracing::warn!(%error, "Failed to read legacy settings snapshots");
                continue;
            }
        };
        let Some(target) = legacy_target(&backups_dir, &source) else {
            continue;
        };
        if target.exists() {
            tracing::warn!(
                "Kept legacy settings snapshot {} because {} already exists",
                source.display(),
                target.display()
            );
        } else if let Err(error) = fs::rename(&source, &target) {
            tracing::warn!(%error, "Failed to move legacy settings snapshot {}", source.display());
        }
    }

    // Succeeds only once every legacy snapshot has moved.
    let _ = fs::remove_dir(&legacy_dir);
}

fn legacy_target(backups_dir: &Path, source: &Path) -> Option<PathBuf> {
    let millis = source
        .file_name()
        .and_then(OsStr::to_str)?
        .strip_prefix("settings_")?
        .strip_suffix(".json")?
        .parse::<i64>()
        .ok()?;
    let at = DateTime::from_timestamp_millis(millis)?.with_timezone(&Local);
    Some(backups_dir.join(format!(
        "{PREFIX}{}.json",
        at.format(BACKUP_TIMESTAMP_FORMAT)
    )))
}

fn io_error(action: &str, path: &Path, error: io::Error) -> DomainError {
    DomainError::InternalError(format!(
        "Failed to {action} settings snapshots at {}: {error}",
        path.display()
    ))
}
