//! Standard embedded worktree state and deterministic materialization.

use std::borrow::Cow;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use gix::bstr::{BStr, ByteSlice};
use gix::refs::Target;
use gix::refs::transaction::{Change, LogChange, PreviousValue, RefEdit};

use tt_domain::errors::DomainError;
use tt_domain::models::extension::ExtensionManifestMetadata;

use super::git_remote::{branch_ref, parse_remote_url, tag_ref};

const ORIGIN: &str = "origin";
const MANIFEST_PATH: &str = "manifest.json";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ManagedRef {
    Branch {
        local_ref: String,
        remote_ref: String,
        tracking_ref: String,
        display_name: String,
    },
    Tag {
        full_ref: String,
        display_name: String,
    },
}

impl ManagedRef {
    pub(super) fn branch(display_name: &str) -> Result<Self, DomainError> {
        let remote_ref = branch_ref(display_name)?;
        let tracking_ref = format!("refs/remotes/origin/{display_name}");
        gix::refs::FullName::try_from(tracking_ref.clone()).map_err(|error| {
            DomainError::InvalidData(format!("Invalid Git tracking reference: {error}"))
        })?;
        Ok(Self::Branch {
            local_ref: remote_ref.clone(),
            remote_ref,
            tracking_ref,
            display_name: display_name.to_string(),
        })
    }

    pub(super) fn tag(display_name: &str) -> Result<Self, DomainError> {
        Ok(Self::Tag {
            full_ref: tag_ref(display_name)?,
            display_name: display_name.to_string(),
        })
    }

    pub(super) fn remote_ref(&self) -> &str {
        match self {
            Self::Branch { remote_ref, .. } => remote_ref,
            Self::Tag { full_ref, .. } => full_ref,
        }
    }

    pub(super) fn fetch_destination(&self) -> &str {
        match self {
            Self::Branch { tracking_ref, .. } => tracking_ref,
            Self::Tag { full_ref, .. } => full_ref,
        }
    }

