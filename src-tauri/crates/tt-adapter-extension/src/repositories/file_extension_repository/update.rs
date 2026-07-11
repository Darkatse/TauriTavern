use std::path::Path;

use tt_domain::errors::DomainError;
use tt_domain::models::extension::ExtensionUpdateResult;

use super::FileExtensionRepository;
use super::git_http::GitHttp;
use super::git_remote::{fetch_exact, open_embedded};
use super::git_worktree::{
    advance_deployed_ref, configure_symlink_policy, has_standard_embedded_git,
    materialize_candidate, prepare_candidate, read_managed_state,
};
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

    let commit = fetched.commit.to_string();
    Ok(ExtensionUpdateResult {
        short_commit_hash: FileExtensionRepository::short_commit_hash(&commit),
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
    let mut source = repository
        .resolve_source_metadata(scope, extension_folder_name, extension_path)
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
        if let Err(error) = repository.replace_directory(&staging_dir, extension_path) {
            FileExtensionRepository::cleanup_temp_directory(&staging_dir).await;
            return Err(error);
        }

        source.installed_commit = latest_commit.clone();
        repository
            .source_store
            .write(scope, extension_folder_name, &source)
            .await?;
    }

    Ok(ExtensionUpdateResult {
        short_commit_hash: FileExtensionRepository::short_commit_hash(&latest_commit),
        extension_path: extension_path.to_string_lossy().to_string(),
        is_up_to_date,
        remote_url: source.remote_url,
    })
}
