use crate::errors::ApplicationError;
use async_trait::async_trait;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
use tt_domain::errors::DomainError;
use tt_domain::frozen_macros::FrozenMacros;
use tt_domain::models::agent::WorkspacePath;
use tt_domain::models::agent::profile::ResolvedAgentProfile;
use tt_domain::models::skill::SkillIndexEntry;
use tt_ports::repositories::skill_repository::SkillRepository;
use tt_ports::workspace_fs::{
    WorkspaceAppendResult, WorkspaceDirectoryEntry, WorkspaceEntryKind, WorkspaceFs,
    WorkspaceMetadata, WorkspaceWriteGuard,
};

mod skills;

pub(crate) const AGENT_TOOL_RESULTS_ROOT: &str = "tool-results";
const SKILLS_ROOT: &str = "skills";

pub(crate) fn is_auto_commit_text_path(path: &WorkspacePath) -> bool {
    Path::new(path.as_str())
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            ["md", "markdown", "txt", "text"]
                .iter()
                .any(|candidate| extension.eq_ignore_ascii_case(candidate))
        })
}

pub(crate) fn task_result_summary_path(workspace_key: &str) -> Result<WorkspacePath, DomainError> {
    WorkspacePath::parse(format!("summaries/{workspace_key}-result.md"))
}

pub(crate) fn workspace_path_is_under_any_root(path: &WorkspacePath, roots: &[String]) -> bool {
    roots
        .iter()
        .any(|root| path_matches_root_or_child(path.as_str(), root))
}

pub(crate) fn format_model_workspace_roots(roots: &[String]) -> String {
    roots
        .iter()
        .map(|root| format!("{root}/"))
        .collect::<Vec<_>>()
        .join(", ")
}

pub(crate) fn format_model_visible_workspace_roots(roots: &[String]) -> String {
    let mut roots = roots.to_vec();
    for root in [AGENT_TOOL_RESULTS_ROOT, SKILLS_ROOT] {
        if !roots.iter().any(|existing| existing == root) {
            roots.push(root.to_string());
        }
    }
    format_model_workspace_roots(&roots)
}

fn path_matches_root_or_child(path: &str, root: &str) -> bool {
    path == root || path_matches_child(path, root)
}

fn path_matches_child(path: &str, root: &str) -> bool {
    path.len() > root.len()
        && path.starts_with(root)
        && path.as_bytes().get(root.len()) == Some(&b'/')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_path_is_under_any_root_matches_root_boundary() {
        let roots = vec!["output".to_string()];

        assert!(workspace_path_is_under_any_root(
            &WorkspacePath::parse("output/main.md").unwrap(),
            &roots
        ));
        assert!(workspace_path_is_under_any_root(
            &WorkspacePath::parse("output").unwrap(),
            &roots
        ));
        assert!(!workspace_path_is_under_any_root(
            &WorkspacePath::parse("output_extra/main.md").unwrap(),
            &roots
        ));
    }
}

#[derive(Debug)]
pub(crate) struct WorkspaceAccessPolicy {
    pub(crate) visible_roots: Vec<String>,
    pub(crate) writable_roots: Vec<String>,
}

impl WorkspaceAccessPolicy {
    pub(crate) fn from_profile(profile: &ResolvedAgentProfile) -> Self {
        let mut visible_roots = profile.workspace.visible_roots.clone();
        for root in [AGENT_TOOL_RESULTS_ROOT, SKILLS_ROOT] {
            if !visible_roots.iter().any(|existing| existing == root) {
                visible_roots.push(root.to_string());
            }
        }
        visible_roots.sort();
        visible_roots.dedup();
        let mut writable_roots: Vec<_> = profile
            .workspace
            .writable_roots
            .iter()
            .filter(|root| ![AGENT_TOOL_RESULTS_ROOT, SKILLS_ROOT].contains(&root.as_str()))
            .cloned()
            .collect();
        writable_roots.sort();
        writable_roots.dedup();
        Self {
            visible_roots,
            writable_roots,
        }
    }

    pub(crate) fn ensure_visible(&self, path: &WorkspacePath) -> Result<(), ApplicationError> {
        if self.is_visible(path) {
            return Ok(());
        }

        let value = path.as_str();
        Err(ApplicationError::PermissionDenied(format!(
            "`{value}` is not readable for this task."
        )))
    }

