use crate::domain::errors::DomainError;
use crate::domain::models::agent::profile::ResolvedAgentProfile;
use crate::domain::models::agent::{WorkspacePath, WorkspaceRootSpec};

pub(crate) const SUMMARY_ROOT: &str = "summaries";
pub(crate) const SCRATCH_ROOT: &str = "scratch";

const SUMMARY_AGENT_PREFIX: &str = "summaries/agents/";
const SUMMARY_AGENTS_ROOT: &str = "summaries/agents";
const SCRATCH_AGENT_PREFIX: &str = "scratch/agents/";
const SUMMARY_PARENT_ROOT: &str = "summaries/parent";

#[derive(Debug, Clone)]
pub(crate) struct ReturnModeWorkspaceScope {
    shared_visible_roots: Vec<String>,
    shared_writable_roots: Vec<String>,
}

impl ReturnModeWorkspaceScope {
    pub(crate) fn from_profile(profile: &ResolvedAgentProfile) -> Self {
        Self {
            shared_visible_roots: profile.workspace.visible_roots.clone(),
            shared_writable_roots: profile.workspace.writable_roots.clone(),
        }
    }

    pub(crate) fn model_visible_roots(&self) -> Vec<String> {
        roots_with_private_task_roots(&self.shared_visible_roots)
    }

    pub(crate) fn model_writable_roots(&self) -> Vec<String> {
        roots_with_private_task_roots(&self.shared_writable_roots)
    }

    pub(crate) fn child(&self, workspace_key: String) -> ChildWorkspaceScope {
        ChildWorkspaceScope::new(
            workspace_key,
            self.shared_visible_roots.clone(),
            self.shared_writable_roots.clone(),
        )
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ChildWorkspaceScope {
    workspace_key: String,
    shared_visible_roots: Vec<String>,
    shared_writable_roots: Vec<String>,
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

impl ChildWorkspaceScope {
    pub(crate) fn new(
        workspace_key: String,
        shared_visible_roots: Vec<String>,
        shared_writable_roots: Vec<String>,
    ) -> Self {
        Self {
            workspace_key,
            shared_visible_roots,
            shared_writable_roots,
        }
    }

    pub(crate) fn for_profile(workspace_key: String, profile: &ResolvedAgentProfile) -> Self {
        ReturnModeWorkspaceScope::from_profile(profile).child(workspace_key)
    }

    pub(crate) fn summary_result_path(&self) -> Result<WorkspacePath, DomainError> {
        WorkspacePath::parse(format!("summaries/agents/{}/result.md", self.workspace_key))
    }

    pub(crate) fn model_to_physical_path(
        &self,
        path: &WorkspacePath,
    ) -> Result<WorkspacePath, DomainError> {
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
            ChildModelWorkspacePath::Other => {
                if self.shared_path_is_visible(path) {
                    WorkspacePath::parse(path.as_str())
                } else {
                    Err(DomainError::InvalidData(format!(
                        "agent.child_workspace_read_denied: path `{}` is not visible to this delegated task",
                        path.as_str()
                    )))
                }
            }
        }
    }

    pub(crate) fn model_to_physical_write_path(
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
            | ChildModelWorkspacePath::PrivateScratch { suffix: None } => {
                return Err(DomainError::InvalidData(format!(
                    "agent.child_workspace_write_denied: write a concrete file under {SUMMARY_ROOT}/ or {SCRATCH_ROOT}/"
                )));
            }
            ChildModelWorkspacePath::Other => {
                if !self.shared_path_is_writable(path) {
                    return Err(DomainError::InvalidData(format!(
                        "agent.child_workspace_write_denied: path `{}` is not writable to this delegated task",
                        path.as_str()
                    )));
                }
            }
        }
        self.model_to_physical_path(path)
    }

    pub(crate) fn physical_to_model_path(
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

        let shared_path = WorkspacePath::parse(path)?;
        if self.shared_path_is_visible(&shared_path) {
            Ok(Some(shared_path))
        } else {
            Ok(None)
        }
    }

    pub(crate) fn physical_to_model_path_for_list(
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

    pub(crate) fn physical_parent_summary_to_model_path(
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

    pub(crate) fn physical_agent_summaries_to_model_path(
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

    pub(crate) fn child_visible_root(
        &self,
        mut root: WorkspaceRootSpec,
    ) -> Result<WorkspaceRootSpec, DomainError> {
        let path = WorkspacePath::parse(&root.path)?;
        root.visible = self.root_is_visible(path.as_str());
        root.writable = self.root_is_writable(path.as_str());
        Ok(root)
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

    fn shared_path_is_visible(&self, path: &WorkspacePath) -> bool {
        self.shared_visible_roots
            .iter()
            .any(|root| path_matches_root_or_child(path.as_str(), root))
    }

    fn shared_path_is_writable(&self, path: &WorkspacePath) -> bool {
        self.shared_writable_roots
            .iter()
            .any(|root| path_matches_child(path.as_str(), root))
    }

    fn root_is_visible(&self, root: &str) -> bool {
        root == SUMMARY_ROOT
            || root == SCRATCH_ROOT
            || self
                .shared_visible_roots
                .iter()
                .any(|allowed| allowed == root)
    }

    fn root_is_writable(&self, root: &str) -> bool {
        root == SUMMARY_ROOT
            || root == SCRATCH_ROOT
            || self
                .shared_writable_roots
                .iter()
                .any(|allowed| allowed == root)
    }
}

fn roots_with_private_task_roots(roots: &[String]) -> Vec<String> {
    let mut output = vec![SUMMARY_ROOT.to_string(), SCRATCH_ROOT.to_string()];
    for root in roots {
        if !output.iter().any(|existing| existing == root) {
            output.push(root.clone());
        }
    }
    output
}

pub(crate) fn format_model_workspace_roots(roots: &[String]) -> String {
    roots
        .iter()
        .map(|root| format!("{root}/"))
        .collect::<Vec<_>>()
        .join(", ")
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

fn path_matches_root_or_child(path: &str, root: &str) -> bool {
    path == root || path_matches_child(path, root)
}

fn path_matches_child(path: &str, root: &str) -> bool {
    path.len() > root.len()
        && path.starts_with(root)
        && path.as_bytes().get(root.len()) == Some(&b'/')
}
