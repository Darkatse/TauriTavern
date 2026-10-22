use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::fs;
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

use super::FileAgentRepository;
use super::fs_tree::{copy_directory_contents, scan_workspace_tree};
use super::paths::validate_workspace_root_path;
use tt_adapter_storage_core::file_system::persist_json_file;
use tt_domain::errors::DomainError;
use tt_domain::models::agent::{
    AgentRun, WorkspaceManifest, WorkspacePersistentChange, WorkspacePersistentChangeSet,
    WorkspaceRootCommit, WorkspaceRootMount, WorkspaceRootScope,
};
use tt_ports::repositories::agent_run_repository::AgentRunRepository;
use tt_ports::repositories::workspace_repository::WorkspaceRepository;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PersistentSnapshot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    base_state_id: Option<String>,
    files: Vec<PersistentSnapshotFile>,
    /// Missing only in historical snapshots; an empty list is a known empty tree.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    directories: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(super) struct PersistentSnapshotFile {
    pub(super) path: String,
    pub(super) sha256: String,
    pub(super) bytes: u64,
}

#[derive(Default, PartialEq, Eq)]
pub(super) struct PersistentTree {
    pub(super) files: Vec<PersistentSnapshotFile>,
    pub(super) directories: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PersistentStateManifest {
    version: u32,
    state_id: String,
    run_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    base_state_id: Option<String>,
    created_at: DateTime<Utc>,
    files: Vec<PersistentSnapshotFile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    directories: Option<Vec<String>>,
    changes: Vec<WorkspacePersistentChange>,
}

impl FileAgentRepository {
    pub(super) async fn initialize_projected_roots(
        &self,
        run: &AgentRun,
        manifest: &WorkspaceManifest,
        run_dir: &Path,
    ) -> Result<PersistentSnapshot, DomainError> {
        let chat = run.chat_target()?;
        let base_state = match chat.persist_base_state_id.as_deref() {
            Some(state_id) => {
                let dir = self.persistent_state_dir(&run.workspace_id, state_id)?;
                let state = self.read_persistent_state_manifest(&dir, state_id).await?;
                Some((dir, state))
            }
            None => None,
        };
        let roots = persistent_roots(manifest)?;
        for root in &roots {
            let run_root = run_dir.join(root);
            fs::create_dir_all(&run_root)
                .await
                .map_err(|e| DomainError::file_io("initialize persist", root, e))?;
            if let Some((base_dir, state)) = &base_state {
                if !state_root_exists(base_dir, root, state).await? {
                    continue;
                }
                copy_directory_contents(&base_dir.join(root), &run_root).await?;
            }
        }
        let tree = scan_tree(run_dir, &roots).await?;
        Ok(PersistentSnapshot {
            base_state_id: chat.persist_base_state_id.clone(),
            files: tree.files,
            directories: Some(tree.directories),
        })
    }

