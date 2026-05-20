use serde_json::json;

use super::args::{classify_workspace_io_error, optional_list_path_arg};
use super::policy::WorkspaceAccessPolicy;
use crate::domain::errors::DomainError;
use crate::domain::models::agent::{AgentToolCall, WorkspacePath};

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

fn make_test_tool_call(name: &str) -> AgentToolCall {
    AgentToolCall {
        id: "call_test".to_string(),
        name: name.to_string(),
        arguments: json!({}),
        provider_metadata: json!({}),
    }
}

#[test]
fn classify_workspace_path_is_directory_error_maps_to_tool_error() {
    // Issue #54: a directory hit on workspace_read_file used to surface as
    // `agent.internal_error`. The tool layer now classifies the
    // repository's typed domain error into the recoverable
    // `workspace.path_is_directory` business error so the model can
    // self-correct by calling workspace_list_files.
    let call = make_test_tool_call("workspace.read_file");
    let error = DomainError::workspace_path_is_directory("persist");

    let result = classify_workspace_io_error(&call, error)
        .expect("directory error must classify into a tool result, not a hard error");

    assert!(result.is_error);
    assert_eq!(
        result.error_code.as_deref(),
        Some("workspace.path_is_directory")
    );
    assert!(
        result.content.contains("persist"),
        "tool error content should preserve the offending path: {}",
        result.content
    );
}

#[test]
fn classify_not_found_error_maps_to_file_not_found() {
    let call = make_test_tool_call("workspace.read_file");
    let error = DomainError::NotFound("Workspace file not found: persist/MEMORY.md".to_string());

    let result = classify_workspace_io_error(&call, error)
        .expect("not found must classify into a tool result");

    assert!(result.is_error);
    assert_eq!(
        result.error_code.as_deref(),
        Some("workspace.file_not_found")
    );
}

#[test]
fn classify_unknown_error_bubbles_up_for_host_failure() {
    let call = make_test_tool_call("workspace.read_file");
    let error = DomainError::InternalError("disk pressure".to_string());

    let result = classify_workspace_io_error(&call, error);
    assert!(
        result.is_err(),
        "infrastructural errors must remain host-level failures",
    );
}
