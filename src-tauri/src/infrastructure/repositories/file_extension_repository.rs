use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};
use tokio::fs as tokio_fs;
use url::Url;

use crate::domain::errors::DomainError;
use crate::domain::models::extension::{
    Extension, ExtensionInstallResult, ExtensionManifest, ExtensionType, ExtensionUpdateResult,
    ExtensionVersion,
};
use crate::domain::repositories::extension_repository::ExtensionRepository;
use crate::infrastructure::logging::logger;
use crate::infrastructure::persistence::file_system::read_json_file;

pub struct FileExtensionRepository {
    app_handle: AppHandle,
    user_extensions_dir: PathBuf,
    global_extensions_dir: PathBuf,
    system_extensions_dir: PathBuf,
}

#[derive(Debug, Serialize, Deserialize)]
struct GithubReleaseAsset {
    name: String,
    browser_download_url: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct GithubRelease {
    tag_name: String,
    assets: Vec<GithubReleaseAsset>,
}

#[derive(Debug, Serialize, Deserialize)]
struct GithubCommit {
    sha: String,
    commit: GithubCommitDetail,
}

#[derive(Debug, Serialize, Deserialize)]
struct GithubCommitDetail {
    message: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct GithubBranch {
    name: String,
    commit: GithubBranchCommit,
}

#[derive(Debug, Serialize, Deserialize)]
struct GithubBranchCommit {
    sha: String,
}

impl FileExtensionRepository {
    pub fn new(app_handle: AppHandle) -> Self {
        // Get app data directory
        let app_data_dir = app_handle
            .path()
            .app_data_dir()
            .expect("Failed to get app data directory");

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

        Self {
            app_handle,
            user_extensions_dir,
            global_extensions_dir,
            system_extensions_dir,
        }
    }

    /// Extract repository owner and name from a GitHub URL
    fn extract_repo_info(&self, url: &str) -> Result<(String, String), DomainError> {
        let parsed_url = Url::parse(url).map_err(|e| {
            logger::error(&format!("Failed to parse URL: {}", e));
            DomainError::InvalidData(format!("Invalid URL: {}", e))
        })?;

        if parsed_url.host_str() != Some("github.com") {
            return Err(DomainError::InvalidData(
                "Only GitHub repositories are supported".to_string(),
            ));
        }

        let path_segments: Vec<&str> = parsed_url
            .path_segments()
            .ok_or_else(|| DomainError::InvalidData("Invalid GitHub URL".to_string()))?
            .collect();

        if path_segments.len() < 2 {
            return Err(DomainError::InvalidData(
                "Invalid GitHub repository URL".to_string(),
            ));
        }

        let owner = path_segments[0].to_string();
        let repo = path_segments[1].to_string().replace(".git", "");

        Ok((owner, repo))
    }

    /// Download a file from a URL to a local path
    async fn download_file(&self, url: &str, path: &Path) -> Result<(), DomainError> {
        logger::debug(&format!("Downloading file from {} to {:?}", url, path));

        let client = Client::new();
        let response = client.get(url).send().await.map_err(|e| {
            logger::error(&format!("Failed to download file: {}", e));
            DomainError::InternalError(format!("Failed to download file: {}", e))
        })?;

        if !response.status().is_success() {
            return Err(DomainError::InternalError(format!(
                "Failed to download file: HTTP {}",
                response.status()
            )));
        }

        let content = response.bytes().await.map_err(|e| {
            logger::error(&format!("Failed to read response body: {}", e));
            DomainError::InternalError(format!("Failed to read response body: {}", e))
        })?;

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            tokio_fs::create_dir_all(parent).await.map_err(|e| {
                logger::error(&format!("Failed to create parent directory: {}", e));
                DomainError::InternalError(format!("Failed to create parent directory: {}", e))
            })?;
        }

        // Write file
        tokio_fs::write(path, content).await.map_err(|e| {
            logger::error(&format!("Failed to write file: {}", e));
            DomainError::InternalError(format!("Failed to write file: {}", e))
        })?;

