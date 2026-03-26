use std::path::{Path, PathBuf};

use crate::domain::errors::DomainError;

#[derive(Debug, Clone)]
pub struct ThirdPartyExtensionDirs {
    pub local_dir: PathBuf,
    pub global_dir: PathBuf,
}

impl ThirdPartyExtensionDirs {
    pub fn from_data_root(data_root: impl AsRef<Path>) -> Self {
        let data_root = data_root.as_ref();
        Self {
            local_dir: data_root.join("default-user").join("extensions"),
            global_dir: data_root.join("extensions").join("third-party"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ResolvedThirdPartyAsset {
    pub path: PathBuf,
    pub mime_type: String,
    pub size_bytes: u64,
}

pub fn resolve_third_party_extension_asset(
    local_extensions_dir: &Path,
    global_extensions_dir: &Path,
    extension_folder: &str,
    relative_path: &Path,
) -> Result<ResolvedThirdPartyAsset, DomainError> {
    for base_dir in [local_extensions_dir, global_extensions_dir] {
        let extension_root = base_dir.join(extension_folder);
        let asset_path = extension_root.join(relative_path);

        let metadata = match std::fs::metadata(&asset_path) {
            Ok(value) => value,
            Err(error) => match error.kind() {
                std::io::ErrorKind::NotFound => continue,
                _ => {
                    return Err(DomainError::InternalError(format!(
                        "Failed to stat third-party extension asset ({}): {}",
                        asset_path.display(),
                        error
                    )));
                }
            },
        };

        if !metadata.is_file() {
            continue;
        }

        let mime_type = mime_guess::from_path(&asset_path)
            .first_or_octet_stream()
            .essence_str()
            .to_string();

        return Ok(ResolvedThirdPartyAsset {
            path: asset_path,
            mime_type,
            size_bytes: metadata.len(),
        });
    }

    Err(DomainError::NotFound(format!(
        "Third-party extension asset not found: {}/{}",
        extension_folder,
        relative_path.display()
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn prefers_local_extension_over_global() {
        let temp = TempDirGuard::new("third-party-assets-local-overrides-global");
        let local_root = temp.path.join("local");
        let global_root = temp.path.join("global");
        let extension_folder = "example-ext";

        std::fs::create_dir_all(local_root.join(extension_folder)).expect("create local extension");
        std::fs::create_dir_all(global_root.join(extension_folder))
            .expect("create global extension");

        std::fs::write(
            local_root.join(extension_folder).join("manifest.json"),
            br#"{ "source": "local" }"#,
        )
        .expect("write local file");
        std::fs::write(
            global_root.join(extension_folder).join("manifest.json"),
            br#"{ "source": "global" }"#,
        )
        .expect("write global file");

        let resolved = resolve_third_party_extension_asset(
            &local_root,
            &global_root,
            extension_folder,
            Path::new("manifest.json"),
        )
        .expect("resolve manifest");

        assert_eq!(
            resolved.path,
            local_root.join(extension_folder).join("manifest.json")
        );
        assert_eq!(resolved.mime_type, "application/json");
        assert_eq!(
            resolved.size_bytes,
            br#"{ "source": "local" }"#.len() as u64
        );
    }

    #[test]
    fn returns_not_found_when_asset_missing() {
        let temp = TempDirGuard::new("third-party-assets-not-found");
        let local_root = temp.path.join("local");
        let global_root = temp.path.join("global");
        let extension_folder = "missing-ext";

        std::fs::create_dir_all(local_root.join(extension_folder)).expect("create extension");

        let result = resolve_third_party_extension_asset(
            &local_root,
            &global_root,
            extension_folder,
            Path::new("missing.js"),
        );

        assert!(matches!(result, Err(DomainError::NotFound(_))));
    }
}
