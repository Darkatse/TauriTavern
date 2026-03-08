use crate::domain::errors::DomainError;
use crate::domain::models::extension::ExtensionUpdateResult;

use super::FileExtensionRepository;
use super::source_store::ExtensionStoreScope;

pub(super) async fn update_extension(
    repository: &FileExtensionRepository,
    extension_name: &str,
    global: bool,
) -> Result<ExtensionUpdateResult, DomainError> {
    tracing::info!("Updating extension: {}", extension_name);

    let scope = ExtensionStoreScope::from_global(global);
    let extension_folder_name = repository.extension_folder_name_from_identifier(extension_name)?;
    let extension_path = repository.resolve_extension_path(&extension_folder_name, global);
    if !extension_path.exists() {
        return Err(DomainError::NotFound(format!(
            "Extension not found at '{}'",
            extension_path.display()
        )));
    }

    let mut source = repository
        .resolve_source_metadata(scope, &extension_folder_name, &extension_path)
        .await?
        .ok_or_else(|| {
            DomainError::InvalidData(
                "Extension source metadata is missing. Reinstall this extension to enable updates."
                    .to_string(),
            )
        })?;

    let provider = repository.providers.for_host(source.host.as_str())?;
    let latest_commit = provider
        .latest_commit(source.repo_path.as_str(), source.reference.as_str())
        .await?;
    let is_up_to_date = source.installed_commit == latest_commit;

    if !is_up_to_date {
        let base_dir = extension_path.parent().ok_or_else(|| {
            DomainError::InternalError(format!(
                "Failed to resolve parent directory for '{}'",
                extension_path.display()
            ))
        })?;

        let (staging_dir, _) = repository
            .stage_extension_snapshot(
                provider,
                source.repo_path.as_str(),
                latest_commit.as_str(),
                base_dir,
                "extension-update",
            )
            .await?;

        if let Err(error) = repository.replace_directory(&staging_dir, &extension_path) {
            FileExtensionRepository::cleanup_temp_directory(&staging_dir).await;
            return Err(error);
        }

        source.installed_commit = latest_commit.clone();
        repository
            .source_store
            .write(scope, &extension_folder_name, &source)
            .await?;
    }

    let short_commit_hash = FileExtensionRepository::short_commit_hash(&latest_commit);

    Ok(ExtensionUpdateResult {
        short_commit_hash,
        extension_path: extension_path.to_string_lossy().to_string(),
        is_up_to_date,
        remote_url: source.remote_url,
    })
}