        Ok(())
    }

    /// Download a GitHub repository as a zip file and extract it
    async fn download_github_repo(
        &self,
        owner: &str,
        repo: &str,
        branch: &str,
        destination: &Path,
    ) -> Result<(), DomainError> {
        tracing::info!(
            "Downloading GitHub repository {}/{} (branch: {}) to {:?}",
            owner,
            repo,
            branch,
            destination
        );

        // Create destination directory if it doesn't exist
        tokio_fs::create_dir_all(destination).await.map_err(|e| {
            logger::error(&format!("Failed to create destination directory: {}", e));
            DomainError::InternalError(format!("Failed to create destination directory: {}", e))
        })?;

        // Download zip file
        let zip_url = format!(
            "https://github.com/{}/{}/archive/{}.zip",
            owner, repo, branch
        );
        let temp_dir = self.app_handle.path().app_cache_dir().map_err(|e| {
            logger::error(&format!("Failed to get app cache directory: {}", e));
            DomainError::InternalError(format!("Failed to get app cache directory: {}", e))
        })?;
        let zip_path = temp_dir.join(format!("{}-{}.zip", repo, branch));

        self.download_file(&zip_url, &zip_path).await?;

        // Extract zip file
        let zip_file = fs::File::open(&zip_path).map_err(|e| {
            logger::error(&format!("Failed to open zip file: {}", e));
            DomainError::InternalError(format!("Failed to open zip file: {}", e))
        })?;

        let mut archive = zip::ZipArchive::new(zip_file).map_err(|e| {
            logger::error(&format!("Failed to read zip archive: {}", e));
            DomainError::InternalError(format!("Failed to read zip archive: {}", e))
        })?;

        // The top-level directory in the zip file is usually "{repo}-{branch}"
        let prefix = format!("{}-{}", repo, branch);

        for i in 0..archive.len() {
            let mut file = archive.by_index(i).map_err(|e| {
                logger::error(&format!("Failed to read zip entry: {}", e));
                DomainError::InternalError(format!("Failed to read zip entry: {}", e))
            })?;

            let file_path = file.name();

            // Skip the top-level directory
            if !file_path.starts_with(&prefix) {
                continue;
            }

            // Remove the top-level directory from the path
            let relative_path = file_path
                .strip_prefix(&format!("{}/", prefix))
                .unwrap_or(file_path);
            let output_path = destination.join(relative_path);

            if file.is_dir() {
                fs::create_dir_all(&output_path).map_err(|e| {
                    logger::error(&format!("Failed to create directory: {}", e));
                    DomainError::InternalError(format!("Failed to create directory: {}", e))
                })?;
            } else {
                if let Some(parent) = output_path.parent() {
                    fs::create_dir_all(parent).map_err(|e| {
                        logger::error(&format!("Failed to create parent directory: {}", e));
                        DomainError::InternalError(format!(
                            "Failed to create parent directory: {}",
                            e
                        ))
                    })?;
                }

                let mut output_file = fs::File::create(&output_path).map_err(|e| {
                    logger::error(&format!("Failed to create file: {}", e));
                    DomainError::InternalError(format!("Failed to create file: {}", e))
                })?;

                std::io::copy(&mut file, &mut output_file).map_err(|e| {
                    logger::error(&format!("Failed to write file: {}", e));
                    DomainError::InternalError(format!("Failed to write file: {}", e))
                })?;
            }
        }

        // Clean up
        fs::remove_file(zip_path).map_err(|e| {
            logger::error(&format!("Failed to remove temporary zip file: {}", e));
            DomainError::InternalError(format!("Failed to remove temporary zip file: {}", e))
        })?;

        Ok(())
    }