    pub(crate) fn ensure_writable(&self, path: &WorkspacePath) -> Result<(), ApplicationError> {
        if self.is_writable(path) {
            return Ok(());
        }

        let value = path.as_str();
        Err(ApplicationError::PermissionDenied(format!(
            "`{value}` is not writable for this task."
        )))
    }

    pub(crate) fn is_visible(&self, path: &WorkspacePath) -> bool {
        workspace_path_is_under_any_root(path, &self.visible_roots)
    }

    pub(crate) fn is_writable(&self, path: &WorkspacePath) -> bool {
        !is_skill_path(path)
            && self
                .writable_roots
                .iter()
                .any(|root| path_matches_child(path.as_str(), root))
    }
}

/// Invocation policy is a view over the Run's shared filesystem, not a copy.
pub(crate) struct ScopedWorkspaceFs {
    inner: Arc<dyn WorkspaceFs>,
    pub(crate) policy: WorkspaceAccessPolicy,
    skill_repository: Option<Arc<dyn SkillRepository>>,
    skill_bindings: Arc<[SkillIndexEntry]>,
    frozen_macros: Arc<FrozenMacros>,
    text_mutation: Option<Mutex<WorkspaceTextMutation>>,
}

/// A Shell call starts from the round's candidate and reports whether it changed.
#[derive(Clone)]
pub(crate) struct WorkspaceTextMutation {
    pub(crate) candidate: Option<WorkspacePath>,
    pub(crate) changed: bool,
}

impl ScopedWorkspaceFs {
    pub(crate) fn new(inner: Arc<dyn WorkspaceFs>, policy: WorkspaceAccessPolicy) -> Self {
        Self {
            inner,
            policy,
            skill_repository: None,
            skill_bindings: Arc::default(),
            frozen_macros: Arc::default(),
            text_mutation: None,
        }
    }

    pub(crate) fn with_skills(
        mut self,
        repository: Arc<dyn SkillRepository>,
        bindings: Arc<[SkillIndexEntry]>,
        frozen_macros: Arc<FrozenMacros>,
    ) -> Self {
        self.skill_repository = Some(repository);
        self.skill_bindings = bindings;
        self.frozen_macros = frozen_macros;
        self
    }

    pub(crate) fn track_text_mutations(mut self, candidate: Option<WorkspacePath>) -> Self {
        self.text_mutation = Some(Mutex::new(WorkspaceTextMutation {
            candidate,
            changed: false,
        }));
        self
    }

    fn mutation(&self) -> Option<MutexGuard<'_, WorkspaceTextMutation>> {
        self.text_mutation.as_ref().map(|mutation| {
            mutation
                .lock()
                .expect("workspace text mutation lock poisoned")
        })
    }

    pub(crate) fn text_mutation(&self) -> Option<WorkspaceTextMutation> {
        self.mutation().map(|mutation| mutation.clone())
    }

    fn remember_write(&self, path: &WorkspacePath) {
        if is_auto_commit_text_path(path)
            && let Some(mut mutation) = self.mutation()
        {
            mutation.candidate = Some(path.clone());
            mutation.changed = true;
        }
    }

    fn forget_removed(&self, path: &WorkspacePath) {
        if let Some(mut mutation) = self.mutation()
            && mutation.candidate.as_ref().is_some_and(|candidate| {
                path_matches_root_or_child(candidate.as_str(), path.as_str())
            })
        {
            mutation.candidate = None;
            mutation.changed = true;
        }
    }

    fn check(&self, path: &WorkspacePath, write: bool) -> Result<(), DomainError> {
        if if write {
            self.policy.is_writable(path)
        } else {
            self.policy.is_visible(path)
        } {
            Ok(())
        } else {
            Err(DomainError::WorkspaceAccessDenied {
                path: path.as_str().to_owned(),
                operation: if write { "write" } else { "read" },
            })
        }
    }
}

