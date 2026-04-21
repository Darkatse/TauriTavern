use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::domain::errors::DomainError;

fn cache_path(data_root: &Path) -> PathBuf {
    data_root.join("_tauritavern").join(".ios-policy.json")
}

fn load_cache_sync(data_root: &Path) -> Result<Option<Value>, DomainError> {
    let path = cache_path(data_root);
    if !path.exists() {
        return Ok(None);
    }

    if !path.is_file() {
        return Err(DomainError::InvalidData(format!(
            "iOS policy cache path is not a file: {}",
            path.display()
        )));
    }

    let raw = std::fs::read_to_string(&path).map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to read iOS policy cache {}: {}",
            path.display(),
            error
        ))
    })?;

    let value = serde_json::from_str(&raw).map_err(|error| {
        DomainError::InvalidData(format!(
            "iOS policy cache {} contains invalid JSON: {}",
            path.display(),
            error
        ))
    })?;

    Ok(Some(value))
}

fn write_cache_sync(path: &Path, raw_policy: &Value) -> Result<(), DomainError> {
    use std::io;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to create directory {}: {}",
                parent.display(),
                error
            ))
        })?;
    }

    let json = serde_json::to_string_pretty(raw_policy).map_err(|error| {
        DomainError::InvalidData(format!(
            "Failed to serialize iOS policy cache payload for {}: {}",
            path.display(),
            error
        ))
    })?;

    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("ios-policy.json");
    let temp_path = path.with_file_name(format!("{}.{}.tmp", file_name, uuid::Uuid::new_v4()));

    std::fs::write(&temp_path, json.as_bytes()).map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to write iOS policy cache temp file {}: {}",
            temp_path.display(),
            error
        ))
    })?;

    match std::fs::rename(&temp_path, path) {
        Ok(()) => Ok(()),
        Err(rename_error) => {
            tracing::warn!(
                "Rename failed while replacing iOS policy cache {}: {}. Falling back to copy/remove.",
                path.display(),
                rename_error
            );

            std::fs::copy(&temp_path, path).map_err(|copy_error| {
                DomainError::InternalError(format!(
                    "Failed to replace iOS policy cache {}. Rename error: {}. Copy fallback error: {}",
                    path.display(),
                    rename_error,
                    copy_error
                ))
            })?;

            match std::fs::remove_file(&temp_path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => tracing::warn!(
                    "Copied iOS policy cache into place, but failed to remove temp file {}: {}",
                    temp_path.display(),
                    error
                ),
            }

            Ok(())
        }
    }
}

fn persist_cache_sync(data_root: &Path, raw_policy: &Value) -> Result<(), DomainError> {
    let path = cache_path(data_root);
    write_cache_sync(&path, raw_policy)
}

pub(crate) fn resolve_effective_raw_policy_sync(
    data_root: &Path,
    settings_raw_policy: Option<&Value>,
) -> Result<Option<Value>, DomainError> {
    if let Some(value) = settings_raw_policy {
        persist_cache_sync(data_root, value)?;
        return Ok(Some(value.clone()));
    }

    let cached = load_cache_sync(data_root)?;
    if cached.is_some() {
        tracing::info!(
            "Using cached ios_policy from {} because tauritavern-settings.json does not contain ios_policy",
            cache_path(data_root).display()
        );
    }
    Ok(cached)
}

pub(crate) async fn resolve_effective_raw_policy(
    data_root: &Path,
    settings_raw_policy: Option<&Value>,
) -> Result<Option<Value>, DomainError> {
    let data_root = data_root.to_path_buf();
    let settings_raw_policy = settings_raw_policy.cloned();
    tokio::task::spawn_blocking(move || {
        resolve_effective_raw_policy_sync(&data_root, settings_raw_policy.as_ref())
    })
    .await
    .map_err(|error| DomainError::InternalError(error.to_string()))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use uuid::Uuid;

    struct TempDirGuard {
        root: PathBuf,
    }

    impl TempDirGuard {
        fn new(prefix: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "tauritavern-ios-policy-cache-{}-{}",
                prefix,
                Uuid::new_v4()
            ));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(&root).expect("create temp root");
            Self { root }
        }
    }

    impl Drop for TempDirGuard {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[tokio::test]
    async fn resolve_effective_raw_policy_prefers_settings_and_persists_cache() {
        let temp = TempDirGuard::new("prefers-settings");
        let policy = json!({
            "version": 1,
            "profile": "ios_external_beta",
            "overrides": { "capabilities": { "updates": { "manual_check": true } } }
        });

        let resolved = resolve_effective_raw_policy(&temp.root, Some(&policy))
            .await
            .expect("resolve policy");
        assert_eq!(resolved, Some(policy.clone()));

        let cached = resolve_effective_raw_policy(&temp.root, None)
            .await
            .expect("resolve cached policy");
        assert_eq!(cached, Some(policy));
    }

    #[tokio::test]
    async fn resolve_effective_raw_policy_uses_cache_when_settings_missing() {
        let temp = TempDirGuard::new("uses-cache");
        let policy = json!({ "version": 1, "profile": "ios_external_beta" });

        let cache_path = cache_path(&temp.root);
        std::fs::create_dir_all(cache_path.parent().expect("cache path has parent"))
            .expect("create cache parent");
        std::fs::write(
            &cache_path,
            serde_json::to_string_pretty(&policy).expect("serialize"),
        )
        .expect("write cache file");

        let resolved = resolve_effective_raw_policy(&temp.root, None)
            .await
            .expect("resolve policy");
        assert_eq!(resolved, Some(policy));
    }

    #[tokio::test]
    async fn load_cache_fails_fast_on_invalid_json() {
        let temp = TempDirGuard::new("invalid-json");
        let path = cache_path(&temp.root);

        tokio::fs::create_dir_all(path.parent().expect("cache path has parent"))
            .await
            .expect("create cache parent");
        tokio::fs::write(&path, b"not json")
            .await
            .expect("write invalid cache");

        let error = resolve_effective_raw_policy(&temp.root, None)
            .await
            .unwrap_err();
        assert!(
            error.to_string().contains("contains invalid JSON"),
            "unexpected error: {}",
            error
        );
        assert!(
            error.to_string().contains(path.to_string_lossy().as_ref()),
            "expected error to mention cache path: {}",
            error
        );
    }

    #[tokio::test]
    async fn load_cache_rejects_directory_at_cache_path() {
        let temp = TempDirGuard::new("cache-is-dir");
        let path = cache_path(&temp.root);

        tokio::fs::create_dir_all(&path)
            .await
            .expect("create directory at cache path");

        let error = resolve_effective_raw_policy(&temp.root, None)
            .await
            .unwrap_err();
        assert!(
            error.to_string().contains("cache path is not a file"),
            "unexpected error: {}",
            error
        );
    }
}