    pub(super) fn display_name(&self) -> &str {
        match self {
            Self::Branch { display_name, .. } | Self::Tag { display_name, .. } => display_name,
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct ManagedState {
    pub(super) selected: ManagedRef,
    pub(super) deployed: gix::ObjectId,
    pub(super) remote_url: String,
}

pub(super) struct PreparedCandidate {
    pub(super) manifest: ExtensionManifestMetadata,
    gitlinks: Vec<PathBuf>,
    index: gix::index::File,
}

pub(super) fn has_standard_embedded_git(extension_path: &Path) -> Result<bool, DomainError> {
    let dot_git = extension_path.join(".git");
    match fs::symlink_metadata(&dot_git) {
        Ok(metadata) if metadata.file_type().is_dir() => Ok(true),
        Ok(_) => Err(DomainError::InvalidData(format!(
            "Unsupported embedded Git layout at '{}'",
            dot_git.display()
        ))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(DomainError::InternalError(format!(
            "Failed to inspect embedded Git layout '{}': {error}",
            dot_git.display()
        ))),
    }
}

pub(super) fn init_embedded(path: &Path) -> Result<gix::Repository, DomainError> {
    let repo = gix::ThreadSafeRepository::init_opts(
        path,
        gix::create::Kind::WithWorktree,
        gix::create::Options {
            destination_must_be_empty: Some(true),
            ..Default::default()
        },
        gix::open::Options::isolated().strict_config(true),
    )
    .map_err(|error| git_error("Failed to initialize embedded Git repository", error))?;
    Ok(repo.to_thread_local())
}

pub(super) fn read_managed_state(repo: &gix::Repository) -> Result<ManagedState, DomainError> {
    let head = repo
        .head()
        .map_err(|error| git_error("Failed to read embedded Git HEAD", error))?;
    let deployed = head
        .id()
        .ok_or_else(|| DomainError::InvalidData("Embedded Git HEAD is unborn".to_string()))?;
    repo.find_commit(deployed.detach())
        .map_err(|error| git_error("Embedded Git HEAD does not point to a commit", error))?;

    if let Some(branch) = head.try_into_referent() {
        let local_ref = utf8_ref(branch.name().as_bstr())?;
        if !local_ref.starts_with("refs/heads/") {
            return Err(DomainError::InvalidData(
                "Embedded Git HEAD does not point to a local branch".to_string(),
            ));
        }
        let remote_name = branch
            .remote_name(gix::remote::Direction::Fetch)
            .ok_or_else(|| {
                DomainError::InvalidData("Embedded Git branch has no fetch remote".to_string())
            })?;
        if !matches!(
            &remote_name,
            gix::remote::Name::Symbol(name) if name.as_ref() == ORIGIN
        ) {
            return Err(DomainError::InvalidData(
                "Embedded Git branch must track the origin remote".to_string(),
            ));
        }
        let remote_ref = branch
            .remote_ref_name(gix::remote::Direction::Fetch)
            .ok_or_else(|| {
                DomainError::InvalidData(
                    "Embedded Git branch has no upstream merge ref".to_string(),
                )
            })?
            .map_err(|error| git_error("Invalid embedded Git upstream ref", error))?;
        let remote_ref = utf8_ref(remote_ref.as_bstr())?;
        let remote = repo
            .find_remote(ORIGIN)
            .map_err(|error| git_error("Failed to open embedded Git remote", error))?;
        let remote_url = remote_url(&remote)?;
        let display_name = remote_ref
            .strip_prefix("refs/heads/")
            .ok_or_else(|| {
                DomainError::InvalidData(
                    "Embedded Git upstream is not a branch reference".to_string(),
                )
            })?
            .to_string();
        let selected = ManagedRef::branch(&display_name)?;
        if !matches!(
            &selected,
            ManagedRef::Branch { local_ref: canonical, .. } if canonical == &local_ref
        ) {
            return Err(DomainError::InvalidData(
                "Embedded Git local branch must match its origin branch".to_string(),
            ));
        }

        return Ok(ManagedState {
            selected,
            deployed: deployed.detach(),
            remote_url,
        });
    }

    let remote = repo
        .find_remote(ORIGIN)
        .map_err(|error| git_error("Failed to open embedded Git origin", error))?;
    let mut tag_refs = remote
        .refspecs(gix::remote::Direction::Fetch)
        .iter()
        .filter_map(|spec| {
            let spec = spec.to_ref();
            let source = spec.source()?;
            let destination = spec.destination()?;
            (source == destination && source.starts_with(b"refs/tags/")).then(|| source.to_owned())
        });
    let full_ref = tag_refs.next().ok_or_else(|| {
        DomainError::InvalidData(
            "Detached embedded Git HEAD has no exact tag fetch refspec".to_string(),
        )
    })?;
    if tag_refs.next().is_some() {
        return Err(DomainError::InvalidData(
            "Detached embedded Git HEAD has ambiguous tag fetch refspecs".to_string(),
        ));
    }
    let full_ref = utf8_ref(full_ref.as_bstr())?;
    let display_name = full_ref
        .strip_prefix("refs/tags/")
        .expect("filtered tag ref")
        .to_string();

    Ok(ManagedState {
        selected: ManagedRef::Tag {
            full_ref,
            display_name,
        },
        deployed: deployed.detach(),
        remote_url: remote_url(&remote)?,
    })
}

pub(super) fn prepare_candidate(
    repo: &gix::Repository,
    commit: gix::ObjectId,
) -> Result<PreparedCandidate, DomainError> {
    let commit = repo
        .find_commit(commit)
        .map_err(|error| git_error("Candidate is not a Git commit", error))?;
    let tree = commit
        .tree()
        .map_err(|error| git_error("Candidate Git commit has no complete tree", error))?;
    let entries = tree
        .traverse()
        .breadthfirst
        .files()
        .map_err(|error| git_error("Failed to traverse candidate Git tree", error))?;

    let mut paths = HashMap::with_capacity(entries.len());
    let mut manifest = None;
    let mut gitlinks = Vec::new();

    for entry in &entries {
        let path = entry.filepath.to_str().map_err(|_| {
            DomainError::InvalidData("Extension Git tree contains a non-UTF-8 path".to_string())
        })?;
        validate_portable_path(path, entry.mode.is_link())?;

        let canonical = gix::utils::str::precompose(Cow::Borrowed(path)).to_lowercase();
        if paths.insert(canonical, entry.mode).is_some() {
            return Err(DomainError::InvalidData(format!(
                "Extension Git tree contains a portable path collision: {path}"
            )));
        }

        if path == MANIFEST_PATH && !entry.mode.is_blob() {
            return Err(DomainError::InvalidData(
                "Extension manifest.json must be a regular file".to_string(),
            ));
        }
        if entry.mode.is_commit() {
            gitlinks.push(PathBuf::from(path));
            continue;
        }

        let expected_kind = if entry.mode.is_tree() {
            gix::object::Kind::Tree
        } else {
            gix::object::Kind::Blob
        };

        if path == MANIFEST_PATH {
            let object = repo
                .find_object(entry.oid)
                .map_err(|error| git_error("Candidate Git manifest object is missing", error))?;
            if object.kind != expected_kind {
                return Err(DomainError::InvalidData(
                    "Extension manifest.json has the wrong object kind".to_string(),
                ));
            }
            manifest = Some(serde_json::from_slice(&object.data).map_err(|error| {
                DomainError::InvalidData(format!("Invalid extension manifest.json: {error}"))
            })?);
        } else {
            let header = repo.find_header(entry.oid).map_err(|error| {
                git_error(
                    &format!("Candidate Git object is missing for '{path}'"),
                    error,
                )
            })?;
            if header.kind() != expected_kind {
                return Err(DomainError::InvalidData(format!(
                    "Extension Git tree entry '{path}' has the wrong object kind"
                )));
            }
        }
    }

    validate_tree_prefixes(&paths)?;
    let manifest = manifest.ok_or_else(|| {
        DomainError::InvalidData("Extension manifest.json is missing".to_string())
    })?;
    let index = repo
        .index_from_tree(&tree.id)
        .map_err(|error| git_error("Failed to build candidate Git index", error))?;

    Ok(PreparedCandidate {
        manifest,
        gitlinks,
        index,
    })
}

pub(super) fn materialize_candidate(
    repo: &gix::Repository,
    workdir: &Path,
    prepared: &mut PreparedCandidate,
) -> Result<(), DomainError> {
    clear_payload(workdir)?;

    let mut options = repo
        .checkout_options(gix::worktree::stack::state::attributes::Source::IdMapping)
        .map_err(|error| git_error("Failed to configure Git checkout", error))?;
    options.destination_is_initially_empty = true;
    options.overwrite_existing = false;
    options.keep_going = false;
    options.fs.symlink = false;
    options.filters.options_mut().drivers.clear();
    options.filter_process_delay = gix::filter::plumbing::driver::apply::Delay::Forbid;

    let interrupted = AtomicBool::new(false);
    let outcome = gix::worktree::state::checkout(
        &mut prepared.index,
        workdir,
        repo.objects
            .clone()
            .into_arc()
            .map_err(|error| git_error("Failed to prepare Git object database", error))?,
        &gix::progress::Discard,
        &gix::progress::Discard,
        &interrupted,
        options,
    )
    .map_err(|error| git_error("Failed to materialize extension Git worktree", error))?;

    if interrupted.load(std::sync::atomic::Ordering::Relaxed)
        || !outcome.collisions.is_empty()
        || !outcome.errors.is_empty()
        || !outcome.delayed_paths_unknown.is_empty()
        || !outcome.delayed_paths_unprocessed.is_empty()
    {
        return Err(DomainError::InternalError(
            "Extension Git checkout did not complete cleanly".to_string(),
        ));
    }

    for path in &prepared.gitlinks {
        fs::create_dir_all(workdir.join(path)).map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to materialize Git submodule placeholder '{}': {error}",
                path.display()
            ))
        })?;
    }
    prepared
        .index
        .write(Default::default())
        .map_err(|error| git_error("Failed to write extension Git index", error))?;
    Ok(())
}