    /// Get the latest commit hash for a GitHub repository
    async fn get_latest_commit_hash(
        &self,
        owner: &str,
        repo: &str,
        branch: &str,
    ) -> Result<String, DomainError> {
        logger::debug(&format!(
            "Getting latest commit hash for {}/{} (branch: {})",
            owner, repo, branch
        ));

        let client = Client::new();
        let url = format!(
            "https://api.github.com/repos/{}/{}/commits/{}",
            owner, repo, branch
        );

        let response = client
            .get(&url)
            .header("User-Agent", "TauriTavern")
            .send()
            .await
            .map_err(|e| {
                logger::error(&format!("Failed to get latest commit hash: {}", e));
                DomainError::InternalError(format!("Failed to get latest commit hash: {}", e))
            })?;

        if !response.status().is_success() {
            return Err(DomainError::InternalError(format!(
                "Failed to get latest commit hash: HTTP {}",
                response.status()
            )));
        }

        let commit: GithubCommit = response.json().await.map_err(|e| {
            logger::error(&format!("Failed to parse GitHub API response: {}", e));
            DomainError::InternalError(format!("Failed to parse GitHub API response: {}", e))
        })?;

        Ok(commit.sha)
    }

    /// Get the default branch for a GitHub repository
    async fn get_default_branch(&self, owner: &str, repo: &str) -> Result<String, DomainError> {
        logger::debug(&format!("Getting default branch for {}/{}", owner, repo));

        let client = Client::new();
        let url = format!("https://api.github.com/repos/{}/{}/branches", owner, repo);

        let response = client
            .get(&url)
            .header("User-Agent", "TauriTavern")
            .send()
            .await
            .map_err(|e| {
                logger::error(&format!("Failed to get repository branches: {}", e));
                DomainError::InternalError(format!("Failed to get repository branches: {}", e))
            })?;

        if !response.status().is_success() {
            return Err(DomainError::InternalError(format!(
                "Failed to get repository branches: HTTP {}",
                response.status()
            )));
        }

        let branches: Vec<GithubBranch> = response.json().await.map_err(|e| {
            logger::error(&format!("Failed to parse GitHub API response: {}", e));
            DomainError::InternalError(format!("Failed to parse GitHub API response: {}", e))
        })?;

        // Try to find main or master branch
        for branch_name in ["main", "master"] {
            if let Some(branch) = branches.iter().find(|b| b.name == branch_name) {
                return Ok(branch.name.clone());
            }
        }

        // If no main or master branch, use the first branch
        if let Some(branch) = branches.first() {
            return Ok(branch.name.clone());
        }

        Err(DomainError::InternalError(
            "No branches found in repository".to_string(),
        ))
    }

    /// Check if a repository is up to date
    async fn check_if_repo_is_up_to_date(
        &self,
        extension_path: &Path,
        owner: &str,
        repo: &str,
        branch: &str,
        current_commit_hash: &str,
    ) -> Result<bool, DomainError> {
        logger::debug(&format!(
            "Checking if repository is up to date: {:?}",
            extension_path
        ));

        let latest_commit_hash = self.get_latest_commit_hash(owner, repo, branch).await?;

        Ok(current_commit_hash == latest_commit_hash)
    }

    /// Get the remote URL for a GitHub repository
    fn get_remote_url(&self, owner: &str, repo: &str) -> String {
        format!("https://github.com/{}/{}", owner, repo)
    }

    /// Get the extension type based on the path
    fn get_extension_type(&self, path: &Path) -> ExtensionType {
        if path.starts_with(&self.system_extensions_dir)
            && !path.starts_with(&self.global_extensions_dir)
        {
            ExtensionType::System
        } else if path.starts_with(&self.global_extensions_dir) {
            ExtensionType::Global
        } else {
            ExtensionType::Local
        }
    }

    /// Get the base directory for an extension type
    fn get_base_directory(&self, extension_type: &ExtensionType) -> &Path {
        match extension_type {
            ExtensionType::System => &self.system_extensions_dir,
            ExtensionType::Global => &self.global_extensions_dir,
            ExtensionType::Local => &self.user_extensions_dir,
        }
    }

    /// Get the extension path based on name and type
    fn get_extension_path(&self, extension_name: &str, global: bool) -> PathBuf {
        if global {
            self.global_extensions_dir.join(extension_name)
        } else {
            self.user_extensions_dir.join(extension_name)
        }
    }

