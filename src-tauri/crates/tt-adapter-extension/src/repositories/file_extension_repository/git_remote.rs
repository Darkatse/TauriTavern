use std::borrow::Cow;
use std::num::NonZeroU32;
use std::path::Path;
use std::sync::atomic::AtomicBool;

use gix::bstr::{BString, ByteSlice};
use gix::protocol::handshake::Ref as RemoteRef;
use gix::remote::{self, Direction};
use gix_transport::client::blocking_io::http::Transport as HttpTransport;
use gix_transport::{Protocol, Service};
use url::Url;

use tt_domain::errors::DomainError;

use super::git_http::GitHttp;

const PROTOCOL_AGENT: &str = "TauriTavern";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct GitRemoteSpec {
    pub(super) url: String,
    pub(super) repo_name: String,
}

#[derive(Debug, Clone)]
pub(super) struct FetchedRef {
    pub(super) commit: gix::ObjectId,
    pub(super) remote_ref: RemoteRef,
}

pub(super) fn parse_remote_url(value: &str) -> Result<GitRemoteSpec, DomainError> {
    let url = Url::parse(value.trim())
        .map_err(|error| DomainError::InvalidData(format!("Invalid Git remote URL: {error}")))?;

    if !matches!(url.scheme(), "http" | "https") {
        return Err(DomainError::InvalidData(
            "Only HTTP and HTTPS Git remotes are supported".to_string(),
        ));
    }
    if url.host_str().is_none() {
        return Err(DomainError::InvalidData(
            "Git remote URL is missing a host".to_string(),
        ));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(DomainError::InvalidData(
            "Authenticated Git remote URLs are not supported".to_string(),
        ));
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err(DomainError::InvalidData(
            "Git remote URL must not contain a query or fragment".to_string(),
        ));
    }

    let repo_segment = url
        .path_segments()
        .and_then(|mut segments| segments.rfind(|segment| !segment.is_empty()))
        .ok_or_else(|| {
            DomainError::InvalidData("Git remote URL must include a repository path".to_string())
        })?;
    let repo_name = repo_segment.strip_suffix(".git").unwrap_or(repo_segment);
    if repo_name.is_empty() {
        return Err(DomainError::InvalidData(
            "Git remote URL is missing a repository name".to_string(),
        ));
    }

    Ok(GitRemoteSpec {
        url: url.to_string(),
        repo_name: repo_name.to_string(),
    })
}

pub(super) fn normalize_requested_reference(reference: Option<String>) -> Option<String> {
    reference
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub(super) fn branch_ref(name: &str) -> Result<String, DomainError> {
    validate_full_ref(format!("refs/heads/{name}"))
}

pub(super) fn tag_ref(name: &str) -> Result<String, DomainError> {
    validate_full_ref(format!("refs/tags/{name}"))
}

fn validate_full_ref(name: String) -> Result<String, DomainError> {
    gix::refs::FullName::try_from(name.clone()).map_err(|error| {
        DomainError::InvalidData(format!("Invalid Git reference name: {error}"))
    })?;
    Ok(name)
}

pub(super) fn advertise_refs(
    http: GitHttp,
    remote_url: &str,
    requested_refs: &[String],
) -> Result<Vec<RemoteRef>, DomainError> {
    Ok(query_refs(http, remote_url, requested_refs)?
        .into_iter()
        .filter(|remote_ref| {
            requested_refs
                .iter()
                .any(|requested| remote_ref_name(remote_ref) == requested.as_bytes())
        })
        .collect())
}

pub(super) fn advertise_ref_prefix(
    http: GitHttp,
    remote_url: &str,
    prefix: &str,
) -> Result<Vec<RemoteRef>, DomainError> {
    Ok(query_refs(http, remote_url, &[prefix.to_string()])?
        .into_iter()
        .filter(|remote_ref| remote_ref_name(remote_ref).starts_with(prefix.as_bytes()))
        .collect())
}

#[expect(
    clippy::result_large_err,
    reason = "gix fixes the credential callback error type; anonymous transport always returns Ok(None)"
)]
fn query_refs(
    http: GitHttp,
    remote_url: &str,
    requested_prefixes: &[String],
) -> Result<Vec<RemoteRef>, DomainError> {
    let git_url = gix::url::parse(remote_url.as_bytes().as_bstr())
        .map_err(|error| DomainError::InvalidData(format!("Invalid Git transport URL: {error}")))?;
    let mut transport = HttpTransport::new_http(http, git_url, Protocol::V2, false);
    let mut progress = gix::progress::Discard;
    let handshake = gix::protocol::handshake(
        &mut transport,
        Service::UploadPack,
        |_action| Ok(None),
        Vec::new(),
        &mut progress,
    )
    .map_err(|error| git_error("Git handshake failed", error))?;

    let refs = match handshake.refs {
        Some(refs) => refs,
        None => {
            let mut ref_prefixes = gix::protocol::ls_refs::RefPrefixes::new();
            ref_prefixes.extend(
                requested_prefixes
                    .iter()
                    .map(|name| BString::from(name.as_str())),
            );
            gix::protocol::LsRefsCommand::new(
                Some(ref_prefixes),
                &handshake.capabilities,
                (
                    "agent",
                    Some(Cow::Owned(gix::protocol::agent(PROTOCOL_AGENT))),
                ),
            )
            .invoke_blocking(&mut transport, &mut progress, false)
            .map_err(|error| git_error("Git reference advertisement failed", error))?
        }
    };

    Ok(refs)
}

