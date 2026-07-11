use std::fs;
use std::path::Path;

use gix::bstr::ByteSlice;
use uuid::Uuid;

use tt_domain::errors::DomainError;
use tt_domain::models::extension::ExtensionInstallResult;

use super::FileExtensionRepository;
use super::git_http::GitHttp;
use super::git_remote::{
    advertise_refs, branch_ref, fetch_exact, normalize_requested_reference, parse_remote_url,
    remote_ref_name, remote_symbolic_target, tag_ref,
};
use super::git_worktree::{
    ManagedRef, configure_install, create_tracking_ref, finalize_install_ref, init_embedded,
    materialize_candidate, prepare_candidate, read_manifest_from_disk, validate_install_folder,
};

const DEFAULT_HEAD_DESTINATION: &str = "refs/remotes/origin/HEAD";

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
        let staging_dir = base_dir.join(format!(
            ".tmp-extension-install-{}",
            Uuid::new_v4().simple()
        ));
        fs::create_dir(&staging_dir).map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to create extension staging directory '{}': {error}",
                staging_dir.display()
            ))
        })?;

        let result = install_embedded(
            &staging_dir,
            &extension_path,
            &remote.url,
            requested_reference.as_deref(),
            &folder_name,
            http_clients.as_ref(),
        );
        if result.is_err() {
            let _ = fs::remove_dir_all(&staging_dir);
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
    let mut repo = init_embedded(staging_dir)?;
    let http = GitHttp::new(http_clients.git_blocking_client_builder()).map_err(|error| {
        DomainError::InternalError(format!("Failed to create Git HTTP client: {error}"))
    })?;
    let (selected, candidate) = match requested_reference {
        Some(reference) => fetch_requested_ref(&mut repo, http, remote_url, reference)?,
        None => fetch_default_branch(&mut repo, http, remote_url)?,
    };

    let mut prepared = prepare_candidate(&repo, candidate)?;
    materialize_candidate(&repo, staging_dir, &mut prepared)?;
    configure_install(&repo, remote_url, &selected)?;
    finalize_install_ref(&repo, &selected, candidate)?;

    let manifest = read_manifest_from_disk(staging_dir)?;
    if manifest != prepared.manifest {
        return Err(DomainError::InternalError(
            "Installed extension manifest differs from the validated Git object".to_string(),
        ));
    }
    drop(prepared);
    drop(repo);

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
        manifest.display_name,
        manifest.version,
        manifest.author,
        extension_path.display()
    );
    Ok(ExtensionInstallResult {
        version: manifest.version,
        author: manifest.author,
        display_name: manifest.display_name,
        extension_path: extension_path.to_string_lossy().to_string(),
        folder_name: folder_name.to_string(),
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
    let remote_ref = branch_ref(display_name)?;
    let tracking_ref = format!("refs/remotes/origin/{display_name}");
    gix::refs::FullName::try_from(tracking_ref.clone()).map_err(|error| {
        DomainError::InvalidData(format!("Invalid Git tracking reference: {error}"))
    })?;
    create_tracking_ref(repo, &tracking_ref, fetched.commit)?;

    Ok((
        ManagedRef::Branch {
            local_ref: remote_ref.clone(),
            remote_ref,
            tracking_ref,
            display_name: display_name.to_string(),
        },
        fetched.commit,
    ))
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
        let tracking_ref = format!("refs/remotes/origin/{reference}");
        gix::refs::FullName::try_from(tracking_ref.clone()).map_err(|error| {
            DomainError::InvalidData(format!("Invalid Git tracking reference: {error}"))
        })?;
        let fetched = fetch_exact(repo, http, remote_url, &branch, &tracking_ref)?;
        return Ok((
            ManagedRef::Branch {
                local_ref: branch.clone(),
                remote_ref: branch,
                tracking_ref,
                display_name: reference.to_string(),
            },
            fetched.commit,
        ));
    }
    if tag_exists {
        let fetched = fetch_exact(repo, http, remote_url, &tag, &tag)?;
        return Ok((
            ManagedRef::Tag {
                full_ref: tag,
                display_name: reference.to_string(),
            },
            fetched.commit,
        ));
    }

    Err(DomainError::InvalidData(format!(
        "Remote Git branch or tag does not exist: {reference}"
    )))
}
