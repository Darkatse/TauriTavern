use std::path::{Path, PathBuf};

use tauri::AppHandle;
use tokio::fs;
use tt_domain::errors::DomainError;
use uuid::Uuid;

use super::assets::{copy_resource_to_file, list_default_files_under};

pub async fn materialize_builtin_skills(
    app_handle: &AppHandle,
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
            app_handle,
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

    let mut entries = fs::read_dir(&cache_root).await.map_err(|error| {
        DomainError::file_io(
            "list builtin Skills cache",
            cache_root.display().to_string(),
            error,
        )
    })?;
    while let Some(entry) = entries.next_entry().await.map_err(|error| {
        DomainError::file_io(
            "list builtin Skills cache",
            cache_root.display().to_string(),
            error,
        )
    })? {
        if entry.path() == target {
            continue;
        }
        fs::remove_dir_all(entry.path()).await.map_err(|error| {
            DomainError::file_io(
                "remove old builtin Skills",
                entry.path().display().to_string(),
                error,
            )
        })?;
    }
    Ok(target)
}
