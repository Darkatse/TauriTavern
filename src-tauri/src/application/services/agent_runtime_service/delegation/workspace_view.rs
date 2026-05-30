use async_trait::async_trait;
use serde_json::Value;

use crate::application::errors::ApplicationError;
use crate::application::services::agent_runtime_service::AgentRuntimeService;
use crate::application::services::agent_tools::{AgentToolDispatchOutcome, AgentToolEffect};
use crate::application::services::agent_workspace_scope::{ChildWorkspaceScope, SUMMARY_ROOT};
use crate::domain::errors::DomainError;
use crate::domain::models::agent::profile::ResolvedAgentProfile;
use crate::domain::models::agent::{
    AgentRun, AgentTaskRecord, AgentToolCall, WorkspaceManifest, WorkspacePath,
    WorkspacePersistentChangeSet,
};
use crate::domain::repositories::workspace_repository::{
    WorkspaceAppendResult, WorkspaceEntry, WorkspaceEntryKind, WorkspaceFile, WorkspaceFileList,
    WorkspaceRepository, WorkspaceWriteGuard,
};

const SUMMARY_PARENT_ROOT: &str = "summaries/parent";

#[derive(Debug, Clone)]
pub(in crate::application::services::agent_runtime_service) struct ChildWorkspaceView {
    scope: ChildWorkspaceScope,
}

impl ChildWorkspaceView {
    pub(super) fn new(workspace_key: String, profile: &ResolvedAgentProfile) -> Self {
        Self {
            scope: ChildWorkspaceScope::for_profile(workspace_key, profile),
        }
    }

    pub(super) fn summary_result_path(&self) -> Result<WorkspacePath, ApplicationError> {
        self.scope
            .summary_result_path()
            .map_err(ApplicationError::from)
    }

    pub(super) fn parent_visible_path(&self, raw: &str) -> Result<String, String> {
        let path = WorkspacePath::parse(raw).map_err(|error| error.to_string())?;
        self.scope
            .model_to_physical_path(&path)
            .map(|path| path.as_str().to_string())
            .map_err(|error| error.to_string())
    }

    pub(in crate::application::services::agent_runtime_service) fn repository<'a>(
        &'a self,
        inner: &'a dyn WorkspaceRepository,
    ) -> ChildWorkspaceRepository<'a> {
        ChildWorkspaceRepository { inner, view: self }
    }

    pub(in crate::application::services::agent_runtime_service) fn write_denial_message(
        &self,
        call: &AgentToolCall,
    ) -> Option<String> {
        if call.name != "workspace.write_file" && call.name != "workspace.apply_patch" {
            return None;
        }
        let path = call
            .arguments
            .as_object()
            .and_then(|args| args.get("path"))
            .and_then(Value::as_str)?;
        let path = WorkspacePath::parse(path).ok()?;
        self.scope
            .model_to_physical_write_path(&path)
            .err()
            .map(|error| match error {
                DomainError::InvalidData(message) => message,
                other => other.to_string(),
            })
    }

    pub(in crate::application::services::agent_runtime_service) fn physicalize_outcome_effect(
        &self,
        mut outcome: AgentToolDispatchOutcome,
    ) -> Result<AgentToolDispatchOutcome, ApplicationError> {
        match &mut outcome.effect {
            AgentToolEffect::WorkspaceFileWritten { file, .. }
            | AgentToolEffect::WorkspaceFilePatched { file, .. } => {
                file.path = self.scope.model_to_physical_path(&file.path)?;
            }
            AgentToolEffect::ChatCommitRequested { path, .. }
            | AgentToolEffect::ChatCommitted { path, .. } => {
                *path = self.scope.model_to_physical_path(path)?;
            }
            AgentToolEffect::None
            | AgentToolEffect::TaskReturned { .. }
            | AgentToolEffect::Finish => {}
        }
        Ok(outcome)
    }

    fn model_file_from_physical(&self, file: WorkspaceFile) -> Result<WorkspaceFile, DomainError> {
        let physical_path = file.path.clone();
        let path = self
            .scope
            .physical_to_model_path(&physical_path)?
            .ok_or_else(|| {
                DomainError::InvalidData(format!(
                    "agent.child_workspace_path_hidden: physical path `{}` has no child-facing path",
                    physical_path.as_str()
                ))
            })?;
        Ok(WorkspaceFile { path, ..file })
    }

    fn recoverable_model_path_error(error: DomainError) -> DomainError {
        match error {
            DomainError::InvalidData(message) if message.starts_with("agent.child_") => {
                DomainError::NotFound(message)
            }
            other => other,
        }
    }

    fn modelize_domain_error(
        &self,
        error: DomainError,
        requested_path: Option<&WorkspacePath>,
        request_kind: &str,
    ) -> DomainError {
        match error {
            DomainError::NotFound(_) => {
                let requested = requested_path.map(WorkspacePath::as_str).unwrap_or(".");
                DomainError::NotFound(format!("Workspace {request_kind} not found: {requested}"))
            }
            DomainError::WorkspacePathIsDirectory { path } => {
                let path = WorkspacePath::parse(&path)
                    .ok()
                    .and_then(|path| self.scope.physical_to_model_path(&path).ok().flatten())
                    .map(|path| path.as_str().to_string())
                    .unwrap_or(path);
                DomainError::WorkspacePathIsDirectory { path }
            }
            DomainError::InvalidData(message) if message.starts_with("agent.child_") => {
                DomainError::NotFound(message)
            }
            other => other,
        }
    }
}