    /// The caller holds persist_lock and the Run read lock through this operation.
    pub(super) async fn publish_persistent_state(
        &self,
        run_id: &str,
        previous_state_id: Option<&str>,
    ) -> Result<WorkspacePersistentChangeSet, DomainError> {
        let run = self.load_run(run_id).await?;
        run.chat_target()?;
        let run_dir = self.run_dir(&run)?;
        let roots = persistent_roots(&self.read_manifest(run_id).await?)?;
        let snapshot: PersistentSnapshot =
            Self::read_json(&run_dir.join("input/persist_snapshot.json")).await?;
        let directories = match snapshot.directories {
            Some(directories) => directories,
            None => match snapshot.base_state_id.as_deref() {
                Some(id) => {
                    let dir = self.persistent_state_dir(&run.workspace_id, id)?;
                    let state = self.read_persistent_state_manifest(&dir, id).await?;
                    state_directories(&dir, &state, &roots).await?
                }
                None => Vec::new(),
            },
        };
        let base = PersistentTree {
            files: snapshot.files,
            directories,
        };
        let current = scan_tree(&run_dir, &roots).await?;
        if let Some(id) = previous_state_id {
            let dir = self.persistent_state_dir(&run.workspace_id, id)?;
            let previous = self.read_persistent_state_manifest(&dir, id).await?;
            if snapshot.base_state_id == previous.base_state_id
                && current.files == previous.files
                && current.directories == state_directories(&dir, &previous, &roots).await?
            {
                // A reused ID retains its original summary, including v1 summaries
                // that did not describe directories.
                return Ok(WorkspacePersistentChangeSet {
                    state_id: previous.state_id,
                    base_state_id: previous.base_state_id,
                    changes: previous.changes,
                });
            }
        }
        let result = WorkspacePersistentChangeSet {
            state_id: Uuid::new_v4().to_string(),
            base_state_id: snapshot.base_state_id,
            changes: persistent_changes(&base, &current),
        };
        let state_dir = self.persistent_state_dir(&run.workspace_id, &result.state_id)?;
        let states_dir = state_dir.parent().expect("state directory has a parent");
        fs::create_dir_all(states_dir)
            .await
            .map_err(|e| DomainError::file_io("create persistent states", run_id, e))?;
        let temp = states_dir.join(format!(
            ".{}.tmp-{}",
            result.state_id,
            Uuid::new_v4().simple()
        ));
        fs::create_dir(&temp)
            .await
            .map_err(|e| DomainError::file_io("stage persist", run_id, e))?;
        let publication = async {
            copy_tree(&run_dir, &temp, &roots, &current).await?;
            let manifest = PersistentStateManifest {
                version: 2,
                state_id: result.state_id.clone(),
                run_id: run.id,
                base_state_id: result.base_state_id.clone(),
                created_at: Utc::now(),
                files: current.files,
                directories: Some(current.directories),
                changes: result.changes.clone(),
            };
            persist_json_file(&temp.join("manifest.json"), &manifest).await?;
            fs::rename(&temp, &state_dir)
                .await
                .map_err(|e| DomainError::file_io("publish persist", &result.state_id, e))
        }
        .await;
        if let Err(error) = publication {
            let _ = fs::remove_dir_all(&temp).await;
            return Err(error);
        }
        Ok(result)
    }

    pub(super) async fn read_persistent_state_manifest(
        &self,
        dir: &Path,
        state_id: &str,
    ) -> Result<PersistentStateManifest, DomainError> {
        let metadata = fs::symlink_metadata(dir).await.map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                DomainError::NotFound(format!(
                    "agent.persistent_state_not_found: {}",
                    dir.display()
                ))
            } else {
                DomainError::file_io("read persistent state", state_id, error)
            }
        })?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(DomainError::InvalidData(format!(
                "agent.persistent_state_invalid: {}",
                dir.display()
            )));
        }
        let manifest: PersistentStateManifest = Self::read_json(&dir.join("manifest.json")).await?;
        if !matches!(manifest.version, 1 | 2) {
            return Err(DomainError::InvalidData(format!(
                "agent.persistent_state_version_unsupported: {}",
                manifest.version
            )));
        }
        if manifest.state_id != state_id {
            return Err(DomainError::InvalidData(format!(
                "agent.persistent_state_manifest_mismatch: {} != {state_id}",
                manifest.state_id
            )));
        }
        if manifest.version == 2 && manifest.directories.is_none() {
            return Err(DomainError::InvalidData(format!(
                "Persistent state {state_id} is missing directories"
            )));
        }
        Ok(manifest)
    }
}

async fn scan_tree(dir: &Path, roots: &[String]) -> Result<PersistentTree, DomainError> {
    let mut tree = PersistentTree::default();
    for root in roots {
        let entries = scan_workspace_tree(&dir.join(root), root, true).await?;
        tree.files.extend(entries.files);
        tree.directories.extend(entries.directories);
    }
    tree.files.sort_by(|a, b| a.path.cmp(&b.path));
    tree.directories.sort();
    Ok(tree)
}

async fn state_directories(
    dir: &Path,
    state: &PersistentStateManifest,
    roots: &[String],
) -> Result<Vec<String>, DomainError> {
    if let Some(directories) = &state.directories {
        return Ok(directories.clone());
    }
    let mut directories = Vec::new();
    for root in roots {
        if state_root_exists(dir, root, state).await? {
            directories.extend(
                scan_workspace_tree(&dir.join(root), root, false)
                    .await?
                    .directories,
            );
        }
    }
    directories.sort();
    Ok(directories)
}

