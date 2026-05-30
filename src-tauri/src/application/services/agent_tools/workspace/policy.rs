use crate::application::errors::ApplicationError;
use crate::domain::models::agent::{WorkspaceManifest, WorkspacePath};
use crate::domain::repositories::workspace_repository::WorkspaceRepository;

#[derive(Debug, Clone)]
pub(super) struct WorkspaceAccessPolicy {
    pub(super) visible_roots: Vec<String>,
    pub(super) writable_roots: Vec<String>,
}

impl WorkspaceAccessPolicy {
    pub(super) fn from_manifest(manifest: &WorkspaceManifest) -> Result<Self, ApplicationError> {
        let mut visible_roots = Vec::new();
        let mut writable_roots = Vec::new();

        for root in &manifest.roots {
            let path = WorkspacePath::parse(&root.path)?;
            if path.as_str().contains('/') {
                return Err(ApplicationError::ValidationError(format!(
                    "agent.invalid_workspace_root: workspace root `{}` must be a single path segment",
                    path.as_str()
                )));
            }
            if root.visible {
                visible_roots.push(path.as_str().to_string());
            }
            if root.writable {
                writable_roots.push(path.as_str().to_string());
            }
        }

        Ok(Self {
            visible_roots,
            writable_roots,
        })
    }

    pub(super) fn ensure_visible(&self, path: &WorkspacePath) -> Result<(), ApplicationError> {
        if self.is_visible(path) {
            return Ok(());
        }

        let value = path.as_str();
        Err(ApplicationError::PermissionDenied(format!(
            "agent.workspace_read_denied: path `{value}` is not visible in the current workspace policy"
        )))
    }

    pub(super) fn ensure_writable(&self, path: &WorkspacePath) -> Result<(), ApplicationError> {
        if self.is_writable(path) {
            return Ok(());
        }

        let value = path.as_str();
        Err(ApplicationError::PermissionDenied(format!(
            "agent.workspace_write_denied: path `{value}` is not writable in the current workspace policy"
        )))
    }

    pub(super) fn is_visible(&self, path: &WorkspacePath) -> bool {
        self.visible_roots
            .iter()
            .any(|root| path_matches_root_or_child(path.as_str(), root.as_str()))
    }

    pub(super) fn is_writable(&self, path: &WorkspacePath) -> bool {
        self.writable_roots
            .iter()
            .any(|root| path_matches_child(path.as_str(), root.as_str()))
    }
}

pub(super) async fn workspace_access_policy(
    workspace_repository: &dyn WorkspaceRepository,
    run_id: &str,
) -> Result<WorkspaceAccessPolicy, ApplicationError> {
    let manifest = workspace_repository.read_manifest(run_id).await?;
    WorkspaceAccessPolicy::from_manifest(&manifest)
}

fn path_matches_root_or_child(path: &str, root: &str) -> bool {
    path == root || path_matches_child(path, root)
}

fn path_matches_child(path: &str, root: &str) -> bool {
    path.len() > root.len()
        && path.starts_with(root)
        && path.as_bytes().get(root.len()) == Some(&b'/')
}