pub(in crate::application::services::agent_runtime_service) struct ChildWorkspaceRepository<'a> {
    inner: &'a dyn WorkspaceRepository,
    view: &'a ChildWorkspaceView,
}

#[async_trait]
impl WorkspaceRepository for ChildWorkspaceRepository<'_> {
    async fn initialize_run(
        &self,
        run: &AgentRun,
        manifest: &WorkspaceManifest,
        prompt_snapshot: &serde_json::Value,
        resolved_profile: &ResolvedAgentProfile,
    ) -> Result<(), DomainError> {
        self.inner
            .initialize_run(run, manifest, prompt_snapshot, resolved_profile)
            .await
    }

    async fn read_manifest(&self, run_id: &str) -> Result<WorkspaceManifest, DomainError> {
        let mut manifest = self.inner.read_manifest(run_id).await?;
        manifest.roots = manifest
            .roots
            .into_iter()
            .map(|root| self.view.scope.child_visible_root(root))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(manifest)
    }

    async fn write_text(
        &self,
        run_id: &str,
        path: &WorkspacePath,
        text: &str,
    ) -> Result<WorkspaceFile, DomainError> {
        let requested_path = path.clone();
        let path = self
            .view
            .scope
            .model_to_physical_write_path(path)
            .map_err(ChildWorkspaceView::recoverable_model_path_error)?;
        let file = self
            .inner
            .write_text(run_id, &path, text)
            .await
            .map_err(|error| {
                self.view
                    .modelize_domain_error(error, Some(&requested_path), "file")
            })?;
        self.view.model_file_from_physical(file)
    }

    async fn write_text_guarded(
        &self,
        run_id: &str,
        path: &WorkspacePath,
        text: &str,
        guard: WorkspaceWriteGuard,
    ) -> Result<WorkspaceFile, DomainError> {
        let requested_path = path.clone();
        let path = self
            .view
            .scope
            .model_to_physical_write_path(path)
            .map_err(ChildWorkspaceView::recoverable_model_path_error)?;
        let file = self
            .inner
            .write_text_guarded(run_id, &path, text, guard)
            .await
            .map_err(|error| {
                self.view
                    .modelize_domain_error(error, Some(&requested_path), "file")
            })?;
        self.view.model_file_from_physical(file)
    }

    async fn append_text(
        &self,
        run_id: &str,
        path: &WorkspacePath,
        text: &str,
    ) -> Result<WorkspaceAppendResult, DomainError> {
        let requested_path = path.clone();
        let path = self
            .view
            .scope
            .model_to_physical_write_path(path)
            .map_err(ChildWorkspaceView::recoverable_model_path_error)?;
        let result = self
            .inner
            .append_text(run_id, &path, text)
            .await
            .map_err(|error| {
                self.view
                    .modelize_domain_error(error, Some(&requested_path), "file")
            })?;
        Ok(WorkspaceAppendResult {
            file: self.view.model_file_from_physical(result.file)?,
            previous_sha256: result.previous_sha256,
        })
    }

    async fn read_text(
        &self,
        run_id: &str,
        path: &WorkspacePath,
    ) -> Result<WorkspaceFile, DomainError> {
        let requested_path = path.clone();
        let path = self
            .view
            .scope
            .model_to_physical_path(path)
            .map_err(ChildWorkspaceView::recoverable_model_path_error)?;
        let file = self.inner.read_text(run_id, &path).await.map_err(|error| {
            self.view
                .modelize_domain_error(error, Some(&requested_path), "file")
        })?;
        self.view.model_file_from_physical(file)
    }

    async fn list_files(
        &self,
        run_id: &str,
        path: Option<&WorkspacePath>,
        depth: usize,
        max_entries: usize,
    ) -> Result<WorkspaceFileList, DomainError> {
        let requested_path = path.cloned();
        let path = path
            .map(|path| self.view.scope.model_to_physical_path(path))
            .transpose()
            .map_err(ChildWorkspaceView::recoverable_model_path_error)?;
        let list = self
            .inner
            .list_files(run_id, path.as_ref(), depth, max_entries)
            .await
            .map_err(|error| {
                self.view
                    .modelize_domain_error(error, requested_path.as_ref(), "path")
            })?;
        self.model_list_from_physical(list, requested_path.as_ref())
    }

    async fn commit_persistent_changes(
        &self,
        run_id: &str,
    ) -> Result<WorkspacePersistentChangeSet, DomainError> {
        self.inner.commit_persistent_changes(run_id).await
    }
}

