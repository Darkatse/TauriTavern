use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tokio::fs as tokio_fs;

use tt_adapter_storage_core::file_system::read_json_file;
use tt_domain::errors::DomainError;

use super::SOURCE_METADATA_FILE;
use super::providers::HOST_GITHUB;

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

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub(super) struct ExtensionSourceMetadata {
    pub(super) host: String,
    /// `owner/repo` for GitHub/Gitee, `group/subgroup/repo` for GitLab.
    pub(super) repo_path: String,
    pub(super) reference: String,
    pub(super) remote_url: String,
    pub(super) installed_commit: String,
}

#[derive(Debug, Deserialize)]
struct LegacyGithubSourceMetadata {
    owner: String,
    repo: String,
    reference: String,
    installed_commit: String,
}

#[derive(Debug, Deserialize)]
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
                    host: HOST_GITHUB.to_string(),
                    repo_path: repo_path.clone(),
                    reference: legacy.reference,
                    remote_url: format!("https://{}/{}", HOST_GITHUB, repo_path),
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
        let local_root = root.join("local");
        let global_root = root.join("global");
        Self {
            local_root,
            global_root,
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

    pub(super) fn legacy_record_path(extension_path: &Path) -> PathBuf {
        extension_path.join(SOURCE_METADATA_FILE)
    }

    pub(super) async fn read(
        &self,
        scope: ExtensionStoreScope,
        extension_name: &str,
    ) -> Result<Option<ExtensionSourceMetadata>, DomainError> {
        let path = self.record_path(scope, extension_name);
        if !path.exists() {
            return Ok(None);
        }

        let stored: StoredSourceMetadata = read_json_file(&path).await?;
        Ok(Some(stored.into_v2()))
    }

    pub(super) fn read_sync(
        &self,
        scope: ExtensionStoreScope,
        extension_name: &str,
    ) -> Result<Option<ExtensionSourceMetadata>, DomainError> {
        let path = self.record_path(scope, extension_name);
        if !path.exists() {
            return Ok(None);
        }

        let contents = fs::read_to_string(&path).map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read extension source state '{}': {}",
                path.display(),
                error
            ))
        })?;

        let stored = serde_json::from_str::<StoredSourceMetadata>(&contents).map_err(|error| {
            DomainError::InvalidData(format!(
                "Invalid extension source state '{}': {}",
                path.display(),
                error
            ))
        })?;

        Ok(Some(stored.into_v2()))
    }

    pub(super) async fn write(
        &self,
        scope: ExtensionStoreScope,
        extension_name: &str,
        metadata: &ExtensionSourceMetadata,
    ) -> Result<(), DomainError> {
        let path = self.record_path(scope, extension_name);
        let serialized = serde_json::to_string_pretty(metadata).map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to serialize extension source state '{}': {}",
                path.display(),
                error
            ))
        })?;

        tokio_fs::write(&path, serialized).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to write extension source state '{}': {}",
                path.display(),
                error
            ))
        })
    }

    pub(super) fn write_sync(
        &self,
        scope: ExtensionStoreScope,
        extension_name: &str,
        metadata: &ExtensionSourceMetadata,
    ) -> Result<(), DomainError> {
        let path = self.record_path(scope, extension_name);
        let serialized = serde_json::to_string_pretty(metadata).map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to serialize extension source state '{}': {}",
                path.display(),
                error
            ))
        })?;

        fs::write(&path, serialized).map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to write extension source state '{}': {}",
                path.display(),
                error
            ))
        })
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

    pub(super) fn migrate_all(
        &self,
        user_extensions_dir: &Path,
        global_extensions_dir: &Path,
    ) -> Result<(), DomainError> {
        self.migrate_scope(ExtensionStoreScope::Local, user_extensions_dir)?;
        self.migrate_scope(ExtensionStoreScope::Global, global_extensions_dir)?;
        Ok(())
    }

    fn migrate_scope(
        &self,
        scope: ExtensionStoreScope,
        extensions_dir: &Path,
    ) -> Result<(), DomainError> {
        if !extensions_dir.exists() {
            return Ok(());
        }

        let entries = fs::read_dir(extensions_dir).map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read extensions directory '{}': {}",
                extensions_dir.display(),
                error
            ))
        })?;

        for entry in entries {
            let entry = entry.map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to read extension directory entry in '{}': {}",
                    extensions_dir.display(),
                    error
                ))
            })?;

            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            let Some(file_name) = path.file_name() else {
                continue;
            };
            let extension_name = file_name.to_string_lossy().to_string();
            if extension_name.starts_with('.') {
                continue;
            }

            self.materialize_sync(scope, &extension_name, &path)?;
        }

        Ok(())
    }

    fn materialize_sync(
        &self,
        scope: ExtensionStoreScope,
        extension_name: &str,
        extension_path: &Path,
    ) -> Result<(), DomainError> {
        if extension_path.join(".git").exists() {
            return Ok(());
        }

        if self.read_sync(scope, extension_name)?.is_some() {
            self.delete_legacy_sync(extension_path)?;
            return Ok(());
        }

        if let Some(metadata) = self.read_legacy_sync(extension_path)? {
            self.write_sync(scope, extension_name, &metadata)?;
            self.delete_legacy_sync(extension_path)?;
            return Ok(());
        }

        Ok(())
    }

    pub(super) async fn resolve_or_migrate(
        &self,
        scope: ExtensionStoreScope,
        extension_name: &str,
        extension_path: &Path,
    ) -> Result<Option<ExtensionSourceMetadata>, DomainError> {
        if let Some(metadata) = self.read(scope, extension_name).await? {
            self.delete_legacy(extension_path).await?;
            return Ok(Some(metadata));
        }

        if let Some(metadata) = self.read_legacy(extension_path).await? {
            self.write(scope, extension_name, &metadata).await?;
            self.delete_legacy(extension_path).await?;
            return Ok(Some(metadata));
        }

        Ok(None)
    }

    async fn read_legacy(
        &self,
        extension_path: &Path,
    ) -> Result<Option<ExtensionSourceMetadata>, DomainError> {
        let path = Self::legacy_record_path(extension_path);
        if !path.exists() {
            return Ok(None);
        }

        let stored: StoredSourceMetadata = read_json_file(&path).await?;
        Ok(Some(stored.into_v2()))
    }

    fn read_legacy_sync(
        &self,
        extension_path: &Path,
    ) -> Result<Option<ExtensionSourceMetadata>, DomainError> {
        let path = Self::legacy_record_path(extension_path);
        if !path.exists() {
            return Ok(None);
        }

        let contents = fs::read_to_string(&path).map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read legacy extension source state '{}': {}",
                path.display(),
                error
            ))
        })?;

        let stored = serde_json::from_str::<StoredSourceMetadata>(&contents).map_err(|error| {
            DomainError::InvalidData(format!(
                "Invalid legacy extension source state '{}': {}",
                path.display(),
                error
            ))
        })?;

        Ok(Some(stored.into_v2()))
    }

    async fn delete_legacy(&self, extension_path: &Path) -> Result<(), DomainError> {
        let path = Self::legacy_record_path(extension_path);
        if !path.exists() {
            return Ok(());
        }

        tokio_fs::remove_file(&path).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to delete legacy extension source state '{}': {}",
                path.display(),
                error
            ))
        })
    }

    fn delete_legacy_sync(&self, extension_path: &Path) -> Result<(), DomainError> {
        let path = Self::legacy_record_path(extension_path);
        if !path.exists() {
            return Ok(());
        }

        fs::remove_file(&path).map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to delete legacy extension source state '{}': {}",
                path.display(),
                error
            ))
        })
    }
}