#[async_trait]
impl WorkspaceFs for ScopedWorkspaceFs {
    async fn read_file(
        &self,
        path: &WorkspacePath,
        maximum_bytes: usize,
    ) -> Result<Vec<u8>, DomainError> {
        self.check(path, false)?;
        if is_skill_path(path) {
            return self.read_skill_file(path, maximum_bytes).await;
        }
        self.inner.read_file(path, maximum_bytes).await
    }
    async fn write_file(
        &self,
        path: &WorkspacePath,
        bytes: &[u8],
        guard: WorkspaceWriteGuard,
    ) -> Result<(), DomainError> {
        self.check(path, true)?;
        self.inner.write_file(path, bytes, guard).await?;
        self.remember_write(path);
        Ok(())
    }
    async fn append_file(&self, path: &WorkspacePath, bytes: &[u8]) -> Result<(), DomainError> {
        self.check(path, true)?;
        self.inner.append_file(path, bytes).await?;
        self.remember_write(path);
        Ok(())
    }
    async fn append_text(
        &self,
        path: &WorkspacePath,
        text: &str,
    ) -> Result<WorkspaceAppendResult, DomainError> {
        self.check(path, true)?;
        let result = self.inner.append_text(path, text).await?;
        self.remember_write(path);
        Ok(result)
    }
    async fn metadata(
        &self,
        path: Option<&WorkspacePath>,
    ) -> Result<WorkspaceMetadata, DomainError> {
        match path {
            Some(path) => {
                self.check(path, false)?;
                if is_skill_path(path) {
                    return self.skill_metadata(path).await;
                }
                self.inner.metadata(Some(path)).await
            }
            None => Ok(WorkspaceMetadata {
                kind: WorkspaceEntryKind::Directory,
                bytes: 0,
                modified: None,
                created: None,
            }),
        }
    }
    async fn read_dir(
        &self,
        path: Option<&WorkspacePath>,
        maximum_entries: usize,
    ) -> Result<Vec<WorkspaceDirectoryEntry>, DomainError> {
        if let Some(path) = path {
            self.check(path, false)?;
            if is_skill_path(path) {
                return self.read_skill_dir(path, maximum_entries).await;
            }
            return self.inner.read_dir(Some(path), maximum_entries).await;
        }
        if self.policy.visible_roots.len() > maximum_entries {
            return Err(DomainError::InvalidData(format!(
                "Workspace root exceeds {maximum_entries} entries"
            )));
        }
        let mut entries = Vec::new();
        for root in &self.policy.visible_roots {
            let path = WorkspacePath::parse(root)?;
            let metadata = self.metadata(Some(&path)).await?;
            entries.push(WorkspaceDirectoryEntry { path, metadata });
        }
        entries.sort_by(|a, b| a.path.as_str().cmp(b.path.as_str()));
        Ok(entries)
    }
    async fn create_dir(&self, path: &WorkspacePath, recursive: bool) -> Result<(), DomainError> {
        self.check(path, true)?;
        self.inner.create_dir(path, recursive).await
    }
    async fn remove(&self, path: &WorkspacePath, recursive: bool) -> Result<(), DomainError> {
        self.check(path, true)?;
        self.inner.remove(path, recursive).await?;
        self.forget_removed(path);
        Ok(())
    }
    async fn rename(
        &self,
        source: &WorkspacePath,
        target: &WorkspacePath,
    ) -> Result<(), DomainError> {
        self.check(source, true)?;
        self.check(target, true)?;
        self.inner.rename(source, target).await?;
        if self.text_mutation.is_some() {
            let target_is_text_file = is_auto_commit_text_path(target)
                && self.inner.metadata(Some(target)).await?.kind == WorkspaceEntryKind::File;
            let mut mutation = self.mutation().expect("text mutation tracking is enabled");
            if target_is_text_file {
                mutation.candidate = Some(target.clone());
                mutation.changed = true;
            } else if let Some(path) = mutation
                .candidate
                .as_ref()
                .filter(|path| path_matches_root_or_child(path.as_str(), source.as_str()))
            {
                // Move the known candidate with its directory, without scanning the subtree.
                let suffix = &path.as_str()[source.as_str().len()..];
                let moved = WorkspacePath::parse(format!("{}{suffix}", target.as_str()))?;
                mutation.candidate = is_auto_commit_text_path(&moved).then_some(moved);
                mutation.changed = true;
            }
        }
        Ok(())
    }
    async fn copy_file(
        &self,
        source: &WorkspacePath,
        target: &WorkspacePath,
    ) -> Result<(), DomainError> {
        self.check(source, false)?;
        self.check(target, true)?;
        if is_skill_path(source) {
            let bytes = self.read_skill_file(source, usize::MAX).await?;
            self.inner
                .write_file(target, &bytes, WorkspaceWriteGuard::Unchecked)
                .await?;
        } else {
            self.inner.copy_file(source, target).await?;
        }
        self.remember_write(target);
        Ok(())
    }
}

fn is_skill_path(path: &WorkspacePath) -> bool {
    path_matches_root_or_child(path.as_str(), SKILLS_ROOT)
}