    /// Sanitize a filename
    fn sanitize_filename(&self, filename: &str) -> String {
        filename
            .chars()
            .map(|c| match c {
                '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
                _ => c,
            })
            .collect()
    }
}

#[async_trait]
impl ExtensionRepository for FileExtensionRepository {
    async fn discover_extensions(&self) -> Result<Vec<Extension>, DomainError> {
        tracing::info!("Discovering extensions");

        let mut extensions = Vec::new();

        // Get built-in extensions (excluding third-party)
        if self.system_extensions_dir.exists() {
            let entries = fs::read_dir(&self.system_extensions_dir).map_err(|e| {
                logger::error(&format!(
                    "Failed to read system extensions directory: {}",
                    e
                ));
                DomainError::InternalError(format!(
                    "Failed to read system extensions directory: {}",
                    e
                ))
            })?;

            for entry in entries {
                let entry = entry.map_err(|e| {
                    logger::error(&format!("Failed to read directory entry: {}", e));
                    DomainError::InternalError(format!("Failed to read directory entry: {}", e))
                })?;

                let path = entry.path();
                if path.is_dir() && path != self.global_extensions_dir {
                    let name = path.file_name().unwrap().to_string_lossy().to_string();
                    let manifest = self.get_manifest(&path).await.ok().flatten();

                    extensions.push(Extension {
                        name,
                        extension_type: ExtensionType::System,
                        manifest,
                        path,
                        remote_url: None,
                        commit_hash: None,
                        branch_name: None,
                        is_up_to_date: None,
                    });
                }
            }
        }

        // Get user extensions
        if self.user_extensions_dir.exists() {
            let entries = fs::read_dir(&self.user_extensions_dir).map_err(|e| {
                logger::error(&format!("Failed to read user extensions directory: {}", e));
                DomainError::InternalError(format!(
                    "Failed to read user extensions directory: {}",
                    e
                ))
            })?;

            for entry in entries {
                let entry = entry.map_err(|e| {
                    logger::error(&format!("Failed to read directory entry: {}", e));
                    DomainError::InternalError(format!("Failed to read directory entry: {}", e))
                })?;

                let path = entry.path();
                if path.is_dir() {
                    let name = path.file_name().unwrap().to_string_lossy().to_string();
                    let manifest = self.get_manifest(&path).await.ok().flatten();

                    extensions.push(Extension {
                        name: format!("third-party/{}", name),
                        extension_type: ExtensionType::Local,
                        manifest,
                        path,
                        remote_url: None,
                        commit_hash: None,
                        branch_name: None,
                        is_up_to_date: None,
                    });
                }
            }
        }

        // Get global extensions
        if self.global_extensions_dir.exists() {
            let entries = fs::read_dir(&self.global_extensions_dir).map_err(|e| {
                logger::error(&format!(
                    "Failed to read global extensions directory: {}",
                    e
                ));
                DomainError::InternalError(format!(
                    "Failed to read global extensions directory: {}",
                    e
                ))
            })?;

            for entry in entries {
                let entry = entry.map_err(|e| {
                    logger::error(&format!("Failed to read directory entry: {}", e));
                    DomainError::InternalError(format!("Failed to read directory entry: {}", e))
                })?;

                let path = entry.path();
                if path.is_dir() {
                    let name = path.file_name().unwrap().to_string_lossy().to_string();
                    let manifest = self.get_manifest(&path).await.ok().flatten();

                    // Skip if the extension is already in the user directory
                    if extensions
                        .iter()
                        .any(|e| e.name == format!("third-party/{}", name))
                    {
                        continue;
                    }

                    extensions.push(Extension {
                        name: format!("third-party/{}", name),
                        extension_type: ExtensionType::Global,
                        manifest,
                        path,
                        remote_url: None,
                        commit_hash: None,
                        branch_name: None,
                        is_up_to_date: None,
                    });
                }
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
    ) -> Result<ExtensionInstallResult, DomainError> {
        tracing::info!("Installing extension from {}", url);

        // Extract repository owner and name
        let (owner, repo) = self.extract_repo_info(url)?;

        // Get the base directory
        let base_dir = if global {
            &self.global_extensions_dir
        } else {
            &self.user_extensions_dir
        };

        // Sanitize repository name
        let sanitized_repo = self.sanitize_filename(&repo);
        let extension_path = base_dir.join(&sanitized_repo);

        // Check if the extension already exists
        if extension_path.exists() {
            return Err(DomainError::InvalidData(format!(
                "Extension already exists at {:?}",
                extension_path
            )));
        }

        // Get the default branch
        let branch = self.get_default_branch(&owner, &repo).await?;

        // Download the repository
        self.download_github_repo(&owner, &repo, &branch, &extension_path)
            .await?;

        // Get the manifest
        let manifest = match self.get_manifest(&extension_path).await? {
            Some(manifest) => manifest,
            None => {
                // Clean up if manifest is not found
                tokio_fs::remove_dir_all(&extension_path)
                    .await
                    .map_err(|e| {
                        logger::error(&format!("Failed to remove extension directory: {}", e));
                        DomainError::InternalError(format!(
                            "Failed to remove extension directory: {}",
                            e
                        ))
                    })?;

                return Err(DomainError::InvalidData(
                    "Extension manifest not found".to_string(),
                ));
            }
        };

        tracing::info!(
            "Extension installed: {} v{} by {}",
            manifest.display_name,
            manifest.version,
            manifest.author
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

        // Get the extension path
        let extension_path = self.get_extension_path(extension_name, global);

        if !extension_path.exists() {
            return Err(DomainError::NotFound(format!(
                "Extension not found at {:?}",
                extension_path
            )));
        }

        // Try to find a GitHub URL in the extension directory
        // For now, we'll assume the extension was installed from GitHub
        // and the repository name is the same as the extension name
        let (owner, repo) = match extension_name.split('/').collect::<Vec<_>>().as_slice() {
            [org, name] => (org.to_string(), name.to_string()),
            _ => {
                // Try to extract from manifest
                if let Some(manifest) = self.get_manifest(&extension_path).await? {
                    if let Some(author) = manifest.author.split(' ').next() {
                        (author.to_string(), extension_name.to_string())
                    } else {
                        return Err(DomainError::InvalidData(
                            "Could not determine repository owner and name".to_string(),
                        ));
                    }
                } else {
                    return Err(DomainError::InvalidData(
                        "Could not determine repository owner and name".to_string(),
                    ));
                }
            }
        };

        // Get the default branch
        let branch = self.get_default_branch(&owner, &repo).await?;

        // Get the current commit hash
        let current_commit_hash = self.get_latest_commit_hash(&owner, &repo, &branch).await?;
        let short_commit_hash = current_commit_hash[..7].to_string();

        // Check if the repository is up to date
        let is_up_to_date = true; // We're always up to date after an update

        // Get the remote URL
        let remote_url = self.get_remote_url(&owner, &repo);

        // Download the repository
        self.download_github_repo(&owner, &repo, &branch, &extension_path)
            .await?;

        tracing::info!(
            "Extension updated: {} to commit {}",
            extension_name,
            short_commit_hash
        );

        Ok(ExtensionUpdateResult {
            short_commit_hash,
            extension_path: extension_path.to_string_lossy().to_string(),
            is_up_to_date,
            remote_url,
        })
    }

    async fn delete_extension(
        &self,
        extension_name: &str,
        global: bool,
    ) -> Result<(), DomainError> {
        tracing::info!("Deleting extension: {}", extension_name);

        // Get the extension path
        let extension_path = self.get_extension_path(extension_name, global);

        if !extension_path.exists() {
            return Err(DomainError::NotFound(format!(
                "Extension not found at {:?}",
                extension_path
            )));
        }

        // Delete the extension directory
        tokio_fs::remove_dir_all(&extension_path)
            .await
            .map_err(|e| {
                tracing::error!("Failed to remove extension directory: {}", e);
                DomainError::InternalError(format!("Failed to remove extension directory: {}", e))
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

        // Get the extension path
        let extension_path = self.get_extension_path(extension_name, global);

        if !extension_path.exists() {
            return Err(DomainError::NotFound(format!(
                "Extension not found at {:?}",
                extension_path
            )));
        }

        // Try to find a GitHub URL in the extension directory
        // For now, we'll assume the extension was installed from GitHub
        // and the repository name is the same as the extension name
        let (owner, repo) = match extension_name.split('/').collect::<Vec<_>>().as_slice() {
            [org, name] => (org.to_string(), name.to_string()),
            _ => {
                // Try to extract from manifest
                if let Some(manifest) = self.get_manifest(&extension_path).await? {
                    if let Some(author) = manifest.author.split(' ').next() {
                        (author.to_string(), extension_name.to_string())
                    } else {
                        return Err(DomainError::InvalidData(
                            "Could not determine repository owner and name".to_string(),
                        ));
                    }
                } else {
                    return Err(DomainError::InvalidData(
                        "Could not determine repository owner and name".to_string(),
                    ));
                }
            }
        };

        // Get the default branch
        let branch = self.get_default_branch(&owner, &repo).await?;

        // Get the current commit hash
        let current_commit_hash = self.get_latest_commit_hash(&owner, &repo, &branch).await?;

        // Check if the repository is up to date
        let is_up_to_date = self
            .check_if_repo_is_up_to_date(
                &extension_path,
                &owner,
                &repo,
                &branch,
                &current_commit_hash,
            )
            .await?;

        // Get the remote URL
        let remote_url = self.get_remote_url(&owner, &repo);

        logger::debug(&format!(
            "Extension version: {} (branch: {}, commit: {}, up to date: {})",
            extension_name, branch, current_commit_hash, is_up_to_date
        ));

        Ok(ExtensionVersion {
            current_branch_name: branch,
            current_commit_hash,
            is_up_to_date,
            remote_url,
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

        // Get the source and destination directories
        let source_dir = match source {
            "global" => &self.global_extensions_dir,
            "local" => &self.user_extensions_dir,
            _ => {
                return Err(DomainError::InvalidData(format!(
                    "Invalid source: {}",
                    source
                )))
            }
        };

        let destination_dir = match destination {
            "global" => &self.global_extensions_dir,
            "local" => &self.user_extensions_dir,
            _ => {
                return Err(DomainError::InvalidData(format!(
                    "Invalid destination: {}",
                    destination
                )))
            }
        };

        // Get the source and destination paths
        let source_path = source_dir.join(extension_name);
        let destination_path = destination_dir.join(extension_name);

        if !source_path.exists() {
            return Err(DomainError::NotFound(format!(
                "Extension not found at {:?}",
                source_path
            )));
        }

        if destination_path.exists() {
            return Err(DomainError::InvalidData(format!(
                "Destination already exists at {:?}",
                destination_path
            )));
        }

        if source == destination {
            return Err(DomainError::InvalidData(
                "Source and destination are the same".to_string(),
            ));
        }

        // Copy the extension directory
        copy_dir_all(&source_path, &destination_path).map_err(|e| {
            logger::error(&format!("Failed to copy extension directory: {}", e));
            DomainError::InternalError(format!("Failed to copy extension directory: {}", e))
        })?;

        // Delete the source directory
        tokio_fs::remove_dir_all(&source_path).await.map_err(|e| {
            logger::error(&format!("Failed to remove source directory: {}", e));
            DomainError::InternalError(format!("Failed to remove source directory: {}", e))
        })?;

        tracing::info!(
            "Extension moved: {} from {} to {}",
            extension_name,
            source,
            destination
        );

        Ok(())
    }
}

// Helper function to recursively copy a directory
fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let path = entry.path();
        let filename = path.file_name().unwrap();
        let target = dst.join(filename);
        if ty.is_dir() {
            copy_dir_all(&path, &target)?;
        } else {
            fs::copy(&path, &target)?;
        }
    }
    Ok(())
}
