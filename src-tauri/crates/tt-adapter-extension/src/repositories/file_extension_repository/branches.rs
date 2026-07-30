use std::path::Path;

use tt_domain::errors::DomainError;
use tt_domain::models::extension::ExtensionBranch;

use super::FileExtensionRepository;
use super::git_http::GitHttp;
use super::git_remote::{
    advertise_ref_prefix, open_embedded, parse_remote_url, remote_ref_commit, remote_ref_name,
};
use super::git_worktree::{ManagedRef, has_standard_embedded_git, read_managed_state};
use super::source_store::ExtensionStoreScope;

const BRANCH_PREFIX: &str = "refs/heads/";

pub(super) async fn get_extension_branches(
    repository: &FileExtensionRepository,
    extension_name: &str,
    global: bool,
) -> Result<Vec<ExtensionBranch>, DomainError> {
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
            list_embedded_branches(&extension_path, http_clients.as_ref())
        })
        .await
        .map_err(|error| {
            DomainError::InternalError(format!("Extension branch query task failed: {error}"))
        })?;
    }

    let Some(source) = repository
        .source_store
        .read(scope, &extension_folder_name, &extension_path)
        .await?
    else {
        return Ok(Vec::new());
    };
    let remote = parse_remote_url(&source.remote_url)?;
    let http_clients = repository.http_clients.clone();
    tokio::task::spawn_blocking(move || {
        list_remote_branches(
            &remote.url,
            Some(source.reference.as_str()),
            http_clients.as_ref(),
        )
    })
    .await
    .map_err(|error| {
        DomainError::InternalError(format!("Extension branch query task failed: {error}"))
    })?
}

fn list_embedded_branches(
    extension_path: &Path,
    http_clients: &tt_adapter_http::HttpClientPool,
) -> Result<Vec<ExtensionBranch>, DomainError> {
    let repo = open_embedded(extension_path)?;
    let state = read_managed_state(&repo)?;
    let current = match &state.selected {
        ManagedRef::Branch { display_name, .. } => Some(display_name.as_str()),
        ManagedRef::Tag { .. } => None,
    };
    list_remote_branches(&state.remote_url, current, http_clients)
}

fn list_remote_branches(
    remote_url: &str,
    current: Option<&str>,
    http_clients: &tt_adapter_http::HttpClientPool,
) -> Result<Vec<ExtensionBranch>, DomainError> {
    let http = GitHttp::new(http_clients.git_blocking_client_builder()).map_err(|error| {
        DomainError::InternalError(format!("Failed to create Git HTTP client: {error}"))
    })?;
    let refs = advertise_ref_prefix(http, remote_url, BRANCH_PREFIX)?;
    let mut branches = refs
        .iter()
        .map(|remote_ref| {
            let full_name = std::str::from_utf8(remote_ref_name(remote_ref)).map_err(|_| {
                DomainError::InvalidData("Remote Git branch name is not UTF-8".to_string())
            })?;
            let name = full_name
                .strip_prefix(BRANCH_PREFIX)
                .expect("advertisement was filtered by branch prefix")
                .to_string();
            let commit = remote_ref_commit(remote_ref)?;
            Ok(ExtensionBranch {
                current: current == Some(name.as_str()),
                name,
                commit: commit.to_hex_with_len(7).to_string(),
                label: String::new(),
            })
        })
        .collect::<Result<Vec<_>, DomainError>>()?;
    branches.sort_unstable_by(|left, right| left.name.cmp(&right.name));
    Ok(branches)
}
