use tokio::fs as tokio_fs;

use crate::domain::errors::DomainError;

use super::FileExtensionRepository;
use super::source_store::ExtensionStoreScope;

pub(super) async fn delete_extension(
    repository: &FileExtensionRepository,
    extension_name: &str,
    global: bool,
) -> Result<(), DomainError> {
    tracing::info!("Deleting extension: {}", extension_name);

    let scope = ExtensionStoreScope::from_global(global);
    let extension_folder_name = repository.normalize_extension_name(extension_name)?;
    let extension_path = repository.resolve_extension_path(extension_name, global)?;
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
    repository
        .source_store
        .delete(scope, &extension_folder_name)
        .await?;

    tracing::info!("Extension deleted: {}", extension_name);
    Ok(())
}
