use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use tokio::fs;

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
    pub(super) files: Vec<PersistentSnapshotFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(super) struct PersistentSnapshotFile {
    pub(super) path: String,
    pub(super) sha256: String,
    pub(super) bytes: u64,
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

        let mut files = Vec::new();
        for root in persistent_roots(manifest)? {
            let stable_root = chat_dir.join(&root);
            let run_root = run_dir.join(&root);
            fs::create_dir_all(&run_root).await.map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to create projected persistent root {}: {}",
                    run_root.display(),
                    error
                ))
            })?;
            copy_directory_contents(&stable_root, &run_root).await?;
            files.extend(scan_workspace_files(&stable_root, &root).await?);
        }

        files.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(PersistentSnapshot { files })
    }

    pub(super) async fn compute_persistent_changes(
        &self,
        run_id: &str,
    ) -> Result<WorkspacePersistentChangeSet, DomainError> {
        let run = self.load_run(run_id).await?;
        let manifest = self.read_manifest(run_id).await?;
        let roots = persistent_roots(&manifest)?;
        let run_dir = self.run_dir(&run)?;
        let chat_dir = self.chat_dir(&run.workspace_id)?;
        let base_snapshot: PersistentSnapshot =
            Self::read_json(&run_dir.join("input").join("persist_snapshot.json")).await?;

        let base = snapshot_map(base_snapshot.files);
        let mut stable = BTreeMap::new();
        let mut overlay = BTreeMap::new();

        for root in roots {
            stable.extend(snapshot_map(
                scan_workspace_files(&chat_dir.join(&root), &root).await?,
            ));
            overlay.extend(snapshot_map(
                scan_workspace_files(&run_dir.join(&root), &root).await?,
            ));
        }

        let mut changes = Vec::new();
        for (path, overlay_file) in &overlay {
            match base.get(path) {
                Some(base_file) if base_file.sha256 == overlay_file.sha256 => {}
                Some(base_file) => {
                    let stable_file = stable.get(path).ok_or_else(|| {
                        DomainError::InvalidData(format!(
                            "agent.persistent_workspace_conflict: persistent file `{path}` was removed since this run started"
                        ))
                    })?;
                    if stable_file.sha256 != base_file.sha256 {
                        return Err(DomainError::InvalidData(format!(
                            "agent.persistent_workspace_conflict: persistent file `{path}` changed since this run started"
                        )));
                    }
                    changes.push(WorkspacePersistentChange {
                        path: path.clone(),
                        kind: WorkspacePersistentChangeKind::Modified,
                        sha256: overlay_file.sha256.clone(),
                        bytes: overlay_file.bytes,
                    });
                }
                None => {
                    if stable.contains_key(path) {
                        return Err(DomainError::InvalidData(format!(
                            "agent.persistent_workspace_conflict: persistent file `{path}` was created by another run"
                        )));
                    }
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
        Ok(WorkspacePersistentChangeSet { changes })
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
