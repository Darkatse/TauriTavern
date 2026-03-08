use std::fs;

use crate::domain::errors::DomainError;
use crate::domain::models::extension::ExtensionInstallResult;
use crate::infrastructure::logging::logger;

use super::FileExtensionRepository;
use super::repo_url::{normalize_requested_reference, parse_repo_url};
use super::source_store::{ExtensionSourceMetadata, ExtensionStoreScope};

pub(super) async fn install_extension(
    repository: &FileExtensionRepository,
    url: &str,
    global: bool,
    branch: Option<String>,
) -> Result<ExtensionInstallResult, DomainError> {
    tracing::info!("Installing extension from {}", url);

    let repo = parse_repo_url(url)?;
    let provider = repository.providers.for_host(repo.host.as_str())?;
    let reference = normalize_requested_reference(branch)
        .or(repo.reference_from_url.clone())
        .unwrap_or(provider.default_branch(repo.repo_path.as_str()).await?);
    let latest_commit = provider
        .latest_commit(repo.repo_path.as_str(), reference.as_str())
        .await?;

    let base_dir = repository.extension_base_dir(global);
    let extension_folder_name =
        FileExtensionRepository::install_folder_name_from_repo_name(repo.repo_name())?;
    let extension_path = base_dir.join(&extension_folder_name);

    if extension_path.exists() {
        return Err(DomainError::InvalidData(format!(
            "Extension already exists at '{}'",
            extension_path.display()
        )));
    }

    let (staging_dir, manifest) = repository
        .stage_extension_snapshot(
            provider,
            repo.repo_path.as_str(),
            latest_commit.as_str(),
            base_dir,
            "extension-install",
        )
        .await?;

    let scope = ExtensionStoreScope::from_global(global);
    let source_metadata = ExtensionSourceMetadata {
        host: repo.host.clone(),
        repo_path: repo.repo_path.clone(),
        reference: reference.clone(),
        remote_url: repo.canonical_remote_url(),
        installed_commit: latest_commit.clone(),
    };
    if let Err(error) = repository
        .source_store
        .write(scope, &extension_folder_name, &source_metadata)
        .await
    {
        FileExtensionRepository::cleanup_temp_directory(&staging_dir).await;
        return Err(error);
    }

    if let Err(error) = fs::rename(&staging_dir, &extension_path) {
        if let Err(cleanup_error) = repository
            .source_store
            .delete(scope, &extension_folder_name)
            .await
        {
            logger::warn(&format!(
                "Failed to rollback extension source metadata for '{}': {}",
                extension_folder_name, cleanup_error
            ));
        }
        FileExtensionRepository::cleanup_temp_directory(&staging_dir).await;
        return Err(DomainError::InternalError(format!(
            "Failed to finalize extension installation into '{}': {}",
            extension_path.display(),
            error
        )));
    }

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