pub(super) fn configure_install(
    repo: &gix::Repository,
    remote_url: &str,
    selected: &ManagedRef,
) -> Result<(), DomainError> {
    update_config(repo, |config| {
        set_config(config, "core", None, "symlinks", "false")?;
        set_config(config, "remote", Some(ORIGIN), "url", remote_url)?;
        let refspec = format!(
            "+{}:{}",
            selected.remote_ref(),
            selected.fetch_destination()
        );
        set_config(config, "remote", Some(ORIGIN), "fetch", &refspec)?;

        if let ManagedRef::Branch {
            local_ref,
            remote_ref,
            ..
        } = selected
        {
            let local_name = local_ref
                .strip_prefix("refs/heads/")
                .expect("branch local ref");
            set_config(config, "branch", Some(local_name), "remote", ORIGIN)?;
            set_config(config, "branch", Some(local_name), "merge", remote_ref)?;
        }
        Ok(())
    })
}

pub(super) fn configure_symlink_policy(repo: &gix::Repository) -> Result<(), DomainError> {
    update_config(repo, |config| {
        set_config(config, "core", None, "symlinks", "false")
    })
}

pub(super) fn configure_branch_switch(
    repo: &gix::Repository,
    selected: &ManagedRef,
) -> Result<(), DomainError> {
    let ManagedRef::Branch {
        local_ref,
        remote_ref,
        ..
    } = selected
    else {
        return Err(DomainError::InvalidData(
            "Extension switch target is not a branch".to_string(),
        ));
    };
    let local_name = local_ref
        .strip_prefix("refs/heads/")
        .expect("branch local ref");
    let refspec = format!(
        "+{}:{}",
        selected.remote_ref(),
        selected.fetch_destination()
    );

    update_config(repo, |config| {
        set_config(config, "core", None, "symlinks", "false")?;
        {
            let mut origin = config
                .section_mut("remote", Some(ORIGIN.as_bytes().as_bstr()))
                .map_err(|error| git_error("Embedded Git origin is missing", error))?;
            while origin.remove("fetch").is_some() {}
        }
        set_config(config, "remote", Some(ORIGIN), "fetch", &refspec)?;
        set_config(config, "branch", Some(local_name), "remote", ORIGIN)?;
        set_config(config, "branch", Some(local_name), "merge", remote_ref)
    })
}

