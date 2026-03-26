use std::path::{Component, Path, PathBuf};
use std::time::UNIX_EPOCH;

use ttsync_contract::manifest::{ManifestEntryV2, ManifestV2};
use ttsync_contract::path::SyncPath;

use crate::domain::errors::DomainError;

pub async fn scan_manifest(sync_root: PathBuf) -> Result<ManifestV2, DomainError> {
    tokio::task::spawn_blocking(move || scan_manifest_sync(&sync_root))
        .await
        .map_err(|error| DomainError::InternalError(error.to_string()))?
}

fn scan_manifest_sync(sync_root: &Path) -> Result<ManifestV2, DomainError> {
    let mut entries = Vec::new();

    for directory in ttsync_core::scope::included_directories() {
        let root = sync_root.join(directory);
        if !root.exists() {
            continue;
        }
        if !root.is_dir() {
            return Err(DomainError::InvalidData(format!(
                "Sync scope root is not a directory: {}",
                root.display()
            )));
        }

        scan_dir_recursive(sync_root, &root, &mut entries)?;
    }

    for file in ttsync_core::scope::included_files() {
        let path = sync_root.join(file);
        if !path.exists() {
            continue;
        }
        if !path.is_file() {
            return Err(DomainError::InvalidData(format!(
                "Sync scope root is not a file: {}",
                path.display()
            )));
        }

        entries.push(make_entry(sync_root, &path)?);
    }

    entries.sort_by(|a, b| a.path.as_str().cmp(b.path.as_str()));
    Ok(ManifestV2 { entries })
}

fn scan_dir_recursive(
    sync_root: &Path,
    dir: &Path,
    entries: &mut Vec<ManifestEntryV2>,
) -> Result<(), DomainError> {
    for entry in std::fs::read_dir(dir).map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to read directory {}: {}",
            dir.display(),
            error
        ))
    })? {
        let entry = entry.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read directory entry in {}: {}",
                dir.display(),
                error
            ))
        })?;

        let file_type = entry.file_type().map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read file type for {}: {}",
                entry.path().display(),
                error
            ))
        })?;

        let entry_path = entry.path();

        if file_type.is_symlink() {
            return Err(DomainError::InvalidData(format!(
                "Symlinks are not supported in sync scope: {}",
                entry_path.display()
            )));
        }

        let relative = entry_path
            .strip_prefix(sync_root)
            .map_err(|error| DomainError::InternalError(error.to_string()))?;
        let relative = normalize_relative_path(relative)?;

        if ttsync_core::scope::is_excluded(&relative) {
            continue;
        }

        if file_type.is_dir() {
            scan_dir_recursive(sync_root, &entry_path, entries)?;
            continue;
        }

        if file_type.is_file() {
            entries.push(make_entry(sync_root, &entry_path)?);
        }
    }

    Ok(())
}

fn make_entry(sync_root: &Path, file_path: &Path) -> Result<ManifestEntryV2, DomainError> {
    let relative = file_path
        .strip_prefix(sync_root)
        .map_err(|error| DomainError::InternalError(error.to_string()))?;
    let relative = normalize_relative_path(relative)?;
    let path =
        SyncPath::new(relative).map_err(|error| DomainError::InvalidData(error.to_string()))?;

    let metadata = std::fs::metadata(file_path).map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to read metadata for {}: {}",
            file_path.display(),
            error
        ))
    })?;

    let modified_ms = metadata
        .modified()
        .map_err(|error| DomainError::InternalError(error.to_string()))?
        .duration_since(UNIX_EPOCH)
        .map_err(|error| DomainError::InternalError(error.to_string()))?
        .as_millis() as u64;

    Ok(ManifestEntryV2 {
        path,
        size_bytes: metadata.len(),
        modified_ms,
        content_hash: None,
    })
}

fn normalize_relative_path(path: &Path) -> Result<String, DomainError> {
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(value) => parts.push(value.to_str().ok_or_else(|| {
                DomainError::InvalidData("Path contains non-UTF-8 components".to_string())
            })?),
            Component::CurDir => continue,
            other => {
                return Err(DomainError::InvalidData(format!(
                    "Path contains unsupported component: {:?}",
                    other
                )));
            }
        }
    }

    Ok(parts.join("/"))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use rand::random;

    use super::scan_manifest_sync;

    fn unique_temp_root() -> PathBuf {
        std::env::temp_dir().join(format!("tauritavern-tt-sync-{}", random::<u64>()))
    }

    #[test]
    fn scan_manifest_respects_v2_scope_and_excludes_lan_sync_state() {
        let root = unique_temp_root();
        let _ = std::fs::remove_dir_all(&root);

        std::fs::create_dir_all(root.join("default-user").join("chats"))
            .expect("create chats directory");
        std::fs::create_dir_all(
            root.join("default-user")
                .join("user")
                .join("lan-sync")
                .join("tt-sync-v2"),
        )
        .expect("create tt sync state directory");

        std::fs::write(
            root.join("default-user").join("chats").join("chat.jsonl"),
            b"chat",
        )
        .expect("write included file");
        std::fs::write(
            root.join("default-user")
                .join("user")
                .join("lan-sync")
                .join("tt-sync-v2")
                .join("identity.json"),
            b"{}",
        )
        .expect("write excluded state file");

        let manifest = scan_manifest_sync(&root).expect("scan manifest");
        let paths = manifest
            .entries
            .into_iter()
            .map(|entry| entry.path.to_string())
            .collect::<Vec<_>>();

        assert!(
            paths.contains(&"default-user/chats/chat.jsonl".to_string()),
            "included file must appear in manifest"
        );
        assert!(
            !paths.contains(&"default-user/user/lan-sync/tt-sync-v2/identity.json".to_string()),
            "lan sync state must never be part of the manifest"
        );

        std::fs::remove_dir_all(&root).expect("remove temp root");
    }
}
