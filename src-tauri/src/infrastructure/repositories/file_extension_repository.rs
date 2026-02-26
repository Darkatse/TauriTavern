use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use reqwest::Client;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use tokio::fs as tokio_fs;
use url::Url;
use uuid::Uuid;

use crate::domain::errors::DomainError;
use crate::domain::models::extension::{
    Extension, ExtensionAssetPayload, ExtensionInstallResult, ExtensionManifest, ExtensionType,
    ExtensionUpdateResult, ExtensionVersion,
};
use crate::domain::repositories::extension_repository::ExtensionRepository;
use crate::infrastructure::http_client::build_http_client;
use crate::infrastructure::logging::logger;
use crate::infrastructure::persistence::file_system::read_json_file;

mod github;
mod metadata;
mod snapshot;

use self::snapshot::copy_dir_all;

pub struct FileExtensionRepository {
    http_client: Client,
    user_extensions_dir: PathBuf,
    global_extensions_dir: PathBuf,
}

/// Built-in extensions enabled in TauriTavern.
/// Keep this list explicit so custom built-ins remain predictable after upstream sync.
const ENABLED_SYSTEM_EXTENSIONS: &[&str] = &[
    "regex",
    "code-render",
    "data-migration",
    "quick-reply",
    "tauritavern-version",
    "lan-sync",
];
const GITHUB_API_BASE: &str = "https://api.github.com";
const SOURCE_METADATA_FILE: &str = ".tauritavern-source.json";

#[derive(Debug, Deserialize)]
struct GithubRepositoryInfo {
    default_branch: String,
}

#[derive(Debug, Deserialize)]
struct GithubCommit {
    sha: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct ExtensionSourceMetadata {
    owner: String,
    repo: String,
    reference: String,
    remote_url: String,
    installed_commit: String,
}

#[derive(Debug)]
struct GithubRepoLocation {
    owner: String,
    repo: String,
    reference_from_url: Option<String>,
}

impl FileExtensionRepository {
    pub fn new(user_extensions_dir: PathBuf, global_extensions_dir: PathBuf) -> Self {
        // Create directories if they don't exist
        fs::create_dir_all(&user_extensions_dir)
            .expect("Failed to create user extensions directory");
        fs::create_dir_all(&global_extensions_dir)
            .expect("Failed to create global extensions directory");

        let http_client =
            build_http_client(Client::builder()).expect("Failed to build extension HTTP client");

        Self {
            http_client,
            user_extensions_dir,
            global_extensions_dir,
        }
    }

    fn extension_base_dir(&self, global: bool) -> &Path {
        if global {
            &self.global_extensions_dir
        } else {
            &self.user_extensions_dir
        }
    }

    fn source_metadata_path(extension_path: &Path) -> PathBuf {
        extension_path.join(SOURCE_METADATA_FILE)
    }

    fn sanitize_filename(filename: &str) -> String {
        filename
            .chars()
            .map(|c| match c {
                '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
                _ => c,
            })
            .collect()
    }

    fn normalize_extension_name(&self, extension_name: &str) -> Result<String, DomainError> {
        let normalized = extension_name.trim().replace('\\', "/");
        let normalized = normalized.trim_matches('/');
        let normalized = normalized
            .strip_prefix("third-party/")
            .unwrap_or(normalized);

        if normalized.is_empty() || normalized.contains("..") {
            return Err(DomainError::InvalidData(format!(
                "Invalid extension name: {}",
                extension_name
            )));
        }

        let sanitized = Self::sanitize_filename(normalized);
        if sanitized.trim().is_empty() {
            return Err(DomainError::InvalidData(format!(
                "Invalid extension name: {}",
                extension_name
            )));
        }

        Ok(sanitized)
    }

    fn normalize_asset_relative_path(relative_path: &str) -> Result<PathBuf, DomainError> {
        let normalized = relative_path.trim().replace('\\', "/");
        let normalized = normalized.trim_matches('/');
        if normalized.is_empty() {
            return Err(DomainError::InvalidData(
                "Extension asset path cannot be empty".to_string(),
            ));
        }

        let mut sanitized = PathBuf::new();
        for segment in normalized.split('/') {
            if segment.is_empty() || segment == "." || segment == ".." {
                return Err(DomainError::InvalidData(format!(
                    "Invalid extension asset path: {}",
                    relative_path
                )));
            }
            sanitized.push(segment);
        }

        Ok(sanitized)
    }

