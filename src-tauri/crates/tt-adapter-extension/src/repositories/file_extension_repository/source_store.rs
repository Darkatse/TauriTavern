use std::path::{Path, PathBuf};

use serde::Deserialize;
use tokio::fs as tokio_fs;

use tt_adapter_storage_core::file_system::read_json_file;
use tt_domain::errors::DomainError;

const GITHUB_HOST: &str = "github.com";
const INLINE_SOURCE_FILE: &str = ".tauritavern-source.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ExtensionStoreScope {
    Local,
    Global,
}

impl ExtensionStoreScope {
    pub(super) fn from_global(global: bool) -> Self {
        if global { Self::Global } else { Self::Local }
    }

    pub(super) fn from_location(location: &str) -> Result<Self, DomainError> {
        match location {
            "local" => Ok(Self::Local),
            "global" => Ok(Self::Global),
            _ => Err(DomainError::InvalidData(format!(
                "Invalid extension location: {}",
                location
            ))),
        }
    }
}

#[derive(Deserialize)]
pub(super) struct ExtensionSourceMetadata {
    pub(super) reference: String,
    pub(super) remote_url: String,
    pub(super) installed_commit: String,
}

#[derive(Deserialize)]
struct LegacyGithubSourceMetadata {
    owner: String,
    repo: String,
    reference: String,
    installed_commit: String,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum StoredSourceMetadata {
    V2(ExtensionSourceMetadata),
    V1(LegacyGithubSourceMetadata),
}

impl StoredSourceMetadata {
    fn into_v2(self) -> ExtensionSourceMetadata {
        match self {
            StoredSourceMetadata::V2(metadata) => metadata,
            StoredSourceMetadata::V1(legacy) => {
                let repo_path = format!("{}/{}", legacy.owner, legacy.repo);
                ExtensionSourceMetadata {
                    reference: legacy.reference,
                    remote_url: format!("https://{GITHUB_HOST}/{repo_path}"),
                    installed_commit: legacy.installed_commit,
                }
            }
        }
    }
}

pub(super) struct ExtensionSourceStore {
    local_root: PathBuf,
    global_root: PathBuf,
}

impl ExtensionSourceStore {
    pub(super) fn new(root: PathBuf) -> Self {
        Self {
            local_root: root.join("local"),
            global_root: root.join("global"),
        }
    }

    fn scope_root(&self, scope: ExtensionStoreScope) -> &Path {
        match scope {
            ExtensionStoreScope::Local => &self.local_root,
            ExtensionStoreScope::Global => &self.global_root,
        }
    }

    fn record_path(&self, scope: ExtensionStoreScope, extension_name: &str) -> PathBuf {
        self.scope_root(scope)
            .join(format!("{}.json", extension_name))
    }

    pub(super) async fn read(
        &self,
        scope: ExtensionStoreScope,
        extension_name: &str,
        extension_path: &Path,
    ) -> Result<Option<ExtensionSourceMetadata>, DomainError> {
        let central_path = self.record_path(scope, extension_name);
        let path = if central_path.exists() {
            central_path
        } else {
            extension_path.join(INLINE_SOURCE_FILE)
        };
        if !path.exists() {
            return Ok(None);
        }

        let stored: StoredSourceMetadata = read_json_file(&path).await?;
        Ok(Some(stored.into_v2()))
    }

    pub(super) async fn delete(
        &self,
        scope: ExtensionStoreScope,
        extension_name: &str,
    ) -> Result<(), DomainError> {
        let path = self.record_path(scope, extension_name);
        if !path.exists() {
            return Ok(());
        }

        tokio_fs::remove_file(&path).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to delete extension source state '{}': {}",
                path.display(),
                error
            ))
        })
    }

    pub(super) async fn move_record(
        &self,
        source_scope: ExtensionStoreScope,
        destination_scope: ExtensionStoreScope,
        extension_name: &str,
    ) -> Result<(), DomainError> {
        let source_path = self.record_path(source_scope, extension_name);
        if !source_path.exists() {
            return Ok(());
        }

        let destination_path = self.record_path(destination_scope, extension_name);
        tokio_fs::rename(&source_path, &destination_path)
            .await
            .map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to move extension source state from '{}' to '{}': {}",
                    source_path.display(),
                    destination_path.display(),
                    error
                ))
            })
    }
}