impl ChildWorkspaceRepository<'_> {
    fn model_list_from_physical(
        &self,
        list: WorkspaceFileList,
        requested_path: Option<&WorkspacePath>,
    ) -> Result<WorkspaceFileList, DomainError> {
        let mut entries = list
            .entries
            .into_iter()
            .filter_map(|entry| {
                self.view
                    .scope
                    .physical_to_model_path_for_list(&entry.path, requested_path)
                    .transpose()
                    .map(|path| path.map(|path| WorkspaceEntry { path, ..entry }))
            })
            .collect::<Result<Vec<_>, _>>()?;

        if entries
            .iter()
            .any(|entry| entry.path.as_str() == SUMMARY_ROOT)
            && !entries
                .iter()
                .any(|entry| entry.path.as_str() == SUMMARY_PARENT_ROOT)
        {
            entries.push(WorkspaceEntry {
                path: WorkspacePath::parse(SUMMARY_PARENT_ROOT)?,
                kind: WorkspaceEntryKind::Directory,
            });
        }

        entries.sort_by(|a, b| {
            let kind_order = match (&a.kind, &b.kind) {
                (WorkspaceEntryKind::Directory, WorkspaceEntryKind::File) => {
                    std::cmp::Ordering::Less
                }
                (WorkspaceEntryKind::File, WorkspaceEntryKind::Directory) => {
                    std::cmp::Ordering::Greater
                }
                _ => std::cmp::Ordering::Equal,
            };
            kind_order.then_with(|| a.path.as_str().cmp(b.path.as_str()))
        });
        entries.dedup_by(|left, right| left.path == right.path && left.kind == right.kind);

        Ok(WorkspaceFileList {
            entries,
            truncated: list.truncated,
        })
    }
}

