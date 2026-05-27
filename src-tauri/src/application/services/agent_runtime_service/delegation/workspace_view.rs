use async_trait::async_trait;
use serde_json::Value;

use crate::application::errors::ApplicationError;
use crate::application::services::agent_runtime_service::AgentRuntimeService;
use crate::application::services::agent_tools::{AgentToolDispatchOutcome, AgentToolEffect};
use crate::domain::errors::DomainError;
use crate::domain::models::agent::profile::ResolvedAgentProfile;
use crate::domain::models::agent::{
    AgentRun, AgentTaskRecord, AgentToolCall, WorkspaceManifest, WorkspacePath,
    WorkspacePersistentChangeSet, WorkspaceRootSpec,
};
use crate::domain::repositories::workspace_repository::{
    WorkspaceEntry, WorkspaceEntryKind, WorkspaceFile, WorkspaceFileList, WorkspaceRepository,
};

const SUMMARY_ROOT: &str = "summaries";
const SCRATCH_ROOT: &str = "scratch";
const SUMMARY_AGENT_PREFIX: &str = "summaries/agents/";
const SUMMARY_AGENTS_ROOT: &str = "summaries/agents";
const SCRATCH_AGENT_PREFIX: &str = "scratch/agents/";
const SUMMARY_PARENT_ROOT: &str = "summaries/parent";

#[derive(Debug, Clone)]
pub(in crate::application::services::agent_runtime_service) struct ChildWorkspaceView {
    workspace_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ChildModelWorkspacePath {
    PrivateSummary { suffix: Option<String> },
    PrivateScratch { suffix: Option<String> },
    ParentSummary { suffix: Option<String> },
    AgentSummaries,
    CurrentAgentSummaryAlias,
    ParentAgentsAlias,
    Other,
}

impl ChildWorkspaceView {
    pub(super) fn new(workspace_key: String) -> Self {
        Self { workspace_key }
    }

    pub(super) fn summary_result_path(&self) -> Result<WorkspacePath, ApplicationError> {
        WorkspacePath::parse(format!("summaries/agents/{}/result.md", self.workspace_key))
            .map_err(ApplicationError::from)
    }

    pub(super) fn parent_visible_path(&self, raw: &str) -> Result<String, String> {
        let path = WorkspacePath::parse(raw).map_err(|error| error.to_string())?;
        self.model_to_physical_path(&path)
            .map(|path| path.as_str().to_string())
            .map_err(|error| error.to_string())
    }

