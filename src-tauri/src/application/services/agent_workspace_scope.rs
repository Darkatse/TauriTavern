use crate::domain::errors::DomainError;
use crate::domain::models::agent::WorkspacePath;

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
