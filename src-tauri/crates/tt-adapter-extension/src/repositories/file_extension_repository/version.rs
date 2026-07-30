use std::path::Path;

use tt_domain::errors::DomainError;
use tt_domain::models::extension::ExtensionVersion;

use super::FileExtensionRepository;
use super::git_http::GitHttp;
use super::git_remote::{
    advertise_refs, branch_ref, exact_remote_ref, parse_remote_url, remote_ref_commit,
    remote_ref_name, tag_ref,
};
use super::git_worktree::{has_standard_embedded_git, read_managed_state};
use super::source_store::ExtensionStoreScope;

pub(super) async fn get_extension_version(
    repository: &FileExtensionRepository,
    extension_name: &str,
    global: bool,
) -> Result<ExtensionVersion, DomainError> {
    tracing::info!("Getting extension version: {}", extension_name);

    let scope = ExtensionStoreScope::from_global(global);
    let extension_folder_name = repository.extension_folder_name_from_identifier(extension_name)?;
    let extension_path = repository.resolve_extension_path(&extension_folder_name, global);
    if !extension_path.exists() {
        return Err(DomainError::NotFound(format!(
            "Extension not found at '{}'",
            extension_path.display()
        )));
    }

    match has_standard_embedded_git(&extension_path)? {
        true => {
            let http_clients = repository.http_clients.clone();
            tokio::task::spawn_blocking(move || {
                embedded_version(&extension_path, http_clients.as_ref())
            })
            .await
            .map_err(|error| {
                DomainError::InternalError(format!("Extension version task failed: {error}"))
            })?
        }
        false => {
            let source = match repository
                .source_store
                .read(scope, &extension_folder_name, &extension_path)
                .await?
            {
                Some(source) => source,
                None => return Ok(unmanaged_version()),
            };
            let remote = parse_remote_url(&source.remote_url)?;
            let http_clients = repository.http_clients.clone();
            tokio::task::spawn_blocking(move || {
                legacy_version(
                    &remote.url,
                    &source.reference,
                    &source.installed_commit,
                    http_clients.as_ref(),
                )
            })
            .await
            .map_err(|error| {
                DomainError::InternalError(format!("Extension version task failed: {error}"))
            })?
        }
    }
}

fn embedded_version(
    extension_path: &Path,
    http_clients: &tt_adapter_http::HttpClientPool,
) -> Result<ExtensionVersion, DomainError> {
    let repo = super::git_remote::open_embedded(extension_path)?;
    let state = read_managed_state(&repo)?;
    let http = GitHttp::new(http_clients.git_blocking_client_builder()).map_err(|error| {
        DomainError::InternalError(format!("Failed to create Git HTTP client: {error}"))
    })?;
    let requested = state.selected.remote_ref().to_string();
    let refs = advertise_refs(http, &state.remote_url, std::slice::from_ref(&requested))?;
    let remote_commit = remote_ref_commit(exact_remote_ref(&refs, &requested)?)?;

    Ok(ExtensionVersion {
        current_branch_name: state.selected.display_name().to_string(),
        current_commit_hash: state.deployed.to_string(),
        is_up_to_date: state.deployed == remote_commit,
        remote_url: state.remote_url,
    })
}

fn legacy_version(
    remote_url: &str,
    reference: &str,
    installed_commit: &str,
    http_clients: &tt_adapter_http::HttpClientPool,
) -> Result<ExtensionVersion, DomainError> {
    let branch = branch_ref(reference)?;
    let tag = tag_ref(reference)?;
    let http = GitHttp::new(http_clients.git_blocking_client_builder()).map_err(|error| {
        DomainError::InternalError(format!("Failed to create Git HTTP client: {error}"))
    })?;
    let refs = advertise_refs(http, remote_url, &[branch.clone(), tag.clone()])?;
    let selected = if refs
        .iter()
        .any(|remote_ref| remote_ref_name(remote_ref) == branch.as_bytes())
    {
        branch
    } else if refs
        .iter()
        .any(|remote_ref| remote_ref_name(remote_ref) == tag.as_bytes())
    {
        tag
    } else {
        return Err(DomainError::InvalidData(format!(
            "Remote Git branch or tag does not exist: {reference}"
        )));
    };
    let remote_commit = remote_ref_commit(exact_remote_ref(&refs, &selected)?)?;
    let deployed = gix::ObjectId::from_hex(installed_commit.as_bytes()).map_err(|error| {
        DomainError::InvalidData(format!("Invalid installed Git commit: {error}"))
    })?;

    Ok(ExtensionVersion {
        current_branch_name: reference.to_string(),
        current_commit_hash: deployed.to_string(),
        is_up_to_date: deployed == remote_commit,
        remote_url: remote_url.to_string(),
    })
}

fn unmanaged_version() -> ExtensionVersion {
    ExtensionVersion {
        current_branch_name: String::new(),
        current_commit_hash: String::new(),
        is_up_to_date: true,
        remote_url: String::new(),
    }
}
