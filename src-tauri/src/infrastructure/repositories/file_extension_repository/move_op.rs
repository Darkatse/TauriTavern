use tokio::fs as tokio_fs;

use crate::domain::errors::DomainError;

use super::FileExtensionRepository;
use super::archive_zip::copy_dir_all;
use super::source_store::ExtensionStoreScope;

pub(super) async fn move_extension(
    repository: &FileExtensionRepository,
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

    let extension_folder_name = repository.normalize_extension_name(extension_name)?;
    let (source_dir, source_scope) = resolve_move_dir(repository, source)?;
    let (destination_dir, destination_scope) = resolve_move_dir(repository, destination)?;

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
    repository
        .source_store
        .move_record(source_scope, destination_scope, &extension_folder_name)
        .await?;

    tracing::info!(
        "Extension moved: {} from {} to {}",
        extension_folder_name,
        source,
        destination
    );
    Ok(())
}

fn resolve_move_dir<'a>(
    repository: &'a FileExtensionRepository,
    location: &str,
) -> Result<(&'a std::path::Path, ExtensionStoreScope), DomainError> {
    let scope = ExtensionStoreScope::from_location(location)?;
    Ok((repository.extension_dir_for_scope(scope), scope))
}