    pub(in crate::application::services::agent_runtime_service) fn repository<'a>(
        &'a self,
        inner: &'a dyn WorkspaceRepository,
    ) -> ChildWorkspaceRepository<'a> {
        ChildWorkspaceRepository { inner, view: self }
    }

    pub(in crate::application::services::agent_runtime_service) fn write_is_denied(
        &self,
        call: &AgentToolCall,
    ) -> bool {
        if call.name != "workspace.write_file" && call.name != "workspace.apply_patch" {
            return false;
        }
        let Some(path) = call
            .arguments
            .as_object()
            .and_then(|args| args.get("path"))
            .and_then(Value::as_str)
        else {
            return false;
        };
        let Ok(path) = WorkspacePath::parse(path) else {
            return false;
        };
        !matches!(
            self.classify_model_path(&path),
            ChildModelWorkspacePath::PrivateSummary { suffix: Some(_) }
                | ChildModelWorkspacePath::PrivateScratch { suffix: Some(_) }
        )
    }

    pub(in crate::application::services::agent_runtime_service) fn physicalize_outcome_effect(
        &self,
        mut outcome: AgentToolDispatchOutcome,
    ) -> Result<AgentToolDispatchOutcome, ApplicationError> {
        match &mut outcome.effect {
            AgentToolEffect::WorkspaceFileWritten { file }
            | AgentToolEffect::WorkspaceFilePatched { file, .. } => {
                file.path = self.model_to_physical_path(&file.path)?;
            }
            AgentToolEffect::ChatCommitRequested { path, .. }
            | AgentToolEffect::ChatCommitted { path, .. } => {
                *path = self.model_to_physical_path(path)?;
            }
            AgentToolEffect::None
            | AgentToolEffect::TaskReturned { .. }
            | AgentToolEffect::Finish => {}
        }
        Ok(outcome)
    }

    fn model_to_physical_path(&self, path: &WorkspacePath) -> Result<WorkspacePath, DomainError> {
        match self.classify_model_path(path) {
            ChildModelWorkspacePath::PrivateSummary { suffix } => {
                WorkspacePath::parse(join_child_path(self.private_summary_root(), suffix.as_deref()))
            }
            ChildModelWorkspacePath::PrivateScratch { suffix } => {
                WorkspacePath::parse(join_child_path(self.private_scratch_root(), suffix.as_deref()))
            }
            ChildModelWorkspacePath::ParentSummary { suffix } => {
                WorkspacePath::parse(join_child_path(SUMMARY_ROOT, suffix.as_deref()))
            }
            ChildModelWorkspacePath::AgentSummaries => WorkspacePath::parse(path.as_str()),
            ChildModelWorkspacePath::CurrentAgentSummaryAlias => {
                Err(DomainError::InvalidData(format!(
                        "agent.child_self_summary_alias_denied: use `{SUMMARY_ROOT}/` for this task's private notes, not `{SUMMARY_AGENTS_ROOT}/{}/`",
                        self.workspace_key
                )))
            }
            ChildModelWorkspacePath::ParentAgentsAlias => Err(DomainError::InvalidData(
                "agent.child_parent_agents_path_denied: use summaries/agents/ for other delegated Agent notes".to_string(),
            )),
            ChildModelWorkspacePath::Other => WorkspacePath::parse(path.as_str()),
        }
    }

    fn classify_model_path(&self, path: &WorkspacePath) -> ChildModelWorkspacePath {
        let value = path.as_str();
        if let Some(suffix) = suffix_for_root(value, SUMMARY_ROOT) {
            if let Some(suffix) = suffix.as_deref() {
                if suffix == "agents" || suffix.starts_with("agents/") {
                    return if self.is_current_agent_summary_alias(value) {
                        ChildModelWorkspacePath::CurrentAgentSummaryAlias
                    } else {
                        ChildModelWorkspacePath::AgentSummaries
                    };
                }
                if suffix == "parent" {
                    return ChildModelWorkspacePath::ParentSummary { suffix: None };
                }
                if let Some(parent_suffix) = suffix.strip_prefix("parent/") {
                    return if parent_suffix == "agents" || parent_suffix.starts_with("agents/") {
                        ChildModelWorkspacePath::ParentAgentsAlias
                    } else {
                        ChildModelWorkspacePath::ParentSummary {
                            suffix: Some(parent_suffix.to_string()),
                        }
                    };
                }
            }

            return ChildModelWorkspacePath::PrivateSummary { suffix };
        }

        if let Some(suffix) = suffix_for_root(value, SCRATCH_ROOT) {
            return ChildModelWorkspacePath::PrivateScratch { suffix };
        }

        ChildModelWorkspacePath::Other
    }

    fn model_to_physical_write_path(
        &self,
        path: &WorkspacePath,
    ) -> Result<WorkspacePath, DomainError> {
        match self.classify_model_path(path) {
            ChildModelWorkspacePath::PrivateSummary { suffix: Some(_) }
            | ChildModelWorkspacePath::PrivateScratch { suffix: Some(_) } => {}
            ChildModelWorkspacePath::ParentSummary { .. } => {
                return Err(DomainError::InvalidData(
                    "agent.child_parent_summary_write_denied: summaries/parent/ is read-only"
                        .to_string(),
                ));
            }
            ChildModelWorkspacePath::AgentSummaries
            | ChildModelWorkspacePath::CurrentAgentSummaryAlias => {
                return Err(DomainError::InvalidData(
                    "agent.child_agent_summary_write_denied: summaries/agents/ is read-only"
                        .to_string(),
                ));
            }
            ChildModelWorkspacePath::ParentAgentsAlias => {
                return Err(DomainError::InvalidData(
                    "agent.child_parent_agents_path_denied: use summaries/agents/ for other delegated Agent notes".to_string(),
                ));
            }
            ChildModelWorkspacePath::PrivateSummary { suffix: None }
            | ChildModelWorkspacePath::PrivateScratch { suffix: None }
            | ChildModelWorkspacePath::Other => {
                return Err(DomainError::InvalidData(format!(
                    "agent.child_workspace_write_denied: return-mode child Agents may write only under {SUMMARY_ROOT}/ or {SCRATCH_ROOT}/"
                )));
            }
        }
        self.model_to_physical_path(path)
    }

    fn physical_to_model_path(
        &self,
        path: &WorkspacePath,
    ) -> Result<Option<WorkspacePath>, DomainError> {
        let path = path.as_str();
        let private_summary_root = self.private_summary_root();
        if path == private_summary_root {
            return WorkspacePath::parse(SUMMARY_ROOT).map(Some);
        }
        if let Some(rest) = path.strip_prefix(&(private_summary_root.clone() + "/")) {
            return WorkspacePath::parse(format!("summaries/{rest}")).map(Some);
        }

        let private_scratch_root = self.private_scratch_root();
        if path == private_scratch_root {
            return WorkspacePath::parse(SCRATCH_ROOT).map(Some);
        }
        if let Some(rest) = path.strip_prefix(&(private_scratch_root + "/")) {
            return WorkspacePath::parse(format!("scratch/{rest}")).map(Some);
        }

        if path == SUMMARY_ROOT {
            return WorkspacePath::parse(SUMMARY_PARENT_ROOT).map(Some);
        }
        if path == SUMMARY_AGENTS_ROOT || path_matches_child(path, SUMMARY_AGENTS_ROOT) {
            return WorkspacePath::parse(path).map(Some);
        }
        if let Some(rest) = path.strip_prefix("summaries/") {
            return WorkspacePath::parse(format!("summaries/parent/{rest}")).map(Some);
        }
        if path == SCRATCH_ROOT || path.starts_with("scratch/") {
            return Ok(None);
        }

        WorkspacePath::parse(path).map(Some)
    }

    fn physical_to_model_path_for_list(
        &self,
        path: &WorkspacePath,
        requested_path: Option<&WorkspacePath>,
    ) -> Result<Option<WorkspacePath>, DomainError> {
        if requested_path.is_none() {
            return self.physical_to_model_path_for_root_list(path);
        }
        let requested = requested_path.expect("checked above").as_str();
        if requested == SUMMARY_PARENT_ROOT || path_matches_child(requested, SUMMARY_PARENT_ROOT) {
            return self.physical_parent_summary_to_model_path(path);
        }
        if requested == SUMMARY_AGENTS_ROOT || path_matches_child(requested, SUMMARY_AGENTS_ROOT) {
            return self.physical_agent_summaries_to_model_path(path);
        }
        self.physical_to_model_path(path)
    }

    fn physical_to_model_path_for_root_list(
        &self,
        path: &WorkspacePath,
    ) -> Result<Option<WorkspacePath>, DomainError> {
        let value = path.as_str();
        if value == SUMMARY_ROOT {
            return WorkspacePath::parse(SUMMARY_ROOT).map(Some);
        }
        if value == SCRATCH_ROOT {
            return WorkspacePath::parse(SCRATCH_ROOT).map(Some);
        }
        if value == SUMMARY_AGENTS_ROOT || path_matches_child(value, SUMMARY_AGENTS_ROOT) {
            return self.physical_agent_summaries_to_model_path(path);
        }
        self.physical_to_model_path(path)
    }

    fn physical_parent_summary_to_model_path(
        &self,
        path: &WorkspacePath,
    ) -> Result<Option<WorkspacePath>, DomainError> {
        let value = path.as_str();
        if value == SUMMARY_ROOT {
            return WorkspacePath::parse(SUMMARY_PARENT_ROOT).map(Some);
        }
        if value == SUMMARY_AGENTS_ROOT || path_matches_child(value, SUMMARY_AGENTS_ROOT) {
            return Ok(None);
        }
        if let Some(rest) = value.strip_prefix("summaries/") {
            return WorkspacePath::parse(format!("summaries/parent/{rest}")).map(Some);
        }
        self.physical_to_model_path(path)
    }

    fn physical_agent_summaries_to_model_path(
        &self,
        path: &WorkspacePath,
    ) -> Result<Option<WorkspacePath>, DomainError> {
        let value = path.as_str();
        if value == SUMMARY_AGENTS_ROOT {
            return WorkspacePath::parse(SUMMARY_AGENTS_ROOT).map(Some);
        }
        if self.is_private_summary_physical_path(value) {
            return Ok(None);
        }
        if path_matches_child(value, SUMMARY_AGENTS_ROOT) {
            return WorkspacePath::parse(value).map(Some);
        }
        self.physical_to_model_path(path)
    }

    fn model_file_from_physical(&self, file: WorkspaceFile) -> Result<WorkspaceFile, DomainError> {
        let physical_path = file.path.clone();
        let path = self
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
                    .and_then(|path| self.physical_to_model_path(&path).ok().flatten())
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

    fn private_summary_root(&self) -> String {
        format!("{SUMMARY_AGENT_PREFIX}{}", self.workspace_key)
    }

    fn private_scratch_root(&self) -> String {
        format!("{SCRATCH_AGENT_PREFIX}{}", self.workspace_key)
    }

    fn is_private_summary_physical_path(&self, path: &str) -> bool {
        let root = self.private_summary_root();
        path == root || path_matches_child(path, &root)
    }

    fn is_current_agent_summary_alias(&self, path: &str) -> bool {
        let alias = format!("{SUMMARY_AGENTS_ROOT}/{}", self.workspace_key);
        path == alias || path_matches_child(path, &alias)
    }
}