#[expect(
    clippy::result_large_err,
    reason = "gix fixes the credential callback error type; anonymous transport always returns Ok(None)"
)]
pub(super) fn fetch_exact(
    repo: &mut gix::Repository,
    http: GitHttp,
    remote_url: &str,
    source: &str,
    destination: &str,
) -> Result<FetchedRef, DomainError> {
    let previous_reflog = repo.refs.write_reflog;
    repo.refs.write_reflog = gix::refs::store::WriteReflog::Disable;
    let result = (|| {
        let refspec = format!("+{source}:{destination}");
        let remote = repo
            .remote_at_without_url_rewrite(remote_url)
            .map_err(|error| git_error("Failed to configure Git remote", error))?
            .with_refspecs([refspec.as_str()], Direction::Fetch)
            .map_err(|error| git_error("Failed to configure Git refspec", error))?
            .with_fetch_tags(remote::fetch::Tags::None);
        let url = remote
            .url(Direction::Fetch)
            .ok_or_else(|| DomainError::InvalidData("Git remote has no fetch URL".to_string()))?
            .to_owned();
        let transport = HttpTransport::new_http(http, url, Protocol::V2, false);
        let prepared = remote
            .to_connection_with_transport(transport)
            .with_credentials(|_action| Ok(None))
            .prepare_fetch(gix::progress::Discard, remote::ref_map::Options::default())
            .map_err(|error| git_error("Failed to prepare Git fetch", error))?;
        let outcome = prepared
            .with_shallow(remote::fetch::Shallow::DepthAtRemote(
                NonZeroU32::new(1).expect("one is non-zero"),
            ))
            .receive(gix::progress::Discard, &AtomicBool::new(false))
            .map_err(|error| git_error("Git fetch failed", error))?;

        let remote_ref = exact_remote_ref(&outcome.ref_map.remote_refs, source)?.clone();
        let commit = remote_ref_commit(&remote_ref)?;
        Ok(FetchedRef { commit, remote_ref })
    })();
    repo.refs.write_reflog = previous_reflog;
    result
}

pub(super) fn exact_remote_ref<'a>(
    refs: &'a [RemoteRef],
    full_name: &str,
) -> Result<&'a RemoteRef, DomainError> {
    let mut matches = refs
        .iter()
        .filter(|remote_ref| remote_ref_name(remote_ref) == full_name.as_bytes());
    let remote_ref = matches.next().ok_or_else(|| {
        DomainError::InternalError(format!("Remote Git reference does not exist: {full_name}"))
    })?;
    if matches.next().is_some() {
        return Err(DomainError::InternalError(format!(
            "Remote Git reference is ambiguous: {full_name}"
        )));
    }
    Ok(remote_ref)
}

