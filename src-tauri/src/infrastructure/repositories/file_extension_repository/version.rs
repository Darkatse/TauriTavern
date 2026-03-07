use crate::domain::errors::DomainError;
use crate::domain::models::extension::ExtensionVersion;

use super::FileExtensionRepository;
use super::source_store::ExtensionStoreScope;

pub(super) async fn get_extension_version(
    repository: &FileExtensionRepository,
    extension_name: &str,
    global: bool,
) -> Result<ExtensionVersion, DomainError> {
    tracing::info!("Getting extension version: {}", extension_name);

    let scope = ExtensionStoreScope::from_global(global);
    let extension_folder_name = repository.normalize_extension_name(extension_name)?;
    let extension_path = repository.resolve_extension_path(extension_name, global)?;
    if !extension_path.exists() {
        return Err(DomainError::NotFound(format!(
            "Extension not found at '{}'",
            extension_path.display()
        )));
    }

    let source = match repository
        .resolve_source_metadata(scope, &extension_folder_name, &extension_path)
        .await?
    {
        Some(source) => source,
        None => {
            return Ok(ExtensionVersion {
                current_branch_name: String::new(),
                current_commit_hash: String::new(),
                is_up_to_date: true,
                remote_url: String::new(),
            });
        }
    };

    let provider = repository.providers.for_host(source.host.as_str())?;
    let latest_commit = provider
        .latest_commit(source.repo_path.as_str(), source.reference.as_str())
        .await?;
    let is_up_to_date = source.installed_commit == latest_commit;

    Ok(ExtensionVersion {
        current_branch_name: source.reference,
        current_commit_hash: source.installed_commit,
        is_up_to_date,
        remote_url: source.remote_url,
    })
}
