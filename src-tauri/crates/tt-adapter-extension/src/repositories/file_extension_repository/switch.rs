use std::path::Path;

use tt_domain::errors::DomainError;

use super::FileExtensionRepository;
use super::directory_ops::{cleanup_temp_directory, create_temp_directory, replace_directory};
use super::git_http::GitHttp;
use super::git_remote::{branch_ref, fetch_exact, open_embedded, parse_remote_url};
use super::git_worktree::{
    ManagedRef, configure_branch_switch, finalize_branch_switch, has_standard_embedded_git,
    materialize_candidate, prepare_candidate, read_managed_state,
};
use super::install::stage_embedded_branch;
use super::source_store::ExtensionStoreScope;

pub(super) async fn switch_extension_branch(
    repository: &FileExtensionRepository,
    extension_name: &str,
    branch: &str,
    global: bool,
) -> Result<(), DomainError> {
    let scope = ExtensionStoreScope::from_global(global);
    let extension_folder_name = repository.extension_folder_name_from_identifier(extension_name)?;
    let extension_path = repository.resolve_extension_path(&extension_folder_name, global);
    if !extension_path.exists() {
        return Err(DomainError::NotFound(format!(
            "Extension not found at '{}'",
            extension_path.display()
        )));
    }
    let target = normalize_branch(branch)?;

    if has_standard_embedded_git(&extension_path)? {
        let http_clients = repository.http_clients.clone();
        return tokio::task::spawn_blocking(move || {
            switch_embedded(&extension_path, &target, http_clients.as_ref())
        })
        .await
        .map_err(|error| {
            DomainError::InternalError(format!("Extension branch switch task failed: {error}"))
        })?;
    }

    let source = repository
        .source_store
        .read(scope, &extension_folder_name, &extension_path)
        .await?
        .ok_or_else(|| {
            DomainError::InvalidData(
                "Extension source metadata is missing. Reinstall this extension to enable branch switching."
                    .to_string(),
            )
        })?;
    if source.reference.trim() == target {
        return Ok(());
    }

    let remote = parse_remote_url(&source.remote_url)?;
    let base_dir = extension_path.parent().ok_or_else(|| {
        DomainError::InternalError(format!(
            "Failed to resolve parent directory for '{}'",
            extension_path.display()
        ))
    })?;
    let base_dir = base_dir.to_owned();
    let extension_path = extension_path.to_owned();
    let http_clients = repository.http_clients.clone();
    tokio::task::spawn_blocking(move || {
        let staging_dir = create_temp_directory(&base_dir, "tmp-extension-migration")?;
        let result: Result<(), DomainError> = (|| {
            stage_embedded_branch(&staging_dir, &remote.url, &target, http_clients.as_ref())?;
            replace_directory(&staging_dir, &extension_path)
        })();
        if result.is_err() {
            cleanup_temp_directory(&staging_dir);
        }
        result
    })
    .await
    .map_err(|error| {
        DomainError::InternalError(format!("Extension migration task failed: {error}"))
    })??;

    if let Err(error) = repository
        .source_store
        .delete(scope, &extension_folder_name)
        .await
    {
        tracing::warn!(
            "Failed to remove migrated extension source metadata for '{}': {error}",
            extension_folder_name
        );
    }
    Ok(())
}

fn normalize_branch(branch: &str) -> Result<String, DomainError> {
    let branch = branch.trim();
    let branch = branch.strip_prefix("origin/").unwrap_or(branch);
    branch_ref(branch)?;
    Ok(branch.to_string())
}

fn switch_embedded(
    extension_path: &Path,
    target: &str,
    http_clients: &tt_adapter_http::HttpClientPool,
) -> Result<(), DomainError> {
    let mut repo = open_embedded(extension_path)?;
    let state = read_managed_state(&repo)?;
    if matches!(
        &state.selected,
        ManagedRef::Branch { display_name, .. } if display_name == target
    ) {
        return Ok(());
    }

    let selected = ManagedRef::branch(target)?;
    let http = GitHttp::new(http_clients.git_blocking_client_builder()).map_err(|error| {
        DomainError::InternalError(format!("Failed to create Git HTTP client: {error}"))
    })?;
    let fetched = fetch_exact(
        &mut repo,
        http,
        &state.remote_url,
        selected.remote_ref(),
        selected.fetch_destination(),
    )?;
    if fetched.commit != state.deployed {
        let mut prepared = prepare_candidate(&repo, fetched.commit)?;
        materialize_candidate(&repo, extension_path, &mut prepared)?;
    }
    configure_branch_switch(&repo, &selected)?;
    finalize_branch_switch(&repo, &selected, fetched.commit)
}
