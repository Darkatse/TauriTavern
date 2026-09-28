use std::io;
use std::path::Path;

use tokio::fs;
use tt_domain::errors::DomainError;
use tt_domain::models::extension::{Extension, ExtensionType};

use super::FileExtensionRepository;

pub(super) async fn discover_extensions(
    repository: &FileExtensionRepository,
) -> Result<Vec<Extension>, DomainError> {
    let mut extensions = super::SYSTEM_EXTENSIONS
        .iter()
        .map(|name| Extension {
            name: (*name).to_string(),
            extension_type: ExtensionType::System,
        })
        .collect::<Vec<_>>();

    for (directory, extension_type) in [
        (&repository.user_extensions_dir, ExtensionType::Local),
        (&repository.global_extensions_dir, ExtensionType::Global),
    ] {
        discover_directory(directory, extension_type, &mut extensions)
            .await
            .map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to discover extensions in '{}': {error}",
                    directory.display()
                ))
            })?;
    }

    Ok(extensions)
}

async fn discover_directory(
    directory: &Path,
    extension_type: ExtensionType,
    extensions: &mut Vec<Extension>,
) -> io::Result<()> {
    let mut entries = match fs::read_dir(directory).await {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };

    while let Some(entry) = entries.next_entry().await? {
        let folder = entry.file_name().to_string_lossy().into_owned();
        if folder.starts_with('.') {
            continue;
        }
        let name = format!("third-party/{folder}");
        if extensions.iter().any(|extension| extension.name == name) {
            continue;
        }

        let path = entry.path();
        if !fs::metadata(&path).await?.is_dir() {
            continue;
        }
        if !fs::try_exists(path.join("manifest.json")).await? {
            tracing::warn!(
                "Skipping extension without manifest.json: {}",
                path.display()
            );
            continue;
        }

        extensions.push(Extension {
            name,
            extension_type: extension_type.clone(),
        });
    }

    Ok(())
}