pub(super) fn create_tracking_ref(
    repo: &gix::Repository,
    name: &str,
    commit: gix::ObjectId,
) -> Result<(), DomainError> {
    apply_ref_edits(
        repo,
        [ref_edit(
            name,
            Target::Object(commit),
            PreviousValue::Any,
            "record remote branch",
        )?],
    )
}

pub(super) fn finalize_install_ref(
    repo: &gix::Repository,
    selected: &ManagedRef,
    commit: gix::ObjectId,
) -> Result<(), DomainError> {
    match selected {
        ManagedRef::Branch { local_ref, .. } => {
            apply_ref_edits(
                repo,
                [
                    ref_edit(
                        local_ref,
                        Target::Object(commit),
                        PreviousValue::MustNotExist,
                        "install extension",
                    )?,
                    ref_edit(
                        "HEAD",
                        Target::Symbolic(local_ref.as_str().try_into().map_err(|error| {
                            git_error("Invalid embedded Git branch ref", error)
                        })?),
                        PreviousValue::Any,
                        "select extension branch",
                    )?,
                ],
            )
        }
        ManagedRef::Tag { .. } => apply_ref_edits(
            repo,
            [ref_edit(
                "HEAD",
                Target::Object(commit),
                PreviousValue::Any,
                "install extension tag",
            )?],
        ),
    }
}

pub(super) fn finalize_branch_switch(
    repo: &gix::Repository,
    selected: &ManagedRef,
    commit: gix::ObjectId,
) -> Result<(), DomainError> {
    let ManagedRef::Branch { local_ref, .. } = selected else {
        return Err(DomainError::InvalidData(
            "Extension switch target is not a branch".to_string(),
        ));
    };
    apply_ref_edits(
        repo,
        [
            ref_edit(
                local_ref,
                Target::Object(commit),
                PreviousValue::Any,
                "switch extension branch",
            )?,
            ref_edit(
                "HEAD",
                Target::Symbolic(
                    local_ref
                        .as_str()
                        .try_into()
                        .map_err(|error| git_error("Invalid embedded Git branch ref", error))?,
                ),
                PreviousValue::Any,
                "select extension branch",
            )?,
        ],
    )
}

pub(super) fn advance_deployed_ref(
    repo: &gix::Repository,
    selected: &ManagedRef,
    previous: gix::ObjectId,
    next: gix::ObjectId,
) -> Result<(), DomainError> {
    let name = match selected {
        ManagedRef::Branch { local_ref, .. } => local_ref.as_str(),
        ManagedRef::Tag { .. } => "HEAD",
    };
    apply_ref_edits(
        repo,
        [ref_edit(
            name,
            Target::Object(next),
            PreviousValue::MustExistAndMatch(Target::Object(previous)),
            "update extension",
        )?],
    )
}

pub(super) fn validate_install_folder(name: &str) -> Result<(), DomainError> {
    validate_portable_path(name, false)
}