fn suffix_for_root(path: &str, root: &str) -> Option<Option<String>> {
    if path == root {
        return Some(None);
    }
    path.strip_prefix(&(root.to_string() + "/"))
        .map(|suffix| Some(suffix.to_string()))
}

fn join_child_path(root: impl Into<String>, suffix: Option<&str>) -> String {
    let root = root.into();
    match suffix {
        Some(suffix) => format!("{root}/{suffix}"),
        None => root,
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
            .map(child_visible_root)
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

    async fn read_text(
        &self,
        run_id: &str,
        path: &WorkspacePath,
    ) -> Result<WorkspaceFile, DomainError> {
        let requested_path = path.clone();
        let path = self
            .view
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
            .map(|path| self.view.model_to_physical_path(path))
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

    async fn prepare_persistent_changes(
        &self,
        run_id: &str,
    ) -> Result<WorkspacePersistentChangeSet, DomainError> {
        self.inner.prepare_persistent_changes(run_id).await
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
    ) -> Result<ChildWorkspaceView, ApplicationError> {
        let task = self
            .task_for_child_invocation(run_id, invocation_id)
            .await?
            .ok_or_else(|| {
                ApplicationError::ValidationError(format!(
                    "agent.task_record_missing: no task record owns child invocation `{invocation_id}`"
                ))
            })?;
        Ok(ChildWorkspaceView::new(task.workspace_key))
    }
}

fn child_visible_root(mut root: WorkspaceRootSpec) -> Result<WorkspaceRootSpec, DomainError> {
    let path = WorkspacePath::parse(&root.path)?;
    root.writable = path.as_str() == SUMMARY_ROOT || path.as_str() == SCRATCH_ROOT;
    Ok(root)
}

fn path_matches_child(path: &str, root: &str) -> bool {
    path.len() > root.len()
        && path.starts_with(root)
        && path.as_bytes().get(root.len()) == Some(&b'/')
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    #[test]
    fn child_model_paths_map_to_semantic_physical_paths() {
        let view = ChildWorkspaceView::new("scene-critic".to_string());

        assert_eq!(
            view.model_to_physical_path(&path("summaries/notes.md"))
                .unwrap()
                .as_str(),
            "summaries/agents/scene-critic/notes.md"
        );
        assert_eq!(
            view.model_to_physical_path(&path("scratch/draft.md"))
                .unwrap()
                .as_str(),
            "scratch/agents/scene-critic/draft.md"
        );
        assert_eq!(
            view.model_to_physical_path(&path("summaries/parent/world.md"))
                .unwrap()
                .as_str(),
            "summaries/world.md"
        );
        assert_eq!(
            view.model_to_physical_path(&path("summaries/agents/other-agent/notes.md"))
                .unwrap()
                .as_str(),
            "summaries/agents/other-agent/notes.md"
        );
        assert!(
            view.model_to_physical_path(&path("summaries/parent/agents/other-agent/notes.md"))
                .is_err()
        );
        assert!(
            view.model_to_physical_path(&path("summaries/agents/scene-critic/notes.md"))
                .is_err()
        );
    }

    #[test]
    fn child_physical_paths_map_back_to_child_view() {
        let view = ChildWorkspaceView::new("scene-critic".to_string());

        assert_eq!(
            view.physical_to_model_path(&path("summaries/agents/scene-critic/notes.md"))
                .unwrap()
                .unwrap()
                .as_str(),
            "summaries/notes.md"
        );
        assert_eq!(
            view.physical_to_model_path(&path("scratch/agents/scene-critic/draft.md"))
                .unwrap()
                .unwrap()
                .as_str(),
            "scratch/draft.md"
        );
        assert_eq!(
            view.physical_parent_summary_to_model_path(&path("summaries/world.md"))
                .unwrap()
                .unwrap()
                .as_str(),
            "summaries/parent/world.md"
        );
        assert_eq!(
            view.physical_to_model_path(&path("summaries/agents/other-agent/notes.md"))
                .unwrap()
                .unwrap()
                .as_str(),
            "summaries/agents/other-agent/notes.md"
        );
    }

    #[test]
    fn list_views_keep_parent_private_and_other_agent_summaries_separate() {
        let view = ChildWorkspaceView::new("scene-critic".to_string());

        assert_eq!(
            view.physical_to_model_path_for_list(&path("summaries"), None)
                .unwrap()
                .unwrap()
                .as_str(),
            "summaries"
        );
        assert_eq!(
            view.physical_to_model_path_for_list(
                &path("summaries/world.md"),
                Some(&path("summaries/parent"))
            )
            .unwrap()
            .unwrap()
            .as_str(),
            "summaries/parent/world.md"
        );
        assert_eq!(
            view.physical_to_model_path_for_list(
                &path("summaries/agents/other-agent/notes.md"),
                Some(&path("summaries/agents"))
            )
            .unwrap()
            .unwrap()
            .as_str(),
            "summaries/agents/other-agent/notes.md"
        );
        assert!(
            view.physical_to_model_path_for_list(
                &path("summaries/agents/other-agent/notes.md"),
                Some(&path("summaries/parent"))
            )
            .unwrap()
            .is_none()
        );
        assert!(
            view.physical_to_model_path_for_list(
                &path("summaries/agents/scene-critic/notes.md"),
                Some(&path("summaries/agents"))
            )
            .unwrap()
            .is_none()
        );
    }

    #[test]
    fn child_write_policy_accepts_private_notes_and_rejects_parent_summary() {
        let view = ChildWorkspaceView::new("scene-critic".to_string());

        assert!(!view.write_is_denied(&write_call("summaries/notes.md")));
        assert!(!view.write_is_denied(&write_call("scratch/notes.md")));
        assert!(view.write_is_denied(&write_call("summaries/parent/world.md")));
        assert!(view.write_is_denied(&write_call("summaries/agents/other-agent/notes.md")));
        assert!(view.write_is_denied(&write_call("output/main.md")));
    }

    #[test]
    fn child_workspace_errors_use_model_facing_paths() {
        let view = ChildWorkspaceView::new("scene-critic".to_string());

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
            view.model_to_physical_path(&path("summaries/parent/agents/other-agent/notes.md"))
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

    fn write_call(path: &str) -> AgentToolCall {
        AgentToolCall {
            id: "call_test".to_string(),
            name: "workspace.write_file".to_string(),
            arguments: json!({ "path": path, "content": "text" }),
            provider_metadata: Value::Null,
        }
    }
}