async fn state_root_exists(
    dir: &Path,
    root: &str,
    state: &PersistentStateManifest,
) -> Result<bool, DomainError> {
    match fs::symlink_metadata(dir.join(root)).await {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => Ok(true),
        Ok(_) => Err(DomainError::InvalidData(format!(
            "agent.persistent_state_root_invalid: {root}"
        ))),
        Err(error)
            if error.kind() == std::io::ErrorKind::NotFound
                && state.version == 1
                && !state
                    .files
                    .iter()
                    .any(|file| file.path.starts_with(&format!("{root}/"))) =>
        {
            Ok(false)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Err(DomainError::InvalidData(format!(
                "agent.persistent_state_root_missing: state `{}` is missing root `{root}`",
                state.state_id
            )))
        }
        Err(error) => Err(DomainError::file_io("read persistent root", root, error)),
    }
}

fn persistent_changes(
    base: &PersistentTree,
    current: &PersistentTree,
) -> Vec<WorkspacePersistentChange> {
    use WorkspacePersistentChange as Change;
    let old: BTreeMap<_, _> = base
        .files
        .iter()
        .map(|file| (file.path.as_str(), file))
        .collect();
    let new: BTreeMap<_, _> = current
        .files
        .iter()
        .map(|file| (file.path.as_str(), file))
        .collect();
    let old_dirs: BTreeSet<_> = base.directories.iter().collect();
    let new_dirs: BTreeSet<_> = current.directories.iter().collect();
    let mut changes = Vec::new();
    for path in old.keys().filter(|path| !new.contains_key(**path)) {
        changes.push(Change::Deleted {
            path: (*path).to_string(),
        });
    }
    for path in old_dirs.difference(&new_dirs) {
        changes.push(Change::DirectoryDeleted {
            path: (*path).clone(),
        });
    }
    for file in &current.files {
        match old.get(file.path.as_str()) {
            Some(previous) if *previous == file => {}
            Some(_) => changes.push(Change::Modified {
                path: file.path.clone(),
                sha256: file.sha256.clone(),
                bytes: file.bytes,
            }),
            None => changes.push(Change::Added {
                path: file.path.clone(),
                sha256: file.sha256.clone(),
                bytes: file.bytes,
            }),
        }
    }
    for path in new_dirs.difference(&old_dirs) {
        changes.push(Change::DirectoryAdded {
            path: (*path).clone(),
        });
    }
    // Stable sorting keeps deletion before addition when a node changes type.
    changes.sort_by(|a, b| a.path().cmp(b.path()));
    changes
}

async fn copy_tree(
    source: &Path,
    target: &Path,
    roots: &[String],
    tree: &PersistentTree,
) -> Result<(), DomainError> {
    for directory in roots.iter().chain(&tree.directories) {
        fs::create_dir_all(target.join(directory))
            .await
            .map_err(|e| DomainError::file_io("copy persistent directory", directory, e))?;
    }
    for entry in &tree.files {
        let mut reader = fs::File::open(source.join(&entry.path))
            .await
            .map_err(|e| DomainError::file_io("copy persistent file", &entry.path, e))?;
        let mut writer = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(target.join(&entry.path))
            .await
            .map_err(|e| DomainError::file_io("copy persistent file", &entry.path, e))?;
        tokio::io::copy(&mut reader, &mut writer)
            .await
            .map_err(|e| DomainError::file_io("copy persistent file", &entry.path, e))?;
        writer
            .flush()
            .await
            .map_err(|e| DomainError::file_io("flush persistent file", &entry.path, e))?;
        writer
            .sync_all()
            .await
            .map_err(|e| DomainError::file_io("sync persistent file", &entry.path, e))?;
    }
    Ok(())
}

pub(super) fn persistent_roots(manifest: &WorkspaceManifest) -> Result<Vec<String>, DomainError> {
    let mut roots = Vec::new();
    for root in &manifest.roots {
        if root.lifecycle != tt_domain::models::agent::WorkspaceRootLifecycle::Persistent {
            continue;
        }
        if root.scope != WorkspaceRootScope::Chat
            || root.mount != WorkspaceRootMount::ProjectedOverlay
            || root.commit != WorkspaceRootCommit::OnRunCompleted
        {
            return Err(DomainError::InvalidData(format!(
                "Unsupported persistent workspace root `{}`",
                root.path
            )));
        }
        roots.push(validate_workspace_root_path(&root.path)?);
    }
    Ok(roots)
}
