use std::fs;
use std::path::PathBuf;

use crate::domain::errors::DomainError;
use crate::domain::models::extension::{Extension, ExtensionType};
use crate::infrastructure::logging::logger;

use super::FileExtensionRepository;
use super::source_store::ExtensionStoreScope;

pub(super) async fn discover_extensions(
    repository: &FileExtensionRepository,
) -> Result<Vec<Extension>, DomainError> {
    tracing::info!("Discovering extensions");

    let mut extensions = Vec::new();

    for &name in super::ENABLED_SYSTEM_EXTENSIONS {
        extensions.push(Extension {
            name: name.to_string(),
            extension_type: ExtensionType::System,
            managed: true,
            manifest: None,
            path: PathBuf::from(format!("scripts/extensions/{}", name)),
            remote_url: None,
            commit_hash: None,
            branch_name: None,
            is_up_to_date: None,
        });
    }

    discover_scoped_extensions(repository, ExtensionStoreScope::Local, &mut extensions).await?;
    discover_scoped_extensions(repository, ExtensionStoreScope::Global, &mut extensions).await?;

    logger::debug(&format!("Discovered {} extensions", extensions.len()));
    Ok(extensions)
}

async fn discover_scoped_extensions(
    repository: &FileExtensionRepository,
    scope: ExtensionStoreScope,
    extensions: &mut Vec<Extension>,
) -> Result<(), DomainError> {
    let extensions_dir = repository.extension_dir_for_scope(scope);
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
        let extension_folder_name = file_name.to_string_lossy().to_string();
        if extension_folder_name.starts_with('.') {
            continue;
        }

        let extension_name = format!("third-party/{}", extension_folder_name);
        if scope == ExtensionStoreScope::Global
            && extensions
                .iter()
                .any(|extension| extension.name == extension_name)
        {
            continue;
        }

        let source = repository
            .resolve_source_metadata(scope, &extension_folder_name, &path)
            .await?;

        let (managed, remote_url, commit_hash, branch_name) = match source {
            Some(source) => (
                true,
                Some(source.remote_url),
                Some(source.installed_commit),
                Some(source.reference),
            ),
            None => {
                tracing::debug!(
                    "Found unmanaged extension '{}' at '{}': missing source metadata and could not infer from git state",
                    extension_folder_name,
                    path.display()
                );
                (false, None, None, None)
            }
        };

        let manifest = repository.read_manifest(&path).await?;

        extensions.push(Extension {
            name: extension_name,
            extension_type: match scope {
                ExtensionStoreScope::Local => ExtensionType::Local,
                ExtensionStoreScope::Global => ExtensionType::Global,
            },
            managed,
            manifest,
            path,
            remote_url,
            commit_hash,
            branch_name,
            is_up_to_date: None,
        });
    }

    Ok(())
}
