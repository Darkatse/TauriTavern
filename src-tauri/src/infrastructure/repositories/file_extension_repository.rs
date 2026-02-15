use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use reqwest::Client;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use tauri::AppHandle;
use tokio::fs as tokio_fs;
use url::Url;
use uuid::Uuid;

use crate::domain::errors::DomainError;
use crate::domain::models::extension::{
    Extension, ExtensionAssetPayload, ExtensionInstallResult, ExtensionManifest, ExtensionType,
    ExtensionUpdateResult, ExtensionVersion,
};
use crate::domain::repositories::extension_repository::ExtensionRepository;
use crate::infrastructure::logging::logger;
use crate::infrastructure::paths::resolve_app_data_dir;
use crate::infrastructure::persistence::file_system::read_json_file;

pub struct FileExtensionRepository {
    http_client: Client,
    user_extensions_dir: PathBuf,
    global_extensions_dir: PathBuf,
    system_extensions_dir: PathBuf,
}

/// Built-in extensions enabled in TauriTavern.
/// Keep this list explicit so unsupported built-ins stay disabled by default.
const ENABLED_SYSTEM_EXTENSIONS: &[&str] = &["regex", "code-render", "data-migration"];
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
    pub fn new(app_handle: AppHandle) -> Self {
        // Get app data directory
        let app_data_dir =
            resolve_app_data_dir(&app_handle).expect("Failed to get app data directory");

        // Construct extension directories
        let data_root = app_data_dir.join("data");
        let user_extensions_dir = data_root.join("default-user").join("extensions");
        let global_extensions_dir = data_root.join("extensions").join("third-party");
        let system_extensions_dir = data_root.join("extensions");

        // Create directories if they don't exist
        fs::create_dir_all(&user_extensions_dir)
            .expect("Failed to create user extensions directory");
        fs::create_dir_all(&global_extensions_dir)
            .expect("Failed to create global extensions directory");
        fs::create_dir_all(&system_extensions_dir)
            .expect("Failed to create system extensions directory");

        let http_client = Client::builder()
            .user_agent("TauriTavern")
            .build()
            .unwrap_or_else(|_| Client::new());

        Self {
            http_client,
            user_extensions_dir,
            global_extensions_dir,
            system_extensions_dir,
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

    fn parse_github_repo_url(&self, url: &str) -> Result<GithubRepoLocation, DomainError> {
        let parsed_url = Url::parse(url).map_err(|error| {
            DomainError::InvalidData(format!("Invalid GitHub URL '{}': {}", url, error))
        })?;

        let host = parsed_url
            .host_str()
            .unwrap_or_default()
            .to_ascii_lowercase();

        if host != "github.com" && host != "www.github.com" {
            return Err(DomainError::InvalidData(
                "Only GitHub repositories are supported".to_string(),
            ));
        }

        let path_segments = parsed_url
            .path_segments()
            .ok_or_else(|| DomainError::InvalidData("Invalid GitHub URL path".to_string()))?
            .filter(|segment| !segment.is_empty())
            .map(ToString::to_string)
            .collect::<Vec<String>>();

        if path_segments.len() < 2 {
            return Err(DomainError::InvalidData(
                "GitHub URL must include owner and repository".to_string(),
            ));
        }

        let owner = path_segments[0].trim().to_string();
        let repo = path_segments[1].trim_end_matches(".git").trim().to_string();

        if owner.is_empty() || repo.is_empty() {
            return Err(DomainError::InvalidData(
                "GitHub owner/repository cannot be empty".to_string(),
            ));
        }

        let reference_from_url = if path_segments.len() >= 4 && path_segments[2] == "tree" {
            let reference = path_segments[3..].join("/");
            if reference.is_empty() {
                None
            } else {
                Some(reference)
            }
        } else {
            parsed_url
                .query_pairs()
                .find(|(key, _)| key == "ref")
                .map(|(_, value)| value.trim().to_string())
                .filter(|value| !value.is_empty())
        };

        Ok(GithubRepoLocation {
            owner,
            repo,
            reference_from_url,
        })
    }

    fn normalize_requested_reference(reference: Option<String>) -> Option<String> {
        reference
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    }

    fn build_github_api_url(&self, segments: &[&str]) -> Result<Url, DomainError> {
        let mut url = Url::parse(GITHUB_API_BASE).map_err(|error| {
            DomainError::InternalError(format!("Failed to parse GitHub API base URL: {}", error))
        })?;

        {
            let mut path_segments = url.path_segments_mut().map_err(|_| {
                DomainError::InternalError("Failed to mutate GitHub API URL".to_string())
            })?;
            path_segments.clear();
            for segment in segments {
                path_segments.push(segment);
            }
        }

        Ok(url)
    }

    async fn github_get_json<T>(&self, segments: &[&str]) -> Result<T, DomainError>
    where
        T: DeserializeOwned,
    {
        let url = self.build_github_api_url(segments)?;

        let response = self
            .http_client
            .get(url.clone())
            .header("Accept", "application/vnd.github+json")
            .send()
            .await
            .map_err(|error| {
                DomainError::InternalError(format!("GitHub request failed: {}", error))
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            let snippet = body.trim();
            let suffix = if snippet.is_empty() {
                String::new()
            } else {
                format!(" ({})", snippet)
            };
            return Err(DomainError::InternalError(format!(
                "GitHub request failed for '{}': HTTP {}{}",
                url, status, suffix
            )));
        }

        response.json::<T>().await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to parse GitHub response for '{}': {}",
                url, error
            ))
        })
    }

    async fn github_get_default_branch(
        &self,
        owner: &str,
        repo: &str,
    ) -> Result<String, DomainError> {
        let info: GithubRepositoryInfo = self.github_get_json(&["repos", owner, repo]).await?;
        if info.default_branch.trim().is_empty() {
            return Err(DomainError::InternalError(format!(
                "Repository '{}/{}' has no default branch",
                owner, repo
            )));
        }

        Ok(info.default_branch)
    }

    async fn github_get_latest_commit_hash(
        &self,
        owner: &str,
        repo: &str,
        reference: &str,
    ) -> Result<String, DomainError> {
        let commit: GithubCommit = self
            .github_get_json(&["repos", owner, repo, "commits", reference])
            .await?;

        if commit.sha.trim().is_empty() {
            return Err(DomainError::InternalError(format!(
                "Repository '{}/{}' returned an empty commit SHA for reference '{}'",
                owner, repo, reference
            )));
        }

        Ok(commit.sha)
    }

    async fn create_temp_directory(
        &self,
        parent: &Path,
        prefix: &str,
    ) -> Result<PathBuf, DomainError> {
        for _ in 0..8 {
            let candidate = parent.join(format!(".{}-{}", prefix, Uuid::new_v4()));
            if !candidate.exists() {
                tokio_fs::create_dir_all(&candidate)
                    .await
                    .map_err(|error| {
                        DomainError::InternalError(format!(
                            "Failed to create temporary directory '{}': {}",
                            candidate.display(),
                            error
                        ))
                    })?;
                return Ok(candidate);
            }
        }

        Err(DomainError::InternalError(
            "Failed to allocate temporary directory for extension operation".to_string(),
        ))
    }

    async fn cleanup_temp_directory(path: &Path) {
        if path.exists() {
            let _ = tokio_fs::remove_dir_all(path).await;
        }
    }

    fn strip_archive_root(path: &Path) -> Option<PathBuf> {
        let mut components = path.components();
        components.next()?;
        let remainder = components.as_path();

        if remainder.as_os_str().is_empty() {
            None
        } else {
            Some(remainder.to_path_buf())
        }
    }

    fn extract_zip_bytes(&self, bytes: &[u8], destination: &Path) -> Result<(), DomainError> {
        let reader = Cursor::new(bytes);
        let mut archive = zip::ZipArchive::new(reader).map_err(|error| {
            DomainError::InternalError(format!("Failed to read downloaded ZIP archive: {}", error))
        })?;

        for index in 0..archive.len() {
            let mut entry = archive.by_index(index).map_err(|error| {
                DomainError::InternalError(format!("Failed to read ZIP entry: {}", error))
            })?;

            // Skip entries that are not safely enclosed paths.
            let enclosed_path = match entry.enclosed_name() {
                Some(path) => path.to_path_buf(),
                None => continue,
            };

            // GitHub archives always wrap files in a top-level root folder.
            let relative_path = match Self::strip_archive_root(&enclosed_path) {
                Some(path) => path,
                None => continue,
            };

            let output_path = destination.join(relative_path);

            if entry.is_dir() {
                fs::create_dir_all(&output_path).map_err(|error| {
                    DomainError::InternalError(format!(
                        "Failed to create directory '{}': {}",
                        output_path.display(),
                        error
                    ))
                })?;
                continue;
            }

            if let Some(parent) = output_path.parent() {
                fs::create_dir_all(parent).map_err(|error| {
                    DomainError::InternalError(format!(
                        "Failed to create directory '{}': {}",
                        parent.display(),
                        error
                    ))
                })?;
            }

            let mut output_file = fs::File::create(&output_path).map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to create file '{}': {}",
                    output_path.display(),
                    error
                ))
            })?;

            std::io::copy(&mut entry, &mut output_file).map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to write file '{}': {}",
                    output_path.display(),
                    error
                ))
            })?;
        }

        Ok(())
    }

    async fn download_and_extract_snapshot(
        &self,
        owner: &str,
        repo: &str,
        commit_hash: &str,
        destination: &Path,
    ) -> Result<(), DomainError> {
        let url = self.build_github_api_url(&["repos", owner, repo, "zipball", commit_hash])?;

        let response = self
            .http_client
            .get(url.clone())
            .header("Accept", "application/vnd.github+json")
            .send()
            .await
            .map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to download extension archive: {}",
                    error
                ))
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            let snippet = body.trim();
            let suffix = if snippet.is_empty() {
                String::new()
            } else {
                format!(" ({})", snippet)
            };
            return Err(DomainError::InternalError(format!(
                "Failed to download extension archive from '{}': HTTP {}{}",
                url, status, suffix
            )));
        }

        let archive_bytes = response.bytes().await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read extension archive response: {}",
                error
            ))
        })?;

        self.extract_zip_bytes(archive_bytes.as_ref(), destination)
    }

    async fn read_source_metadata(
        &self,
        extension_path: &Path,
    ) -> Result<Option<ExtensionSourceMetadata>, DomainError> {
        let metadata_path = Self::source_metadata_path(extension_path);
        if !metadata_path.exists() {
            return Ok(None);
        }

        let metadata: ExtensionSourceMetadata = read_json_file(&metadata_path).await?;
        Ok(Some(metadata))
    }

    async fn write_source_metadata(
        &self,
        extension_path: &Path,
        metadata: &ExtensionSourceMetadata,
    ) -> Result<(), DomainError> {
        let metadata_path = Self::source_metadata_path(extension_path);
        let serialized = serde_json::to_string_pretty(metadata).map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to serialize extension source metadata for '{}': {}",
                extension_path.display(),
                error
            ))
        })?;

        tokio_fs::write(&metadata_path, serialized)
            .await
            .map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to write extension source metadata '{}': {}",
                    metadata_path.display(),
                    error
                ))
            })
    }

    fn parse_origin_remote_url(config: &str) -> Option<String> {
        let mut in_origin = false;

        for line in config.lines() {
            let trimmed = line.trim();

            if trimmed.starts_with('[') && trimmed.ends_with(']') {
                in_origin = trimmed == "[remote \"origin\"]";
                continue;
            }

            if !in_origin {
                continue;
            }

            if let Some((key, value)) = trimmed.split_once('=') {
                if key.trim() == "url" {
                    return Some(value.trim().to_string());
                }
            }
        }

        None
    }

    fn normalize_git_remote_url(remote_url: &str) -> String {
        let trimmed = remote_url.trim();

        if let Some(path) = trimmed.strip_prefix("git@github.com:") {
            return format!("https://github.com/{}", path);
        }

        if let Some(path) = trimmed.strip_prefix("ssh://git@github.com/") {
            return format!("https://github.com/{}", path);
        }

        trimmed.to_string()
    }

    fn resolve_git_head_commit(git_dir: &Path, head_content: &str) -> Option<String> {
        let trimmed = head_content.trim();
        if trimmed.is_empty() {
            return None;
        }

        if let Some(reference) = trimmed.strip_prefix("ref: ") {
            let ref_name = reference.trim();
            if ref_name.is_empty() {
                return None;
            }

            let ref_path = git_dir.join(ref_name);
            if let Ok(commit) = fs::read_to_string(ref_path) {
                let commit = commit.trim();
                if !commit.is_empty() {
                    return Some(commit.to_string());
                }
            }

            let packed_refs_path = git_dir.join("packed-refs");
            if let Ok(packed_refs) = fs::read_to_string(packed_refs_path) {
                for line in packed_refs.lines() {
                    let line = line.trim();
                    if line.is_empty() || line.starts_with('#') || line.starts_with('^') {
                        continue;
                    }

                    let mut parts = line.split_whitespace();
                    let Some(commit) = parts.next() else {
                        continue;
                    };
                    let Some(name) = parts.next() else {
                        continue;
                    };

                    if name == ref_name {
                        return Some(commit.to_string());
                    }
                }
            }

            return None;
        }

        Some(trimmed.to_string())
    }

    fn infer_source_metadata_from_git(
        &self,
        extension_path: &Path,
    ) -> Option<ExtensionSourceMetadata> {
        let git_dir = extension_path.join(".git");
        if !git_dir.is_dir() {
            return None;
        }

        let git_config = fs::read_to_string(git_dir.join("config")).ok()?;
        let remote_url = Self::parse_origin_remote_url(&git_config)?;
        let normalized_remote_url = Self::normalize_git_remote_url(&remote_url);
        let repo = self.parse_github_repo_url(&normalized_remote_url).ok()?;

        let head_content = fs::read_to_string(git_dir.join("HEAD")).ok()?;
        let installed_commit = Self::resolve_git_head_commit(&git_dir, &head_content)?;
        let reference = if let Some(head_ref) = head_content.trim().strip_prefix("ref: ") {
            head_ref
                .trim()
                .strip_prefix("refs/heads/")
                .unwrap_or(head_ref.trim())
                .to_string()
        } else {
            installed_commit.clone()
        };

        if reference.trim().is_empty() {
            return None;
        }

        Some(ExtensionSourceMetadata {
            owner: repo.owner.clone(),
            repo: repo.repo.clone(),
            reference,
            remote_url: format!("https://github.com/{}/{}", repo.owner, repo.repo),
            installed_commit,
        })
    }

    async fn resolve_source_metadata(
        &self,
        extension_path: &Path,
    ) -> Result<Option<ExtensionSourceMetadata>, DomainError> {
        if let Some(metadata) = self.read_source_metadata(extension_path).await? {
            return Ok(Some(metadata));
        }

        Ok(self.infer_source_metadata_from_git(extension_path))
    }

    async fn required_manifest(
        &self,
        extension_path: &Path,
    ) -> Result<ExtensionManifest, DomainError> {
        match self.get_manifest(extension_path).await? {
            Some(manifest) => Ok(manifest),
            None => Err(DomainError::InvalidData(
                "Extension manifest not found".to_string(),
            )),
        }
    }

    fn short_commit_hash(commit_hash: &str) -> String {
        commit_hash.chars().take(7).collect()
    }

    fn replace_directory(&self, source: &Path, destination: &Path) -> Result<(), DomainError> {
        let destination_name = destination
            .file_name()
            .map(|value| value.to_string_lossy().to_string())
            .unwrap_or_else(|| "extension".to_string());
        let backup_path =
            destination.with_file_name(format!(".backup-{}-{}", destination_name, Uuid::new_v4()));

        fs::rename(destination, &backup_path).map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to move existing extension '{}' to temporary backup '{}': {}",
                destination.display(),
                backup_path.display(),
                error
            ))
        })?;

        if let Err(error) = fs::rename(source, destination) {
            let _ = fs::rename(&backup_path, destination);
            return Err(DomainError::InternalError(format!(
                "Failed to activate updated extension '{}': {}",
                destination.display(),
                error
            )));
        }

        if let Err(error) = fs::remove_dir_all(&backup_path) {
            logger::warn(&format!(
                "Failed to remove extension backup directory '{}': {}",
                backup_path.display(),
                error
            ));
        }

        Ok(())
    }

    fn resolve_move_dir<'a>(&'a self, location: &str) -> Result<&'a Path, DomainError> {
        match location {
            "global" => Ok(&self.global_extensions_dir),
            "local" => Ok(&self.user_extensions_dir),
            _ => Err(DomainError::InvalidData(format!(
                "Invalid extension location: {}",
                location
            ))),
        }
    }
}

#[async_trait]
impl ExtensionRepository for FileExtensionRepository {
    async fn discover_extensions(&self) -> Result<Vec<Extension>, DomainError> {
        tracing::info!("Discovering extensions");

        let mut extensions = Vec::new();

        // Built-in extensions are explicitly allowlisted; unsupported modules remain disabled.
        for &name in ENABLED_SYSTEM_EXTENSIONS {
            let path = self.system_extensions_dir.join(name);
            let manifest = self.get_manifest(&path).await.ok().flatten();

            extensions.push(Extension {
                name: name.to_string(),
                extension_type: ExtensionType::System,
                manifest,
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

            if !asset_path.starts_with(&extension_root) || !asset_path.is_file() {
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

// Helper function to recursively copy a directory.
fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let path = entry.path();
        let file_name = path.file_name().unwrap();
        let target = dst.join(file_name);

        if ty.is_dir() {
            copy_dir_all(&path, &target)?;
        } else {
            fs::copy(&path, &target)?;
        }
    }
    Ok(())
}