impl AgentRuntimeService {
    pub(in crate::application::services::agent_runtime_service) async fn task_for_child_invocation(
        &self,
        run_id: &str,
        invocation_id: &str,
    ) -> Result<Option<AgentTaskRecord>, ApplicationError> {
        let mut matches = self
            .invocation_repository
            .list_tasks(run_id)
            .await?
            .into_iter()
            .filter(|task| task.child_invocation_id == invocation_id)
            .collect::<Vec<_>>();
        if matches.len() > 1 {
            return Err(ApplicationError::ValidationError(format!(
                "agent.duplicate_task_record: multiple tasks own child invocation `{invocation_id}`"
            )));
        }
        Ok(matches.pop())
    }

    pub(in crate::application::services::agent_runtime_service) async fn child_workspace_view(
        &self,
        run_id: &str,
        invocation_id: &str,
        profile: &ResolvedAgentProfile,
    ) -> Result<ChildWorkspaceView, ApplicationError> {
        let task = self
            .task_for_child_invocation(run_id, invocation_id)
            .await?
            .ok_or_else(|| {
                ApplicationError::ValidationError(format!(
                    "agent.task_record_missing: no task record owns child invocation `{invocation_id}`"
                ))
            })?;
        Ok(ChildWorkspaceView::new(task.workspace_key, profile))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    #[test]
    fn child_model_paths_map_to_semantic_physical_paths() {
        let view = test_view();

        assert_eq!(
            view.scope
                .model_to_physical_path(&path("summaries/notes.md"))
                .unwrap()
                .as_str(),
            "summaries/agents/scene-critic/notes.md"
        );
        assert_eq!(
            view.scope
                .model_to_physical_path(&path("scratch/draft.md"))
                .unwrap()
                .as_str(),
            "scratch/agents/scene-critic/draft.md"
        );
        assert_eq!(
            view.scope
                .model_to_physical_path(&path("summaries/parent/world.md"))
                .unwrap()
                .as_str(),
            "summaries/world.md"
        );
        assert_eq!(
            view.scope
                .model_to_physical_path(&path("summaries/agents/other-agent/notes.md"))
                .unwrap()
                .as_str(),
            "summaries/agents/other-agent/notes.md"
        );
        assert!(
            view.scope
                .model_to_physical_path(&path("summaries/parent/agents/other-agent/notes.md"))
                .is_err()
        );
        assert!(
            view.scope
                .model_to_physical_path(&path("summaries/agents/scene-critic/notes.md"))
                .is_err()
        );
    }

    #[test]
    fn child_physical_paths_map_back_to_child_view() {
        let view = test_view();

        assert_eq!(
            view.scope
                .physical_to_model_path(&path("summaries/agents/scene-critic/notes.md"))
                .unwrap()
                .unwrap()
                .as_str(),
            "summaries/notes.md"
        );
        assert_eq!(
            view.scope
                .physical_to_model_path(&path("scratch/agents/scene-critic/draft.md"))
                .unwrap()
                .unwrap()
                .as_str(),
            "scratch/draft.md"
        );
        assert_eq!(
            view.scope
                .physical_parent_summary_to_model_path(&path("summaries/world.md"))
                .unwrap()
                .unwrap()
                .as_str(),
            "summaries/parent/world.md"
        );
        assert_eq!(
            view.scope
                .physical_to_model_path(&path("summaries/agents/other-agent/notes.md"))
                .unwrap()
                .unwrap()
                .as_str(),
            "summaries/agents/other-agent/notes.md"
        );
    }

    #[test]
    fn list_views_keep_parent_private_and_other_agent_summaries_separate() {
        let view = test_view();

        assert_eq!(
            view.scope
                .physical_to_model_path_for_list(&path("summaries"), None)
                .unwrap()
                .unwrap()
                .as_str(),
            "summaries"
        );
        assert_eq!(
            view.scope
                .physical_to_model_path_for_list(
                    &path("summaries/world.md"),
                    Some(&path("summaries/parent"))
                )
                .unwrap()
                .unwrap()
                .as_str(),
            "summaries/parent/world.md"
        );
        assert_eq!(
            view.scope
                .physical_to_model_path_for_list(
                    &path("summaries/agents/other-agent/notes.md"),
                    Some(&path("summaries/agents"))
                )
                .unwrap()
                .unwrap()
                .as_str(),
            "summaries/agents/other-agent/notes.md"
        );
        assert!(
            view.scope
                .physical_to_model_path_for_list(
                    &path("summaries/agents/other-agent/notes.md"),
                    Some(&path("summaries/parent"))
                )
                .unwrap()
                .is_none()
        );
        assert!(
            view.scope
                .physical_to_model_path_for_list(
                    &path("summaries/agents/scene-critic/notes.md"),
                    Some(&path("summaries/agents"))
                )
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn root_listing_hides_non_profile_internal_paths() {
        let view = test_view();

        assert!(
            view.scope
                .physical_to_model_path_for_list(&path("input"), None)
                .unwrap()
                .is_none()
        );
        assert!(
            view.scope
                .physical_to_model_path_for_list(&path("model-responses/round-1.json"), None)
                .unwrap()
                .is_none()
        );
        assert!(
            view.scope
                .physical_to_model_path_for_list(&path("agent-results/result.json"), None)
                .unwrap()
                .is_none()
        );
        assert_eq!(
            view.scope
                .physical_to_model_path_for_list(&path("output/main.md"), None)
                .unwrap()
                .unwrap()
                .as_str(),
            "output/main.md"
        );
    }

    #[test]
    fn child_write_policy_accepts_private_notes_and_rejects_parent_summary() {
        let view = test_view();

        assert!(
            view.write_denial_message(&write_call("summaries/notes.md"))
                .is_none()
        );
        assert!(
            view.write_denial_message(&write_call("scratch/notes.md"))
                .is_none()
        );
        assert!(
            view.write_denial_message(&write_call("summaries/parent/world.md"))
                .is_some()
        );
        assert!(
            view.write_denial_message(&write_call("summaries/agents/other-agent/notes.md"))
                .is_some()
        );
        assert!(
            view.write_denial_message(&write_call("output/main.md"))
                .is_none()
        );
        assert!(
            view.write_denial_message(&write_call("persist/story_state.md"))
                .is_none()
        );
        assert!(
            view.write_denial_message(&write_call("plan/outline.md"))
                .is_some()
        );
    }

    #[test]
    fn child_workspace_errors_use_model_facing_paths() {
        let view = test_view();

        let error = view.modelize_domain_error(
            DomainError::NotFound(
                "Workspace file not found: summaries/agents/scene-critic/missing.md".to_string(),
            ),
            Some(&path("summaries/missing.md")),
            "file",
        );
        assert!(matches!(
            error,
            DomainError::NotFound(message)
                if message == "Workspace file not found: summaries/missing.md"
        ));

        let error = ChildWorkspaceView::recoverable_model_path_error(
            view.scope
                .model_to_physical_path(&path("summaries/parent/agents/other-agent/notes.md"))
                .unwrap_err(),
        );
        assert!(matches!(
            error,
            DomainError::NotFound(message)
                if message.contains("use summaries/agents/")
        ));
    }

    fn path(raw: &str) -> WorkspacePath {
        WorkspacePath::parse(raw).unwrap()
    }

    fn test_view() -> ChildWorkspaceView {
        ChildWorkspaceView {
            scope: ChildWorkspaceScope::new(
                "scene-critic".to_string(),
                vec![
                    "output".to_string(),
                    "persist".to_string(),
                    "plan".to_string(),
                ],
                vec!["output".to_string(), "persist".to_string()],
            ),
        }
    }

    fn write_call(path: &str) -> AgentToolCall {
        AgentToolCall {
            id: "call_test".to_string(),
            name: "workspace.write_file".to_string(),
            arguments: json!({ "path": path, "content": "text" }),
            provider_metadata: Value::Null,
        }
    }
}