    fn third_party_candidate_dirs<'a>(&'a self, location_hint: Option<&str>) -> [&'a Path; 2] {
        match location_hint.map(|value| value.to_ascii_lowercase()) {
            Some(ref value) if value == "global" => {
                [&self.global_extensions_dir, &self.user_extensions_dir]
            }
            _ => [&self.user_extensions_dir, &self.global_extensions_dir],
        }
    }

    fn resolve_extension_path(
        &self,
        extension_name: &str,
        global: bool,
    ) -> Result<PathBuf, DomainError> {
        let normalized_name = self.normalize_extension_name(extension_name)?;
        Ok(self.extension_base_dir(global).join(normalized_name))
    }
}

#[async_trait]
impl ExtensionRepository for FileExtensionRepository {
    async fn discover_extensions(&self) -> Result<Vec<Extension>, DomainError> {
        tracing::info!("Discovering extensions");

        let mut extensions = Vec::new();

        // Built-in extensions are explicitly allowlisted; unsupported modules stay disabled.
        for &name in ENABLED_SYSTEM_EXTENSIONS {
            let path = PathBuf::from(format!("scripts/extensions/{}", name));

            extensions.push(Extension {
                name: name.to_string(),
                extension_type: ExtensionType::System,
                manifest: None,
                path,
                remote_url: None,
                commit_hash: None,
                branch_name: None,
                is_up_to_date: None,
            });
        }

        // Get user extensions
        if self.user_extensions_dir.exists() {
            let entries = fs::read_dir(&self.user_extensions_dir).map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to read local extensions directory '{}': {}",
                    self.user_extensions_dir.display(),
                    error
                ))
            })?;

            for entry in entries {
                let entry = entry.map_err(|error| {
                    DomainError::InternalError(format!(
                        "Failed to read local extension directory entry: {}",
                        error
                    ))
                })?;

                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }

                let name = match path.file_name() {
                    Some(value) => value.to_string_lossy().to_string(),
                    None => continue,
                };

                if name.starts_with('.') {
                    continue;
                }

                let manifest = self.get_manifest(&path).await.ok().flatten();
                let source = self.read_source_metadata(&path).await.ok().flatten();

                extensions.push(Extension {
                    name: format!("third-party/{}", name),
                    extension_type: ExtensionType::Local,
                    manifest,
                    path,
                    remote_url: source.as_ref().map(|metadata| metadata.remote_url.clone()),
                    commit_hash: source
                        .as_ref()
                        .map(|metadata| metadata.installed_commit.clone()),
                    branch_name: source.as_ref().map(|metadata| metadata.reference.clone()),
                    is_up_to_date: None,
                });
            }
        }

        // Get global extensions
        if self.global_extensions_dir.exists() {
            let entries = fs::read_dir(&self.global_extensions_dir).map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to read global extensions directory '{}': {}",
                    self.global_extensions_dir.display(),
                    error
                ))
            })?;

            for entry in entries {
                let entry = entry.map_err(|error| {
                    DomainError::InternalError(format!(
                        "Failed to read global extension directory entry: {}",
                        error
                    ))
                })?;

                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }

                let name = match path.file_name() {
                    Some(value) => value.to_string_lossy().to_string(),
                    None => continue,
                };

                if name.starts_with('.') {
                    continue;
                }

                // In case of conflict, the extension in user scope takes precedence.
                if extensions
                    .iter()
                    .any(|extension| extension.name == format!("third-party/{}", name))
                {
                    continue;
                }

                let manifest = self.get_manifest(&path).await.ok().flatten();
                let source = self.read_source_metadata(&path).await.ok().flatten();

                extensions.push(Extension {
                    name: format!("third-party/{}", name),
                    extension_type: ExtensionType::Global,
                    manifest,
                    path,
                    remote_url: source.as_ref().map(|metadata| metadata.remote_url.clone()),
                    commit_hash: source
                        .as_ref()
                        .map(|metadata| metadata.installed_commit.clone()),
                    branch_name: source.as_ref().map(|metadata| metadata.reference.clone()),
                    is_up_to_date: None,
                });
            }
        }

        logger::debug(&format!("Discovered {} extensions", extensions.len()));
        Ok(extensions)
    }

    async fn get_manifest(
        &self,
        extension_path: &Path,
    ) -> Result<Option<ExtensionManifest>, DomainError> {
        let manifest_path = extension_path.join("manifest.json");
        if !manifest_path.exists() {
            return Ok(None);
        }

        let manifest: ExtensionManifest = read_json_file(&manifest_path).await?;
        Ok(Some(manifest))
    }

    async fn install_extension(
        &self,
        url: &str,
        global: bool,
        branch: Option<String>,
    ) -> Result<ExtensionInstallResult, DomainError> {
        tracing::info!("Installing extension from {}", url);

        let repo = self.parse_github_repo_url(url)?;
        let reference = Self::normalize_requested_reference(branch)
            .or(repo.reference_from_url.clone())
            .unwrap_or(
                self.github_get_default_branch(&repo.owner, &repo.repo)
                    .await?,
            );
        let latest_commit = self
            .github_get_latest_commit_hash(&repo.owner, &repo.repo, &reference)
            .await?;

        let base_dir = self.extension_base_dir(global);
        let extension_folder_name = Self::sanitize_filename(&repo.repo);
        let extension_path = base_dir.join(&extension_folder_name);

        if extension_path.exists() {
            return Err(DomainError::InvalidData(format!(
                "Extension already exists at '{}'",
                extension_path.display()
            )));
        }

        let staging_dir = self
            .create_temp_directory(base_dir, "extension-install")
            .await?;
        if let Err(error) = self
            .download_and_extract_snapshot(&repo.owner, &repo.repo, &latest_commit, &staging_dir)
            .await
        {
            Self::cleanup_temp_directory(&staging_dir).await;
            return Err(error);
        }

        let manifest = match self.required_manifest(&staging_dir).await {
            Ok(manifest) => manifest,
            Err(error) => {
                Self::cleanup_temp_directory(&staging_dir).await;
                return Err(error);
            }
        };

        fs::rename(&staging_dir, &extension_path).map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to finalize extension installation into '{}': {}",
                extension_path.display(),
                error
            ))
        })?;

        let source_metadata = ExtensionSourceMetadata {
            owner: repo.owner.clone(),
            repo: repo.repo.clone(),
            reference: reference.clone(),
            remote_url: format!("https://github.com/{}/{}", repo.owner, repo.repo),
            installed_commit: latest_commit.clone(),
        };
        self.write_source_metadata(&extension_path, &source_metadata)
            .await?;

        tracing::info!(
            "Extension installed: {} v{} by {} ({})",
            manifest.display_name,
            manifest.version,
            manifest.author,
            extension_path.display()
        );

        Ok(ExtensionInstallResult {
            version: manifest.version,
            author: manifest.author,
            display_name: manifest.display_name,
            extension_path: extension_path.to_string_lossy().to_string(),
        })
    }

    async fn update_extension(
        &self,
        extension_name: &str,
        global: bool,
    ) -> Result<ExtensionUpdateResult, DomainError> {
        tracing::info!("Updating extension: {}", extension_name);

        let extension_path = self.resolve_extension_path(extension_name, global)?;
        if !extension_path.exists() {
            return Err(DomainError::NotFound(format!(
                "Extension not found at '{}'",
                extension_path.display()
            )));
        }

        let mut source = self
            .resolve_source_metadata(&extension_path)
            .await?
            .ok_or_else(|| {
                DomainError::InvalidData(
                    "Extension source metadata is missing. Reinstall this extension to enable updates."
                        .to_string(),
                )
            })?;

        let latest_commit = self
            .github_get_latest_commit_hash(&source.owner, &source.repo, &source.reference)
            .await?;
        let is_up_to_date = source.installed_commit == latest_commit;

        if !is_up_to_date {
            let base_dir = extension_path.parent().ok_or_else(|| {
                DomainError::InternalError(format!(
                    "Failed to resolve parent directory for '{}'",
                    extension_path.display()
                ))
            })?;
            let staging_dir = self
                .create_temp_directory(base_dir, "extension-update")
                .await?;

            if let Err(error) = self
                .download_and_extract_snapshot(
                    &source.owner,
                    &source.repo,
                    &latest_commit,
                    &staging_dir,
                )
                .await
            {
                Self::cleanup_temp_directory(&staging_dir).await;
                return Err(error);
            }

            if let Err(error) = self.required_manifest(&staging_dir).await {
                Self::cleanup_temp_directory(&staging_dir).await;
                return Err(error);
            }

            if let Err(error) = self.replace_directory(&staging_dir, &extension_path) {
                Self::cleanup_temp_directory(&staging_dir).await;
                return Err(error);
            }

            source.installed_commit = latest_commit.clone();
            self.write_source_metadata(&extension_path, &source).await?;
        }

        let short_commit_hash = Self::short_commit_hash(&latest_commit);

        Ok(ExtensionUpdateResult {
            short_commit_hash,
            extension_path: extension_path.to_string_lossy().to_string(),
            is_up_to_date,
            remote_url: source.remote_url,
        })
    }

    async fn delete_extension(
        &self,
        extension_name: &str,
        global: bool,
    ) -> Result<(), DomainError> {
        tracing::info!("Deleting extension: {}", extension_name);

        let extension_path = self.resolve_extension_path(extension_name, global)?;
        if !extension_path.exists() {
            return Err(DomainError::NotFound(format!(
                "Extension not found at '{}'",
                extension_path.display()
            )));
        }

        tokio_fs::remove_dir_all(&extension_path)
            .await
            .map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to delete extension directory '{}': {}",
                    extension_path.display(),
                    error
                ))
            })?;

        tracing::info!("Extension deleted: {}", extension_name);
        Ok(())
    }

    async fn get_extension_version(
        &self,
        extension_name: &str,
        global: bool,
    ) -> Result<ExtensionVersion, DomainError> {
        tracing::info!("Getting extension version: {}", extension_name);

        let extension_path = self.resolve_extension_path(extension_name, global)?;
        if !extension_path.exists() {
            return Err(DomainError::NotFound(format!(
                "Extension not found at '{}'",
                extension_path.display()
            )));
        }

        let source = match self.resolve_source_metadata(&extension_path).await? {
            Some(source) => source,
            None => {
                // Keep behavior close to upstream: for non-managed directories,
                // version endpoint returns an empty Git state instead of failing.
                return Ok(ExtensionVersion {
                    current_branch_name: String::new(),
                    current_commit_hash: String::new(),
                    is_up_to_date: true,
                    remote_url: String::new(),
                });
            }
        };

        let latest_commit = self
            .github_get_latest_commit_hash(&source.owner, &source.repo, &source.reference)
            .await?;
        let is_up_to_date = source.installed_commit == latest_commit;

        Ok(ExtensionVersion {
            current_branch_name: source.reference,
            current_commit_hash: source.installed_commit,
            is_up_to_date,
            remote_url: source.remote_url,
        })
    }

    async fn move_extension(
        &self,
        extension_name: &str,
        source: &str,
        destination: &str,
    ) -> Result<(), DomainError> {
        tracing::info!(
            "Moving extension: {} from {} to {}",
            extension_name,
            source,
            destination
        );

        if source == destination {
            return Err(DomainError::InvalidData(
                "Source and destination are the same".to_string(),
            ));
        }

        let extension_folder_name = self.normalize_extension_name(extension_name)?;
        let source_dir = self.resolve_move_dir(source)?;
        let destination_dir = self.resolve_move_dir(destination)?;

        let source_path = source_dir.join(&extension_folder_name);
        let destination_path = destination_dir.join(&extension_folder_name);

        if !source_path.exists() {
            return Err(DomainError::NotFound(format!(
                "Source extension does not exist at '{}'",
                source_path.display()
            )));
        }

        if destination_path.exists() {
            return Err(DomainError::InvalidData(format!(
                "Destination extension already exists at '{}'",
                destination_path.display()
            )));
        }

        copy_dir_all(&source_path, &destination_path).map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to copy extension from '{}' to '{}': {}",
                source_path.display(),
                destination_path.display(),
                error
            ))
        })?;

        tokio_fs::remove_dir_all(&source_path)
            .await
            .map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to delete old extension location '{}': {}",
                    source_path.display(),
                    error
                ))
            })?;

        tracing::info!(
            "Extension moved: {} from {} to {}",
            extension_folder_name,
            source,
            destination
        );
        Ok(())
    }

    async fn read_third_party_asset(
        &self,
        extension_name: &str,
        relative_path: &str,
        location_hint: Option<&str>,
    ) -> Result<ExtensionAssetPayload, DomainError> {
        let extension_folder_name = self.normalize_extension_name(extension_name)?;
        let normalized_relative = Self::normalize_asset_relative_path(relative_path)?;

        for base_dir in self.third_party_candidate_dirs(location_hint) {
            let extension_root = base_dir.join(&extension_folder_name);
            let asset_path = extension_root.join(&normalized_relative);

            if !asset_path.is_file() {
                continue;
            }

            let bytes = tokio_fs::read(&asset_path).await.map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to read extension asset '{}': {}",
                    asset_path.display(),
                    error
                ))
            })?;

            let mime_type = mime_guess::from_path(&asset_path)
                .first_or_octet_stream()
                .essence_str()
                .to_string();

            return Ok(ExtensionAssetPayload {
                content_base64: BASE64_STANDARD.encode(bytes),
                mime_type,
            });
        }

        Err(DomainError::NotFound(format!(
            "Third-party extension asset not found: {}/{}",
            extension_name, relative_path
        )))
    }
}
