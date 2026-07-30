use std::path::{Path, PathBuf};

use serde_json::Value;

use tt_adapter_storage_core::file_system::write_json_file_sync;
use tt_domain::errors::DomainError;

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

fn persist_cache_sync(data_root: &Path, raw_policy: &Value) -> Result<(), DomainError> {
    write_json_file_sync(&cache_path(data_root), raw_policy)
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

    #[test]
    fn resolve_effective_raw_policy_prefers_settings_and_persists_cache() {
        let temp = TempDirGuard::new("prefers-settings");
        let policy = json!({
            "version": 1,
            "profile": "ios_external_beta",
            "overrides": { "capabilities": { "updates": { "manual_check": true } } }
        });

        let resolved =
            resolve_effective_raw_policy_sync(&temp.root, Some(&policy)).expect("resolve policy");
        assert_eq!(resolved, Some(policy.clone()));

        let cached =
            resolve_effective_raw_policy_sync(&temp.root, None).expect("resolve cached policy");
        assert_eq!(cached, Some(policy));
    }

    #[test]
    fn resolve_effective_raw_policy_uses_cache_when_settings_missing() {
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

        let resolved = resolve_effective_raw_policy_sync(&temp.root, None).expect("resolve policy");
        assert_eq!(resolved, Some(policy));
    }

    #[test]
    fn load_cache_fails_fast_on_invalid_json() {
        let temp = TempDirGuard::new("invalid-json");
        let path = cache_path(&temp.root);

        std::fs::create_dir_all(path.parent().expect("cache path has parent"))
            .expect("create cache parent");
        std::fs::write(&path, b"not json").expect("write invalid cache");

        let error = resolve_effective_raw_policy_sync(&temp.root, None).unwrap_err();
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

    #[test]
    fn load_cache_rejects_directory_at_cache_path() {
        let temp = TempDirGuard::new("cache-is-dir");
        let path = cache_path(&temp.root);

        std::fs::create_dir_all(&path).expect("create directory at cache path");

        let error = resolve_effective_raw_policy_sync(&temp.root, None).unwrap_err();
        assert!(
            error.to_string().contains("cache path is not a file"),
            "unexpected error: {}",
            error
        );
    }
}