pub(super) fn remote_ref_commit(remote_ref: &RemoteRef) -> Result<gix::ObjectId, DomainError> {
    match remote_ref {
        RemoteRef::Peeled { object, .. }
        | RemoteRef::Direct { object, .. }
        | RemoteRef::Symbolic { object, .. } => Ok(*object),
        RemoteRef::Unborn { .. } => Err(DomainError::InvalidData(
            "Remote Git reference is unborn".to_string(),
        )),
    }
}

pub(super) fn remote_ref_name(remote_ref: &RemoteRef) -> &[u8] {
    match remote_ref {
        RemoteRef::Peeled { full_ref_name, .. }
        | RemoteRef::Direct { full_ref_name, .. }
        | RemoteRef::Symbolic { full_ref_name, .. }
        | RemoteRef::Unborn { full_ref_name, .. } => full_ref_name.as_ref(),
    }
}

pub(super) fn remote_symbolic_target(remote_ref: &RemoteRef) -> Option<&[u8]> {
    match remote_ref {
        RemoteRef::Symbolic { target, .. } => Some(target.as_ref()),
        _ => None,
    }
}

pub(super) fn open_embedded(path: &Path) -> Result<gix::Repository, DomainError> {
    let repo = gix::open_opts(path, gix::open::Options::isolated().strict_config(true))
        .map_err(|error| git_error("Failed to open embedded Git repository", error))?;
    let expected_workdir = canonicalize(path, "extension worktree")?;
    let expected_git_dir = canonicalize(&path.join(".git"), "extension Git directory")?;
    let git_dir = canonicalize(repo.git_dir(), "embedded Git directory")?;
    let common_dir = canonicalize(repo.common_dir(), "embedded Git common directory")?;
    let workdir = repo.workdir().ok_or_else(unsupported_layout)?;
    let workdir = canonicalize(workdir, "embedded Git worktree")?;

    if repo.is_bare()
        || repo.kind() != gix::repository::Kind::Common
        || git_dir != expected_git_dir
        || common_dir != git_dir
        || workdir != expected_workdir
    {
        return Err(unsupported_layout());
    }

    Ok(repo)
}

fn canonicalize(path: &Path, label: &str) -> Result<std::path::PathBuf, DomainError> {
    std::fs::canonicalize(path).map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to resolve {label} '{}': {error}",
            path.display()
        ))
    })
}

fn unsupported_layout() -> DomainError {
    DomainError::InvalidData(
        "Unsupported embedded Git layout. Reinstall this extension to enable Git management."
            .to_string(),
    )
}

fn git_error(context: &str, error: impl std::fmt::Display) -> DomainError {
    DomainError::InternalError(format!("{context}: {error}"))
}

#[cfg(test)]
mod tests {
    use gix::protocol::handshake::Ref as RemoteRef;

    use super::*;

    #[test]
    fn parses_anonymous_http_remote_without_provider_restrictions() {
        let spec =
            parse_remote_url("https://git.example.test:8443/group/repo.git").expect("parse remote");
        assert_eq!(spec.url, "https://git.example.test:8443/group/repo.git");
        assert_eq!(spec.repo_name, "repo");
    }

    #[test]
    fn rejects_remote_identity_modifiers() {
        for url in [
            "ssh://example.test/repo.git",
            "https://user@example.test/repo.git",
            "https://example.test/repo.git?ref=main",
            "https://example.test/repo.git#main",
        ] {
            assert!(parse_remote_url(url).is_err(), "{url} must be rejected");
        }
    }

    #[test]
    fn validates_exact_branch_and_tag_names() {
        assert_eq!(
            branch_ref("feature/mobile").unwrap(),
            "refs/heads/feature/mobile"
        );
        assert_eq!(tag_ref("v1.2.3").unwrap(), "refs/tags/v1.2.3");
        assert!(branch_ref("bad..name").is_err());
    }

    #[test]
    fn peels_annotated_remote_tag_to_commit() {
        let tag = gix::ObjectId::from_hex(b"1111111111111111111111111111111111111111").unwrap();
        let commit = gix::ObjectId::from_hex(b"2222222222222222222222222222222222222222").unwrap();
        let remote_ref = RemoteRef::Peeled {
            full_ref_name: "refs/tags/v1".into(),
            tag,
            object: commit,
        };
        assert_eq!(remote_ref_commit(&remote_ref).unwrap(), commit);
    }
}
