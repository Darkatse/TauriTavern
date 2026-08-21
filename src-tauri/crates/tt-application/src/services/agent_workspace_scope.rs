use tt_domain::errors::DomainError;
use tt_domain::models::agent::WorkspacePath;

pub(crate) const AGENT_TOOL_RESULTS_ROOT: &str = "tool-results";

pub(crate) fn task_result_summary_path(workspace_key: &str) -> Result<WorkspacePath, DomainError> {
    WorkspacePath::parse(format!("summaries/{workspace_key}-result.md"))
}

pub(crate) fn workspace_path_is_under_any_root(path: &WorkspacePath, roots: &[String]) -> bool {
    roots
        .iter()
        .any(|root| path_matches_root_or_child(path.as_str(), root))
}

/// 判断路径是否为某个 writable_root 的子项（不含根本身）。
/// 语义与 `WorkspaceAccessPolicy::is_writable` 完全一致，供 workspace
/// 工具和 skill 脚本写入校验共用。
pub(crate) fn is_writable_workspace_path(path: &WorkspacePath, writable_roots: &[String]) -> bool {
    writable_roots
        .iter()
        .any(|root| path_matches_child(path.as_str(), root))
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
    if !roots.iter().any(|root| root == AGENT_TOOL_RESULTS_ROOT) {
        roots.push(AGENT_TOOL_RESULTS_ROOT.to_string());
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
    fn task_result_summary_path_is_flat_and_stable() {
        let path = task_result_summary_path("scene-critic-002").expect("summary path");

        assert_eq!(path.as_str(), "summaries/scene-critic-002-result.md");
    }

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
