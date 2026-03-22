use async_trait::async_trait;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::domain::errors::DomainError;
use crate::domain::models::extension::{
    Extension, ExtensionInstallResult, ExtensionManifestMetadata, ExtensionUpdateResult,
    ExtensionVersion,
};
use crate::domain::repositories::extension_repository::ExtensionRepository;
use crate::infrastructure::http_client_pool::HttpClientPool;
use crate::infrastructure::persistence::file_system::read_json_file;
use crate::infrastructure::third_party_paths::{
    parse_third_party_extension_folder_name, sanitize_third_party_extension_folder_name,
};

mod archive_zip;
mod delete;
mod discovery;
mod install;
mod move_op;
mod providers;
mod repo_url;
mod source_store;
mod update;
mod version;

#[cfg(test)]
mod tests;

use self::providers::ExtensionSourceProvider;
use self::providers::ExtensionSourceProviders;
use self::source_store::{ExtensionSourceMetadata, ExtensionSourceStore, ExtensionStoreScope};

pub struct FileExtensionRepository {
    user_extensions_dir: PathBuf,
    global_extensions_dir: PathBuf,
    source_store: ExtensionSourceStore,
    providers: ExtensionSourceProviders,
}

/// Built-in extensions enabled in TauriTavern.
/// Keep this list explicit so custom built-ins remain predictable after upstream sync.
const ENABLED_SYSTEM_EXTENSIONS: &[&str] = &[
    "regex",
    "code-render",
    "connection-manager",
    "data-migration",
    "attachments",
    "quick-reply",
    "stable-diffusion",
    "vectors",
    "tauritavern-version",
];
const SOURCE_METADATA_FILE: &str = ".tauritavern-source.json";

impl FileExtensionRepository {
    pub fn new(
        user_extensions_dir: PathBuf,
        global_extensions_dir: PathBuf,
        source_store_root: PathBuf,
        http_clients: Arc<HttpClientPool>,
    ) -> Result<Self, DomainError> {
        let source_store = ExtensionSourceStore::new(source_store_root);
        let providers = ExtensionSourceProviders::new(http_clients);
        let repository = Self {
            user_extensions_dir,
            global_extensions_dir,
            source_store,
            providers,
        };
        repository.source_store.migrate_all(
            &repository.user_extensions_dir,
            &repository.global_extensions_dir,
        )?;

        Ok(repository)
    }

    fn extension_base_dir(&self, global: bool) -> &Path {
        if global {
            &self.global_extensions_dir
        } else {
            &self.user_extensions_dir
        }
    }

    fn extension_dir_for_scope(&self, scope: ExtensionStoreScope) -> &Path {
        match scope {
            ExtensionStoreScope::Local => &self.user_extensions_dir,
            ExtensionStoreScope::Global => &self.global_extensions_dir,
        }
    }

    fn extension_folder_name_from_identifier(
        &self,
        extension_name: &str,
    ) -> Result<String, DomainError> {
        parse_third_party_extension_folder_name(extension_name).map_err(|_| {
            DomainError::InvalidData(format!("Invalid extension name: {}", extension_name))
        })
    }

    fn install_folder_name_from_repo_name(repo_name: &str) -> Result<String, DomainError> {
        sanitize_third_party_extension_folder_name(repo_name).map_err(|_| {
            DomainError::InvalidData(format!("Invalid extension repository name: {}", repo_name))
        })
    }

    fn resolve_extension_path(&self, extension_folder_name: &str, global: bool) -> PathBuf {
        self.extension_base_dir(global).join(extension_folder_name)
    }

    async fn read_manifest_metadata(
        &self,
        extension_path: &Path,
    ) -> Result<Option<ExtensionManifestMetadata>, DomainError> {
        let manifest_path = extension_path.join("manifest.json");
        if !manifest_path.exists() {
            return Ok(None);
        }

        let manifest: ExtensionManifestMetadata = read_json_file(&manifest_path).await?;
        Ok(Some(manifest))
    }

    async fn resolve_source_metadata(
        &self,
        scope: ExtensionStoreScope,
        extension_name: &str,
        extension_path: &Path,
    ) -> Result<Option<ExtensionSourceMetadata>, DomainError> {
        self.source_store
            .resolve_or_migrate(scope, extension_name, extension_path)
            .await
    }

    async fn stage_extension_snapshot(
        &self,
        provider: &dyn ExtensionSourceProvider,
        repo_path: &str,
        commit: &str,
        base_dir: &Path,
        temp_prefix: &str,
    ) -> Result<(PathBuf, ExtensionManifestMetadata), DomainError> {
        let staging_dir = self.create_temp_directory(base_dir, temp_prefix).await?;

        let result: Result<ExtensionManifestMetadata, DomainError> = async {
            let archive_bytes = provider.download_archive_zip(repo_path, commit).await?;
            self.extract_zip_bytes(archive_bytes.as_ref(), &staging_dir)?;
            self.required_manifest_metadata(&staging_dir).await
        }
        .await;

        match result {
            Ok(manifest) => Ok((staging_dir, manifest)),
            Err(error) => {
                Self::cleanup_temp_directory(&staging_dir).await;
                Err(error)
            }
        }
    }
}

#[async_trait]
impl ExtensionRepository for FileExtensionRepository {
    async fn discover_extensions(&self) -> Result<Vec<Extension>, DomainError> {
        discovery::discover_extensions(self).await
    }

    async fn get_manifest_metadata(
        &self,
        extension_path: &Path,
    ) -> Result<Option<ExtensionManifestMetadata>, DomainError> {
        self.read_manifest_metadata(extension_path).await
    }

    async fn install_extension(
        &self,
        url: &str,
        global: bool,
        branch: Option<String>,
    ) -> Result<ExtensionInstallResult, DomainError> {
        install::install_extension(self, url, global, branch).await
    }

    async fn update_extension(
        &self,
        extension_name: &str,
        global: bool,
    ) -> Result<ExtensionUpdateResult, DomainError> {
        update::update_extension(self, extension_name, global).await
    }

    async fn delete_extension(
        &self,
        extension_name: &str,
        global: bool,
    ) -> Result<(), DomainError> {
        delete::delete_extension(self, extension_name, global).await
    }

    async fn get_extension_version(
        &self,
        extension_name: &str,
        global: bool,
    ) -> Result<ExtensionVersion, DomainError> {
        version::get_extension_version(self, extension_name, global).await
    }

    async fn move_extension(
        &self,
        extension_name: &str,
        source: &str,
        destination: &str,
    ) -> Result<(), DomainError> {
        move_op::move_extension(self, extension_name, source, destination).await
    }
}
