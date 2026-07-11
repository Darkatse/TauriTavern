use std::fs;
use std::path::Path;

use gix::bstr::ByteSlice;

use tt_domain::errors::DomainError;
use tt_domain::models::extension::{ExtensionInstallResult, ExtensionManifestMetadata};

use super::FileExtensionRepository;
use super::directory_ops::{cleanup_temp_directory, create_temp_directory};
use super::git_http::GitHttp;
use super::git_remote::{
    advertise_refs, branch_ref, fetch_exact, normalize_requested_reference, parse_remote_url,
    remote_ref_name, remote_symbolic_target, tag_ref,
};
use super::git_worktree::{
    ManagedRef, configure_install, create_tracking_ref, finalize_install_ref, init_embedded,
    materialize_candidate, prepare_candidate, validate_install_folder,
};

const DEFAULT_HEAD_DESTINATION: &str = "refs/remotes/origin/HEAD";

pub(super) struct StagedEmbedded {
    pub(super) manifest: ExtensionManifestMetadata,
    pub(super) commit: gix::ObjectId,
}

pub(super) async fn install_extension(
    repository: &FileExtensionRepository,
    url: &str,
    global: bool,
    reference: Option<String>,
) -> Result<ExtensionInstallResult, DomainError> {
    tracing::info!("Installing extension");

    let remote = parse_remote_url(url)?;
    let requested_reference = normalize_requested_reference(reference);
    let extension_folder_name =
        FileExtensionRepository::install_folder_name_from_repo_name(&remote.repo_name)?;
    validate_install_folder(&extension_folder_name)?;

    let extension_path = repository
        .extension_base_dir(global)
        .join(&extension_folder_name);
    if extension_path.exists() {
        return Err(DomainError::Conflict(format!(
            "Extension already exists at '{}'",
            extension_path.display()
        )));
    }

    let base_dir = repository.extension_base_dir(global).to_owned();
    let http_clients = repository.http_clients.clone();
    let folder_name = extension_folder_name.clone();
    tokio::task::spawn_blocking(move || {
        fs::create_dir_all(&base_dir).map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to create extension directory '{}': {error}",
                base_dir.display()
            ))
        })?;
        let staging_dir = create_temp_directory(&base_dir, "tmp-extension-install")?;

        let result = install_embedded(
            &staging_dir,
            &extension_path,
            &remote.url,
            requested_reference.as_deref(),
            &folder_name,
            http_clients.as_ref(),
        );
        if result.is_err() {
            cleanup_temp_directory(&staging_dir);
        }
        result
    })
    .await
    .map_err(|error| {
        DomainError::InternalError(format!("Extension install task failed: {error}"))
    })?
}

fn install_embedded(
    staging_dir: &Path,
    extension_path: &Path,
    remote_url: &str,
    requested_reference: Option<&str>,
    folder_name: &str,
    http_clients: &tt_adapter_http::HttpClientPool,
) -> Result<ExtensionInstallResult, DomainError> {
    let staged = stage_embedded(staging_dir, remote_url, requested_reference, http_clients)?;

    fs::rename(staging_dir, extension_path).map_err(|error| {
        if extension_path.exists() {
            DomainError::Conflict(format!(
                "Extension already exists at '{}'",
                extension_path.display()
            ))
        } else {
            DomainError::InternalError(format!(
                "Failed to finalize extension installation into '{}': {error}",
                extension_path.display()
            ))
        }
    })?;

    tracing::info!(
        "Extension installed: {} v{} by {} ({})",
        staged.manifest.display_name,
        staged.manifest.version,
        staged.manifest.author,
        extension_path.display()
    );
    Ok(ExtensionInstallResult {
        version: staged.manifest.version,
        author: staged.manifest.author,
        display_name: staged.manifest.display_name,
        extension_path: extension_path.to_string_lossy().to_string(),
        folder_name: folder_name.to_string(),
    })
}

pub(super) fn stage_embedded(
    staging_dir: &Path,
    remote_url: &str,
    requested_reference: Option<&str>,
    http_clients: &tt_adapter_http::HttpClientPool,
) -> Result<StagedEmbedded, DomainError> {
    let mut repo = init_embedded(staging_dir)?;
    let http = GitHttp::new(http_clients.git_blocking_client_builder()).map_err(|error| {
        DomainError::InternalError(format!("Failed to create Git HTTP client: {error}"))
    })?;
    let (selected, candidate) = match requested_reference {
        Some(reference) => fetch_requested_ref(&mut repo, http, remote_url, reference)?,
        None => fetch_default_branch(&mut repo, http, remote_url)?,
    };

    finish_staged_embedded(repo, staging_dir, remote_url, selected, candidate)
}

