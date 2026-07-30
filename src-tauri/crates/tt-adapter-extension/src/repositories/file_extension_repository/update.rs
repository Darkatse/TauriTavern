use std::path::Path;

use tt_domain::errors::DomainError;
use tt_domain::models::extension::ExtensionUpdateResult;

use super::FileExtensionRepository;
use super::directory_ops::{cleanup_temp_directory, create_temp_directory, replace_directory};
use super::git_http::GitHttp;
use super::git_remote::{fetch_exact, open_embedded, parse_remote_url};
use super::git_worktree::{
    advance_deployed_ref, configure_symlink_policy, has_standard_embedded_git,
    materialize_candidate, prepare_candidate, read_managed_state,
};
use super::install::stage_embedded;
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

    if has_standard_embedded_git(&extension_path)? {
        let http_clients = repository.http_clients.clone();
        return tokio::task::spawn_blocking(move || {
            update_embedded(&extension_path, http_clients.as_ref())
        })
        .await
        .map_err(|error| {
            DomainError::InternalError(format!("Extension update task failed: {error}"))
        })?;
    }

    update_legacy(repository, scope, &extension_folder_name, &extension_path).await
}

fn update_embedded(
    extension_path: &Path,
    http_clients: &tt_adapter_http::HttpClientPool,
) -> Result<ExtensionUpdateResult, DomainError> {
    let mut repo = open_embedded(extension_path)?;
    let state = read_managed_state(&repo)?;
    let http = GitHttp::new(http_clients.git_blocking_client_builder()).map_err(|error| {
        DomainError::InternalError(format!("Failed to create Git HTTP client: {error}"))
    })?;
    let fetched = fetch_exact(
        &mut repo,
        http,
        &state.remote_url,
        state.selected.remote_ref(),
        state.selected.fetch_destination(),
    )?;
    let is_up_to_date = fetched.commit == state.deployed;

    if !is_up_to_date {
        let mut prepared = prepare_candidate(&repo, fetched.commit)?;
        configure_symlink_policy(&repo)?;
        materialize_candidate(&repo, extension_path, &mut prepared)?;
        advance_deployed_ref(&repo, &state.selected, state.deployed, fetched.commit)?;
    }

    Ok(ExtensionUpdateResult {
        short_commit_hash: fetched.commit.to_hex_with_len(7).to_string(),
        extension_path: extension_path.to_string_lossy().to_string(),
        is_up_to_date,
        remote_url: state.remote_url,
    })
}

async fn update_legacy(
    repository: &FileExtensionRepository,
    scope: ExtensionStoreScope,
    extension_folder_name: &str,
    extension_path: &Path,
) -> Result<ExtensionUpdateResult, DomainError> {
    let source = repository
        .source_store
        .read(scope, extension_folder_name, extension_path)
        .await?
        .ok_or_else(|| {
            DomainError::InvalidData(
                "Extension source metadata is missing. Reinstall this extension to enable updates."
                    .to_string(),
            )
        })?;
    let remote = parse_remote_url(&source.remote_url)?;
    let installed_commit =
        gix::ObjectId::from_hex(source.installed_commit.as_bytes()).map_err(|error| {
            DomainError::InvalidData(format!("Invalid installed Git commit: {error}"))
        })?;
    let base_dir = extension_path.parent().ok_or_else(|| {
        DomainError::InternalError(format!(
            "Failed to resolve parent directory for '{}'",
            extension_path.display()
        ))
    })?;

    let base_dir = base_dir.to_owned();
    let extension_path_display = extension_path.to_string_lossy().to_string();
    let extension_path = extension_path.to_owned();
    let remote_url = remote.url;
    let result_remote_url = remote_url.clone();
    let reference = source.reference;
    let http_clients = repository.http_clients.clone();
    let (commit, is_up_to_date) = tokio::task::spawn_blocking(move || {
        let staging_dir = create_temp_directory(&base_dir, "tmp-extension-migration")?;
        let result: Result<(gix::ObjectId, bool), DomainError> = (|| {
            let staged = stage_embedded(
                &staging_dir,
                &remote_url,
                Some(&reference),
                http_clients.as_ref(),
            )?;
            let is_up_to_date = staged.commit == installed_commit;
            replace_directory(&staging_dir, &extension_path)?;
            Ok((staged.commit, is_up_to_date))
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
        .delete(scope, extension_folder_name)
        .await
    {
        tracing::warn!(
            "Failed to remove migrated extension source metadata for '{}': {error}",
            extension_folder_name
        );
    }

    Ok(ExtensionUpdateResult {
        short_commit_hash: commit.to_hex_with_len(7).to_string(),
        extension_path: extension_path_display,
        is_up_to_date,
        remote_url: result_remote_url,
    })
}
