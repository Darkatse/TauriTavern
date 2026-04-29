use serde_json::json;

use super::args::optional_list_path_arg;
use super::policy::WorkspaceAccessPolicy;
use crate::domain::models::agent::WorkspacePath;

fn test_policy() -> WorkspaceAccessPolicy {
    let roots = ["output", "scratch", "plan", "summaries", "persist"]
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
    WorkspaceAccessPolicy {
        visible_roots: roots.clone(),
        writable_roots: roots,
    }
}

#[test]
fn writable_policy_rejects_input_paths() {
    let path = WorkspacePath::parse("input/prompt_snapshot.json").unwrap();
    assert!(test_policy().ensure_writable(&path).is_err());
}

#[test]
fn visible_policy_allows_workspace_artifact_roots() {
    for value in [
        "output",
        "scratch/file.md",
        "plan/outline.md",
        "summaries/a.md",
        "persist/MEMORY.md",
    ] {
        let path = WorkspacePath::parse(value).unwrap();
        assert!(test_policy().ensure_visible(&path).is_ok());
    }
}

#[test]
fn writable_policy_requires_child_path() {
    let root = WorkspacePath::parse("output").unwrap();
    let file = WorkspacePath::parse("output/main.md").unwrap();

    assert!(test_policy().ensure_writable(&root).is_err());
    assert!(test_policy().ensure_writable(&file).is_ok());
}

#[test]
fn list_path_arg_treats_empty_and_dot_as_workspace_root() {
    for value in ["", " ", ".", "./"] {
        let args = json!({ "path": value });
        assert!(
            optional_list_path_arg(args.as_object().unwrap(), "path")
                .unwrap()
                .is_none()
        );
    }
}