pub(super) fn stage_embedded_branch(
    staging_dir: &Path,
    remote_url: &str,
    branch: &str,
    http_clients: &tt_adapter_http::HttpClientPool,
) -> Result<StagedEmbedded, DomainError> {
    let mut repo = init_embedded(staging_dir)?;
    let http = GitHttp::new(http_clients.git_blocking_client_builder()).map_err(|error| {
        DomainError::InternalError(format!("Failed to create Git HTTP client: {error}"))
    })?;
    let (selected, candidate) = fetch_branch(&mut repo, http, remote_url, branch)?;

    finish_staged_embedded(repo, staging_dir, remote_url, selected, candidate)
}

fn finish_staged_embedded(
    repo: gix::Repository,
    staging_dir: &Path,
    remote_url: &str,
    selected: ManagedRef,
    candidate: gix::ObjectId,
) -> Result<StagedEmbedded, DomainError> {
    let mut prepared = prepare_candidate(&repo, candidate)?;
    materialize_candidate(&repo, staging_dir, &mut prepared)?;
    configure_install(&repo, remote_url, &selected)?;
    finalize_install_ref(&repo, &selected, candidate)?;

    Ok(StagedEmbedded {
        manifest: prepared.manifest,
        commit: candidate,
    })
}

fn fetch_default_branch(
    repo: &mut gix::Repository,
    http: GitHttp,
    remote_url: &str,
) -> Result<(ManagedRef, gix::ObjectId), DomainError> {
    let fetched = fetch_exact(repo, http, remote_url, "HEAD", DEFAULT_HEAD_DESTINATION)?;
    let target = remote_symbolic_target(&fetched.remote_ref).ok_or_else(|| {
        DomainError::InvalidData(
            "Remote Git HEAD must be a born symbolic branch reference".to_string(),
        )
    })?;
    let target = target
        .to_str()
        .map_err(|_| DomainError::InvalidData("Remote Git HEAD branch is not UTF-8".to_string()))?;
    let display_name = target.strip_prefix("refs/heads/").ok_or_else(|| {
        DomainError::InvalidData("Remote Git HEAD does not point to a branch".to_string())
    })?;
    let selected = ManagedRef::branch(display_name)?;
    let tracking_ref = selected.fetch_destination().to_string();
    create_tracking_ref(repo, &tracking_ref, fetched.commit)?;

    Ok((selected, fetched.commit))
}

fn fetch_requested_ref(
    repo: &mut gix::Repository,
    http: GitHttp,
    remote_url: &str,
    reference: &str,
) -> Result<(ManagedRef, gix::ObjectId), DomainError> {
    let branch = branch_ref(reference)?;
    let tag = tag_ref(reference)?;
    let advertised = advertise_refs(
        http.new_session(),
        remote_url,
        &[branch.clone(), tag.clone()],
    )?;
    let branch_exists = advertised
        .iter()
        .any(|remote_ref| remote_ref_name(remote_ref) == branch.as_bytes());
    let tag_exists = advertised
        .iter()
        .any(|remote_ref| remote_ref_name(remote_ref) == tag.as_bytes());

    if branch_exists {
        return fetch_branch(repo, http, remote_url, reference);
    }
    if tag_exists {
        let fetched = fetch_exact(repo, http, remote_url, &tag, &tag)?;
        return Ok((ManagedRef::tag(reference)?, fetched.commit));
    }

    Err(DomainError::InvalidData(format!(
        "Remote Git branch or tag does not exist: {reference}"
    )))
}

fn fetch_branch(
    repo: &mut gix::Repository,
    http: GitHttp,
    remote_url: &str,
    branch: &str,
) -> Result<(ManagedRef, gix::ObjectId), DomainError> {
    let selected = ManagedRef::branch(branch)?;
    let fetched = fetch_exact(
        repo,
        http,
        remote_url,
        selected.remote_ref(),
        selected.fetch_destination(),
    )?;
    Ok((selected, fetched.commit))
}
