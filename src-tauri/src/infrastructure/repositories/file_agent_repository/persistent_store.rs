use std::collections::BTreeMap;
use std::path::Path;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::fs;
use uuid::Uuid;

use super::FileAgentRepository;
use super::fs_tree::{copy_directory_contents, scan_workspace_files, snapshot_map};
use super::paths::validate_workspace_root_path;
use crate::domain::errors::DomainError;
use crate::domain::models::agent::{
    AgentRun, WorkspaceManifest, WorkspacePersistentChange, WorkspacePersistentChangeKind,
    WorkspacePersistentChangeSet, WorkspaceRootCommit, WorkspaceRootMount, WorkspaceRootScope,
};
use crate::domain::repositories::agent_run_repository::AgentRunRepository;
use crate::domain::repositories::workspace_repository::WorkspaceRepository;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PersistentSnapshot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) base_state_id: Option<String>,
    pub(super) files: Vec<PersistentSnapshotFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(super) struct PersistentSnapshotFile {
    pub(super) path: String,
    pub(super) sha256: String,
    pub(super) bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistentStateManifest {
    version: u32,
    state_id: String,
    run_id: String,
    workspace_id: String,
    stable_chat_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    base_state_id: Option<String>,
    created_at: DateTime<Utc>,
    files: Vec<PersistentSnapshotFile>,
    changes: Vec<WorkspacePersistentChange>,
}

impl FileAgentRepository {
    pub(super) async fn initialize_projected_roots(
        &self,
        run: &AgentRun,
        manifest: &WorkspaceManifest,
        run_dir: &Path,
    ) -> Result<PersistentSnapshot, DomainError> {
        let chat_dir = self.chat_dir(&run.workspace_id)?;
        fs::create_dir_all(&chat_dir).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to create agent chat workspace {}: {}",
                chat_dir.display(),
                error
            ))
        })?;

        let base_state = match run.persist_base_state_id.as_deref() {
            Some(state_id) => {
                let state_dir = self.persistent_state_dir(&run.workspace_id, state_id)?;
                let state_manifest = self.read_persistent_state_manifest(&state_dir).await?;
                if state_manifest.state_id != state_id {
                    return Err(DomainError::InvalidData(format!(
                        "agent.persistent_state_manifest_mismatch: manifest state `{}` does not match requested state `{state_id}`",
                        state_manifest.state_id
                    )));
                }
                if state_manifest.workspace_id != run.workspace_id {
                    return Err(DomainError::InvalidData(format!(
                        "agent.persistent_state_workspace_mismatch: state `{state_id}` belongs to workspace `{}`",
                        state_manifest.workspace_id
                    )));
                }
                Some((state_dir, state_manifest))
            }
            None => None,
        };

        let mut files = Vec::new();
        for root in persistent_roots(manifest)? {
            let run_root = run_dir.join(&root);
            fs::create_dir_all(&run_root).await.map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to create projected persistent root {}: {}",
                    run_root.display(),
                    error
                ))
            })?;
            if let Some((base_state_dir, base_state_manifest)) = base_state.as_ref() {
                let base_root = base_state_dir.join(&root);
                let metadata = match fs::symlink_metadata(&base_root).await {
                    Ok(metadata) => metadata,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        if !persistent_manifest_has_files_for_root(base_state_manifest, &root) {
                            continue;
                        }
                        return Err(DomainError::InvalidData(format!(
                            "agent.persistent_state_root_missing: state `{}` is missing root `{root}`",
                            run.persist_base_state_id.as_deref().unwrap_or_default()
                        )));
                    }
                    Err(error) => {
                        return Err(DomainError::InternalError(format!(
                            "Failed to inspect persistent state root {}: {}",
                            base_root.display(),
                            error
                        )));
                    }
                };
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    return Err(DomainError::InvalidData(format!(
                        "agent.persistent_state_root_invalid: {}",
                        base_root.display()
                    )));
                }
                copy_directory_contents(&base_root, &run_root).await?;
            }
            files.extend(scan_workspace_files(&run_root, &root).await?);
        }

        files.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(PersistentSnapshot {
            base_state_id: run.persist_base_state_id.clone(),
            files,
        })
    }

    pub(super) async fn compute_persistent_changes(
        &self,
        run_id: &str,
    ) -> Result<WorkspacePersistentChangeSet, DomainError> {
        let run = self.load_run(run_id).await?;
        let manifest = self.read_manifest(run_id).await?;
        let roots = persistent_roots(&manifest)?;
        let run_dir = self.run_dir(&run)?;
        let base_snapshot: PersistentSnapshot =
            Self::read_json(&run_dir.join("input").join("persist_snapshot.json")).await?;

        let base = snapshot_map(base_snapshot.files);
        let mut overlay = BTreeMap::new();

        for root in roots {
            overlay.extend(snapshot_map(
                scan_workspace_files(&run_dir.join(&root), &root).await?,
            ));
        }

        let mut changes = Vec::new();
        for (path, overlay_file) in &overlay {
            match base.get(path) {
                Some(base_file) if base_file.sha256 == overlay_file.sha256 => {}
                Some(_) => {
                    changes.push(WorkspacePersistentChange {
                        path: path.clone(),
                        kind: WorkspacePersistentChangeKind::Modified,
                        sha256: overlay_file.sha256.clone(),
                        bytes: overlay_file.bytes,
                    });
                }
                None => {
                    changes.push(WorkspacePersistentChange {
                        path: path.clone(),
                        kind: WorkspacePersistentChangeKind::Added,
                        sha256: overlay_file.sha256.clone(),
                        bytes: overlay_file.bytes,
                    });
                }
            }
        }

        for path in base.keys() {
            if !overlay.contains_key(path) {
                return Err(DomainError::InvalidData(format!(
                    "agent.persistent_delete_unsupported: persistent file `{path}` is missing from the run projection"
                )));
            }
        }

        changes.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(WorkspacePersistentChangeSet {
            state_id: run.id,
            base_state_id: base_snapshot.base_state_id,
            changes,
        })
    }

    pub(super) async fn commit_persistent_state(
        &self,
        run_id: &str,
        changes: WorkspacePersistentChangeSet,
    ) -> Result<WorkspacePersistentChangeSet, DomainError> {
        let run = self.load_run(run_id).await?;
        let manifest = self.read_manifest(run_id).await?;
        let roots = persistent_roots(&manifest)?;
        let run_dir = self.run_dir(&run)?;
        let state_dir = self.persistent_state_dir(&run.workspace_id, &changes.state_id)?;
        match fs::symlink_metadata(&state_dir).await {
            Ok(_) => {
                return Err(DomainError::InvalidData(format!(
                    "agent.persistent_state_exists: state `{}` already exists",
                    changes.state_id
                )));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(DomainError::InternalError(format!(
                    "Failed to inspect persistent state {}: {}",
                    state_dir.display(),
                    error
                )));
            }
        }

        let states_dir = state_dir.parent().ok_or_else(|| {
            DomainError::InternalError(format!(
                "Persistent state path has no parent: {}",
                state_dir.display()
            ))
        })?;
        fs::create_dir_all(states_dir).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to create persistent states directory {}: {}",
                states_dir.display(),
                error
            ))
        })?;

        let temp_dir = states_dir.join(format!(
            ".{}.tmp-{}",
            changes.state_id,
            Uuid::new_v4().simple()
        ));
        fs::create_dir(&temp_dir).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to create persistent state temp directory {}: {}",
                temp_dir.display(),
                error
            ))
        })?;

        let commit_result = async {
            let mut files = Vec::new();
            for root in roots {
                let source_root = run_dir.join(&root);
                let target_root = temp_dir.join(&root);
                fs::create_dir_all(&target_root).await.map_err(|error| {
                    DomainError::InternalError(format!(
                        "Failed to create persistent state root {}: {}",
                        target_root.display(),
                        error
                    ))
                })?;
                copy_directory_contents(&source_root, &target_root).await?;
                files.extend(scan_workspace_files(&target_root, &root).await?);
            }
            files.sort_by(|a, b| a.path.cmp(&b.path));

            let state_manifest = PersistentStateManifest {
                version: 1,
                state_id: changes.state_id.clone(),
                run_id: run.id.clone(),
                workspace_id: run.workspace_id.clone(),
                stable_chat_id: run.stable_chat_id.clone(),
                base_state_id: changes.base_state_id.clone(),
                created_at: Utc::now(),
                files,
                changes: changes.changes.clone(),
            };
            Self::write_json_atomic(&temp_dir.join("manifest.json"), &state_manifest).await?;
            fs::rename(&temp_dir, &state_dir).await.map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to promote persistent state {} to {}: {}",
                    temp_dir.display(),
                    state_dir.display(),
                    error
                ))
            })?;
            Ok::<(), DomainError>(())
        }
        .await;

        if let Err(error) = commit_result {
            let _ = fs::remove_dir_all(&temp_dir).await;
            return Err(error);
        }

        Ok(changes)
    }

    async fn read_persistent_state_manifest(
        &self,
        state_dir: &Path,
    ) -> Result<PersistentStateManifest, DomainError> {
        let metadata = fs::symlink_metadata(state_dir).await.map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                DomainError::NotFound(format!(
                    "agent.persistent_state_not_found: {}",
                    state_dir.display()
                ))
            } else {
                DomainError::InternalError(format!(
                    "Failed to inspect persistent state {}: {}",
                    state_dir.display(),
                    error
                ))
            }
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(DomainError::InvalidData(format!(
                "agent.persistent_state_invalid: {}",
                state_dir.display()
            )));
        }

        let manifest: PersistentStateManifest =
            Self::read_json(&state_dir.join("manifest.json")).await?;
        if manifest.version != 1 {
            return Err(DomainError::InvalidData(format!(
                "agent.persistent_state_version_unsupported: {}",
                manifest.version
            )));
        }
        Ok(manifest)
    }
}

pub(super) fn persistent_roots(manifest: &WorkspaceManifest) -> Result<Vec<String>, DomainError> {
    let mut roots = Vec::new();
    for root in &manifest.roots {
        if root.lifecycle != crate::domain::models::agent::WorkspaceRootLifecycle::Persistent {
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

fn persistent_manifest_has_files_for_root(manifest: &PersistentStateManifest, root: &str) -> bool {
    let prefix = format!("{root}/");
    manifest
        .files
        .iter()
        .any(|file| file.path.starts_with(&prefix))
}
