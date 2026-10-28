use std::path::{Path, PathBuf};

use tokio::fs;
use tt_domain::errors::DomainError;
use uuid::Uuid;

use super::assets::{BundledResources, copy_resource_to_file, list_default_files_under};

pub async fn materialize_builtin_skills(
    resources: &dyn BundledResources,
    data_root: &Path,
) -> Result<PathBuf, DomainError> {
    let cache_root = data_root.join("_cache/builtin-skills");
    let digest = env!("TAURITAVERN_BUILTIN_SKILLS_DIGEST");
    let target = cache_root.join(digest);
    // The digest names immutable content; publishing the directory is the final write.
    if fs::try_exists(&target).await.map_err(|error| {
        DomainError::file_io("stat builtin Skills", target.display().to_string(), error)
    })? {
        return Ok(target);
    }

    let staging = cache_root.join(format!(".{digest}.tmp-{}", Uuid::new_v4().simple()));
    fs::create_dir_all(&staging).await.map_err(|error| {
        DomainError::file_io(
            "create builtin Skills staging",
            staging.display().to_string(),
            error,
        )
    })?;
    for relative in list_default_files_under("skills", "") {
        copy_resource_to_file(
            resources,
            &format!("default/skills/{relative}"),
            &staging.join(relative),
        )
        .await?;
    }
    fs::rename(&staging, &target).await.map_err(|error| {
        DomainError::file_io(
            "publish builtin Skills",
            target.display().to_string(),
            error,
        )
    })?;

    // Older copies are never read again; leftovers must not block startup.
    if let Err(error) = remove_other_entries(&cache_root, &target).await {
        tracing::warn!(
            "Failed to remove old builtin Skills in {}: {error}",
            cache_root.display()
        );
    }
    Ok(target)
}

async fn remove_other_entries(cache_root: &Path, keep: &Path) -> std::io::Result<()> {
    let mut entries = fs::read_dir(cache_root).await?;
    while let Some(entry) = entries.next_entry().await? {
        if entry.path() == keep {
            continue;
        }
        if entry.file_type().await?.is_dir() {
            fs::remove_dir_all(entry.path()).await?;
        } else {
            fs::remove_file(entry.path()).await?;
        }
    }
    Ok(())
}