fn validate_portable_path(path: &str, is_symlink: bool) -> Result<(), DomainError> {
    let leaf_index = path.split('/').count().saturating_sub(1);
    for (index, component) in path.split('/').enumerate() {
        let mode = (is_symlink && index == leaf_index)
            .then_some(gix::validate::path::component::Mode::Symlink);
        gix::validate::path::component(component.as_bytes().as_bstr(), mode, Default::default())
            .map_err(|error| {
                DomainError::InvalidData(format!(
                    "Extension Git tree contains invalid path '{path}': {error}"
                ))
            })?;
    }
    Ok(())
}

fn validate_tree_prefixes(
    paths: &HashMap<String, gix::objs::tree::EntryMode>,
) -> Result<(), DomainError> {
    for path in paths.keys() {
        for (index, _) in path.match_indices('/') {
            let prefix = &path[..index];
            match paths.get(prefix) {
                Some(mode) if mode.is_tree() => {}
                _ => {
                    return Err(DomainError::InvalidData(format!(
                        "Extension Git tree has an invalid file/directory shape at '{prefix}'"
                    )));
                }
            }
        }
    }
    Ok(())
}

fn clear_payload(workdir: &Path) -> Result<(), DomainError> {
    for entry in fs::read_dir(workdir).map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to read extension directory '{}': {error}",
            workdir.display()
        ))
    })? {
        let entry = entry.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read extension directory entry '{}': {error}",
                workdir.display()
            ))
        })?;
        if entry.file_name() == ".git" {
            continue;
        }
        let path = entry.path();
        let file_type = fs::symlink_metadata(&path)
            .map_err(|error| git_error("Failed to inspect extension payload", error))?
            .file_type();
        if file_type.is_dir() {
            fs::remove_dir_all(&path)
        } else {
            fs::remove_file(&path)
        }
        .map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to clear extension payload '{}': {error}",
                path.display()
            ))
        })?;
    }
    Ok(())
}

fn remote_url(remote: &gix::Remote<'_>) -> Result<String, DomainError> {
    let url = remote
        .url(gix::remote::Direction::Fetch)
        .ok_or_else(|| {
            DomainError::InvalidData("Embedded Git remote has no fetch URL".to_string())
        })?
        .to_bstring();
    let url = url.to_str().map_err(|_| {
        DomainError::InvalidData("Embedded Git remote URL is not UTF-8".to_string())
    })?;
    Ok(parse_remote_url(url)?.url)
}

fn utf8_ref(name: &BStr) -> Result<String, DomainError> {
    name.to_str()
        .map(str::to_owned)
        .map_err(|_| DomainError::InvalidData("Embedded Git ref name is not UTF-8".to_string()))
}

