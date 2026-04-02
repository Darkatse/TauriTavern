use std::path::{Component, Path, PathBuf};

use crate::domain::errors::DomainError;

const SYNC_SCOPE_DIRECTORIES: &[&str] = &[
    "default-user/chats",
    "default-user/characters",
    "default-user/groups",
    "default-user/group chats",
    "default-user/worlds",
    "default-user/themes",
    "default-user/user",
    "default-user/User Avatars",
    "default-user/OpenAI Settings",
    "default-user/extensions",
    "default-user/backgrounds",
    "extensions/third-party",
    "_tauritavern/extension-sources/local",
    "_tauritavern/extension-sources/global",
];

const SYNC_SCOPE_FILES: &[&str] = &["default-user/settings.json"];
const EXCLUDED_RELATIVE_PATHS: &[&str] = &["default-user/user/lan-sync"];

pub fn sync_scope_directories() -> &'static [&'static str] {
    SYNC_SCOPE_DIRECTORIES
}

pub fn sync_scope_files() -> &'static [&'static str] {
    SYNC_SCOPE_FILES
}

pub fn is_excluded_relative_path(relative_path: &str) -> bool {
    let value = relative_path.trim();
    EXCLUDED_RELATIVE_PATHS.iter().any(|excluded| {
        value == *excluded
            || value
                .strip_prefix(excluded)
                .is_some_and(|suffix| suffix.starts_with('/'))
    })
}

pub fn validate_relative_path(relative_path: &str) -> Result<(), DomainError> {
    let value = relative_path.trim();
    if value.is_empty() {
        return Err(DomainError::InvalidData(
            "Relative path is empty".to_string(),
        ));
    }

    if value.starts_with('/') {
        return Err(DomainError::InvalidData(
            "Relative path must not start with '/'".to_string(),
        ));
    }

    if value.contains('\\') {
        return Err(DomainError::InvalidData(
            "Relative path must use '/' separators".to_string(),
        ));
    }

    if is_excluded_relative_path(value) {
        return Err(DomainError::InvalidData(format!(
            "Path is excluded from sync scope: {}",
            value
        )));
    }

    let parts: Vec<&str> = value.split('/').collect();
    if parts
        .iter()
        .any(|part| part.is_empty() || *part == "." || *part == "..")
    {
        return Err(DomainError::InvalidData(format!(
            "Relative path contains invalid components: {}",
            value
        )));
    }

    if SYNC_SCOPE_FILES.contains(&value) || is_within_scoped_directory(value) {
        return Ok(());
    }

    Err(DomainError::InvalidData(format!(
        "Path not allowed in sync scope: {}",
        value
    )))
}

fn is_within_scoped_directory(relative_path: &str) -> bool {
    SYNC_SCOPE_DIRECTORIES.iter().any(|scope_root| {
        relative_path
            .strip_prefix(scope_root)
            .is_some_and(|suffix| suffix.starts_with('/'))
    })
}

pub fn resolve_relative_path(
    sync_root: &Path,
    relative_path: &str,
) -> Result<PathBuf, DomainError> {
    validate_relative_path(relative_path)?;

    let mut full_path = PathBuf::from(sync_root);
    for part in relative_path.split('/') {
        full_path.push(part);
    }

    Ok(full_path)
}

pub fn normalize_relative_path(path: &Path) -> Result<String, DomainError> {
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
    use super::validate_relative_path;

    #[test]
    fn accepts_plugin_content_and_source_state_paths() {
        let allowed_paths = [
            "default-user/settings.json",
            "default-user/extensions/local-ext/index.js",
            "extensions/third-party/global-ext/index.js",
            "_tauritavern/extension-sources/local/local-ext.json",
            "_tauritavern/extension-sources/global/global-ext.json",
        ];

        for path in allowed_paths {
            validate_relative_path(path)
                .unwrap_or_else(|error| panic!("expected {path} to be allowed: {error}"));
        }
    }

    #[test]
    fn rejects_paths_outside_scope_and_lan_sync_state() {
        let rejected_paths = [
            "extensions/index.js",
            "_tauritavern/extension-sources/index.json",
            "default-user/user/lan-sync/config.json",
            "_tauritavern/settings.json",
        ];

        for path in rejected_paths {
            let result = validate_relative_path(path);
            assert!(result.is_err(), "expected {path} to be rejected");
        }
    }
}