fn update_config(
    repo: &gix::Repository,
    update: impl FnOnce(&mut gix::config::File<'static>) -> Result<(), DomainError>,
) -> Result<(), DomainError> {
    let path = repo.git_dir().join("config");
    let mut config =
        gix::config::File::from_path_no_includes(path.clone(), gix::config::Source::Local)
            .map_err(|error| git_error("Failed to read embedded Git config", error))?;
    update(&mut config)?;

    let mut lock = gix::lock::File::acquire_to_update_resource(
        &path,
        gix::lock::acquire::Fail::Immediately,
        None,
    )
    .map_err(|error| git_error("Failed to lock embedded Git config", error))?;
    config
        .write_to(&mut lock)
        .map_err(|error| git_error("Failed to write embedded Git config", error))?;
    lock.commit()
        .map_err(|error| git_error("Failed to commit embedded Git config", error))?;
    Ok(())
}

fn set_config(
    config: &mut gix::config::File<'static>,
    section: &str,
    subsection: Option<&str>,
    key: &str,
    value: &str,
) -> Result<(), DomainError> {
    config
        .set_raw_value_by(
            section,
            subsection.map(|value| value.as_bytes().as_bstr()),
            key.to_owned(),
            value.as_bytes().as_bstr(),
        )
        .map_err(|error| git_error("Failed to update embedded Git config", error))?;
    Ok(())
}

fn ref_edit(
    name: &str,
    new: Target,
    expected: PreviousValue,
    message: &str,
) -> Result<RefEdit, DomainError> {
    Ok(RefEdit {
        name: name
            .try_into()
            .map_err(|error| git_error("Invalid embedded Git ref", error))?,
        deref: false,
        change: Change::Update {
            log: LogChange {
                message: message.into(),
                ..Default::default()
            },
            expected,
            new,
        },
    })
}

fn apply_ref_edits(
    repo: &gix::Repository,
    edits: impl IntoIterator<Item = RefEdit>,
) -> Result<(), DomainError> {
    let signature = gix::actor::Signature {
        name: "TauriTavern".into(),
        email: "noreply@tauritavern.local".into(),
        time: gix::date::Time::now_utc(),
    };
    let mut time = gix::date::parse::TimeBuf::default();
    repo.edit_references_as(edits, Some(signature.to_ref(&mut time)))
        .map_err(|error| git_error("Failed to update embedded Git refs", error))?;
    Ok(())
}

fn git_error(context: &str, error: impl std::fmt::Display) -> DomainError {
    DomainError::InternalError(format!("{context}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "tauritavern-gix-{label}-{}",
            uuid::Uuid::new_v4().simple()
        ))
    }

    fn write_commit(
        repo: &gix::Repository,
        mut entries: Vec<gix::objs::tree::Entry>,
    ) -> gix::ObjectId {
        entries.sort();
        let tree = repo
            .write_object(&gix::objs::Tree { entries })
            .expect("write tree")
            .detach();
        let signature = gix::actor::Signature {
            name: "TauriTavern Test".into(),
            email: "test@tauritavern.local".into(),
            time: gix::date::Time::now_utc(),
        };
        repo.write_object(&gix::objs::Commit {
            tree,
            parents: Default::default(),
            author: signature.clone(),
            committer: signature,
            encoding: None,
            message: "fixture".into(),
            extra_headers: Vec::new(),
        })
        .expect("write commit")
        .detach()
    }

    fn blob_entry(
        repo: &gix::Repository,
        name: &str,
        data: &[u8],
        kind: gix::objs::tree::EntryKind,
    ) -> gix::objs::tree::Entry {
        gix::objs::tree::Entry {
            mode: kind.into(),
            filename: name.into(),
            oid: repo.write_blob(data).expect("write blob").detach(),
        }
    }

    #[test]
    fn portable_path_rejects_reserved_and_git_names() {
        assert!(validate_portable_path("src/main.js", false).is_ok());
        assert!(validate_portable_path(".git/config", false).is_err());
        assert!(validate_portable_path("CON/file.js", false).is_err());
        assert!(validate_portable_path("bad./file.js", false).is_err());
        assert!(validate_portable_path(".gitmodules", true).is_err());
    }

    #[test]
    fn tree_prefixes_require_directories() {
        let mut paths = HashMap::new();
        paths.insert("src".to_string(), gix::objs::tree::EntryKind::Tree.into());
        paths.insert(
            "src/main.js".to_string(),
            gix::objs::tree::EntryKind::Blob.into(),
        );
        assert!(validate_tree_prefixes(&paths).is_ok());

        paths.insert("src".to_string(), gix::objs::tree::EntryKind::Blob.into());
        assert!(validate_tree_prefixes(&paths).is_err());
    }

    #[test]
    fn preflight_rejects_missing_and_wrong_kind_payload_objects() {
        let path = temp_path("invalid-payload-object");
        let repo = init_embedded(&path).expect("init repository");
        let manifest = br#"{
            "display_name":"Fixture",
            "version":"1.0.0",
            "author":"TauriTavern"
        }"#;

        let missing = gix::ObjectId::from_hex(b"1111111111111111111111111111111111111111")
            .expect("missing object id");
        let missing_commit = write_commit(
            &repo,
            vec![
                blob_entry(
                    &repo,
                    MANIFEST_PATH,
                    manifest,
                    gix::objs::tree::EntryKind::Blob,
                ),
                gix::objs::tree::Entry {
                    mode: gix::objs::tree::EntryKind::Blob.into(),
                    filename: "payload.txt".into(),
                    oid: missing,
                },
            ],
        );
        assert!(prepare_candidate(&repo, missing_commit).is_err());

        let tree = repo
            .write_object(&gix::objs::Tree {
                entries: Vec::new(),
            })
            .expect("write wrong-kind tree")
            .detach();
        let wrong_kind_commit = write_commit(
            &repo,
            vec![
                blob_entry(
                    &repo,
                    MANIFEST_PATH,
                    manifest,
                    gix::objs::tree::EntryKind::Blob,
                ),
                gix::objs::tree::Entry {
                    mode: gix::objs::tree::EntryKind::Blob.into(),
                    filename: "payload.txt".into(),
                    oid: tree,
                },
            ],
        );
        assert!(prepare_candidate(&repo, wrong_kind_commit).is_err());

        drop(repo);
        fs::remove_dir_all(path).expect("remove repository");
    }

    #[test]
    fn materializes_candidate_and_round_trips_standard_branch_state() {
        let path = temp_path("roundtrip");
        let repo = init_embedded(&path).expect("init repository");
        fs::write(path.join("stale.txt"), "stale").expect("write stale payload");

        let manifest = br#"{
            "display_name":"Fixture",
            "version":"1.2.3",
            "author":"TauriTavern"
        }"#;
        let missing_gitlink =
            gix::ObjectId::from_hex(b"1111111111111111111111111111111111111111").unwrap();
        let commit = write_commit(
            &repo,
            vec![
                blob_entry(
                    &repo,
                    "manifest.json",
                    manifest,
                    gix::objs::tree::EntryKind::Blob,
                ),
                blob_entry(
                    &repo,
                    "target.txt",
                    b"target",
                    gix::objs::tree::EntryKind::Blob,
                ),
                blob_entry(
                    &repo,
                    "shortcut",
                    b"target.txt",
                    gix::objs::tree::EntryKind::Link,
                ),
                gix::objs::tree::Entry {
                    mode: gix::objs::tree::EntryKind::Commit.into(),
                    filename: "submodule".into(),
                    oid: missing_gitlink,
                },
            ],
        );
        let selected = ManagedRef::Branch {
            local_ref: "refs/heads/feature/mobile".to_string(),
            remote_ref: "refs/heads/feature/mobile".to_string(),
            tracking_ref: "refs/remotes/origin/feature/mobile".to_string(),
            display_name: "feature/mobile".to_string(),
        };

        create_tracking_ref(&repo, selected.fetch_destination(), commit).unwrap();
        let mut prepared = prepare_candidate(&repo, commit).expect("preflight candidate");
        assert_eq!(prepared.manifest.display_name, "Fixture");
        materialize_candidate(&repo, &path, &mut prepared).expect("materialize candidate");
        configure_install(
            &repo,
            "https://git.example.test/group/fixture.git",
            &selected,
        )
        .expect("configure repository");
        finalize_install_ref(&repo, &selected, commit).expect("finalize repository state");

        assert!(!path.join("stale.txt").exists());
        assert_eq!(fs::read(path.join("shortcut")).unwrap(), b"target.txt");
        assert!(
            fs::symlink_metadata(path.join("shortcut"))
                .unwrap()
                .file_type()
                .is_file()
        );
        assert!(path.join("submodule").is_dir());
        drop(prepared);
        drop(repo);

        let reopened = super::super::git_remote::open_embedded(&path).unwrap();
        let state = read_managed_state(&reopened).expect("read managed state");
        assert_eq!(state.deployed, commit);
        assert_eq!(state.selected, selected);
        assert_eq!(
            state.remote_url,
            "https://git.example.test/group/fixture.git"
        );
        assert!(
            fs::read_to_string(path.join(".git/config"))
                .unwrap()
                .contains("symlinks = false")
        );

        fs::remove_dir_all(path).expect("cleanup fixture");
    }

    #[test]
    fn preflight_rejects_portable_collisions_before_payload_changes() {
        let path = temp_path("collision");
        let repo = init_embedded(&path).expect("init repository");
        fs::write(path.join("keep.txt"), "keep").expect("write payload sentinel");
        let commit = write_commit(
            &repo,
            vec![
                blob_entry(
                    &repo,
                    "manifest.json",
                    br#"{"display_name":"Fixture","version":"1","author":"Test"}"#,
                    gix::objs::tree::EntryKind::Blob,
                ),
                blob_entry(&repo, "Readme.md", b"one", gix::objs::tree::EntryKind::Blob),
                blob_entry(&repo, "README.md", b"two", gix::objs::tree::EntryKind::Blob),
            ],
        );

        assert!(prepare_candidate(&repo, commit).is_err());
        assert_eq!(fs::read_to_string(path.join("keep.txt")).unwrap(), "keep");

        drop(repo);
        fs::remove_dir_all(path).expect("cleanup fixture");
    }
}
