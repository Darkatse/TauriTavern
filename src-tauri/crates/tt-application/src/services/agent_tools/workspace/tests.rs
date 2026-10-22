use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::{Map, Value, json};

use super::{MAX_READ_CHARS, MAX_READ_LINES, apply_patch, read_file, write_file};
use crate::services::agent_tools::{AgentToolEffect, AgentToolSession};
use crate::services::agent_workspace_scope::{ScopedWorkspaceFs, WorkspaceAccessPolicy};
use tt_domain::errors::{DomainError, WorkspaceWriteConflictKind};
use tt_domain::models::agent::{WorkspaceFileWriteMode, WorkspacePath};
use tt_domain::models::tool::{ToolArguments, ToolId, ToolInvocation};
use tt_ports::workspace_fs::{
    WorkspaceAppendResult, WorkspaceDirectoryEntry, WorkspaceEntryKind, WorkspaceFs,
    WorkspaceMetadata, WorkspaceWriteGuard, sha256_hex,
};

#[tokio::test]
async fn workspace_read_invalid_path_returns_canonical_tool_error() {
    let repository = TestWorkspaceFs::with_file("output/main.md", "existing");
    let mut session = AgentToolSession::default();
    let call = workspace_call("workspace.read_file", json!({ "path": "../secrets.json" }));

    let (result, effect) = read_file(&repository, &call, call_args(&call), &mut session)
        .await
        .expect("invalid model path must remain recoverable");

    assert!(matches!(effect, AgentToolEffect::None));
    assert!(result.is_error);
    assert_eq!(result.error_code.as_deref(), Some("workspace.invalid_path"));
    assert!(result.resource_refs.is_empty());
}

#[tokio::test]
async fn workspace_read_hidden_path_returns_recoverable_tool_error() {
    let repository = TestWorkspaceFs::with_file("output/main.md", "existing");
    let mut session = AgentToolSession::default();
    let call = workspace_call(
        "workspace.read_file",
        json!({ "path": "input/prompt_snapshot.json" }),
    );

    let (result, effect) = read_file(&repository, &call, call_args(&call), &mut session)
        .await
        .expect("hidden model path must remain recoverable");

    assert!(matches!(effect, AgentToolEffect::None));
    assert!(result.is_error);
    assert_eq!(
        result.error_code.as_deref(),
        Some("workspace.path_not_visible")
    );
}

#[tokio::test]
async fn workspace_read_defaults_to_a_preview_for_oversized_files() {
    let text = (1..=MAX_READ_LINES + 1)
        .map(|line| format!("line {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    let repository = TestWorkspaceFs::with_file("output/large.md", &text);
    let mut session = AgentToolSession::default();
    let call = workspace_call("workspace.read_file", json!({ "path": "output/large.md" }));

    let (result, _) = read_file(&repository, &call, call_args(&call), &mut session)
        .await
        .expect("large read should return a preview");

    assert!(!result.is_error);
    assert_eq!(result.structured["startLine"], 1);
    assert_eq!(result.structured["endLine"], MAX_READ_LINES);
    assert_eq!(result.structured["nextStartLine"], MAX_READ_LINES + 1);
    assert_eq!(result.structured["truncated"], true);
    assert!(result.content.contains("Continue with start_line="));
}

#[tokio::test]
async fn workspace_read_keeps_a_large_single_line_out_of_the_next_model_request() {
    let text = "x".repeat(MAX_READ_CHARS + 1);
    let repository = TestWorkspaceFs::with_file("output/large.txt", &text);
    let mut session = AgentToolSession::default();
    let call = workspace_call("workspace.read_file", json!({ "path": "output/large.txt" }));

    let (result, _) = read_file(&repository, &call, call_args(&call), &mut session)
        .await
        .expect("large read should return a preview");

    assert!(!result.is_error);
    assert_eq!(result.structured["lineTruncated"], true);
    assert_eq!(result.structured["fullRead"], false);
    assert!(result.content.contains("only its beginning is shown"));
}

#[tokio::test]
async fn workspace_write_root_returns_recoverable_tool_error() {
    let repository = TestWorkspaceFs::with_file("output/main.md", "existing");
    let mut session = AgentToolSession::default();
    let call = workspace_call(
        "workspace.write_file",
        json!({
            "path": "output",
            "content": "replacement",
        }),
    );

    let (result, effect) = write_file(&repository, &call, call_args(&call), &mut session)
        .await
        .expect("non-writable model path must remain recoverable");

    assert!(matches!(effect, AgentToolEffect::None));
    assert!(result.is_error);
    assert_eq!(
        result.error_code.as_deref(),
        Some("workspace.path_not_writable")
    );
}

#[tokio::test]
async fn workspace_write_existing_file_requires_prior_read() {
    let repository = TestWorkspaceFs::with_file("output/main.md", "old text");
    let mut session = AgentToolSession::default();

    let write_call = workspace_call(
        "workspace.write_file",
        json!({
            "path": "output/main.md",
            "content": "new text",
        }),
    );
    let (result, _) = write_file(
        &repository,
        &write_call,
        call_args(&write_call),
        &mut session,
    )
    .await
    .expect("write existing file");

    assert!(result.is_error);
    assert_eq!(
        result.error_code.as_deref(),
        Some("workspace.write_requires_read")
    );
    assert_eq!(
        (&repository as &dyn WorkspaceFs)
            .read_text(&WorkspacePath::parse("output/main.md").unwrap())
            .await
            .expect("read file")
            .text,
        "old text"
    );

    let read_call = workspace_call("workspace.read_file", json!({ "path": "output/main.md" }));
    read_file(&repository, &read_call, call_args(&read_call), &mut session)
        .await
        .expect("read file");
    let (result, effect) = write_file(
        &repository,
        &write_call,
        call_args(&write_call),
        &mut session,
    )
    .await
    .expect("write after read");

    assert!(!result.is_error);
    assert!(matches!(
        effect,
        crate::services::agent_tools::AgentToolEffect::WorkspaceFileWritten {
            mode: WorkspaceFileWriteMode::Replace,
            ..
        }
    ));
}

#[tokio::test]
async fn workspace_patch_partial_failure_requires_full_read_before_retry() {
    let repository = TestWorkspaceFs::with_file("output/main.md", "alpha beta\ngamma");
    let mut session = AgentToolSession::default();

    let partial_read = workspace_call(
        "workspace.read_file",
        json!({
            "path": "output/main.md",
            "start_line": 1,
            "line_count": 1
        }),
    );
    let full_read = workspace_call("workspace.read_file", json!({ "path": "output/main.md" }));
    let missing_patch = workspace_call(
        "workspace.apply_patch",
        json!({
            "path": "output/main.md",
            "old_string": "delta",
            "new_string": "omega"
        }),
    );
    let patch = workspace_call(
        "workspace.apply_patch",
        json!({
            "path": "output/main.md",
            "old_string": "alpha",
            "new_string": "omega"
        }),
    );

    read_file(
        &repository,
        &partial_read,
        call_args(&partial_read),
        &mut session,
    )
    .await
    .expect("partial read");

    let (result, _) = apply_patch(
        &repository,
        &missing_patch,
        call_args(&missing_patch),
        &mut session,
    )
    .await
    .expect("patch miss");
    assert_eq!(
        result.error_code.as_deref(),
        Some("workspace.patch_requires_full_read")
    );

    let (result, _) = apply_patch(&repository, &patch, call_args(&patch), &mut session)
        .await
        .expect("patch blocked after partial failure");
    assert_eq!(
        result.error_code.as_deref(),
        Some("workspace.patch_requires_full_read")
    );

    read_file(&repository, &full_read, call_args(&full_read), &mut session)
        .await
        .expect("full read");
    let (result, _) = apply_patch(&repository, &patch, call_args(&patch), &mut session)
        .await
        .expect("patch after full read");

    assert!(!result.is_error);
    assert_eq!(
        (&repository as &dyn WorkspaceFs)
            .read_text(&WorkspacePath::parse("output/main.md").unwrap())
            .await
            .expect("read patched file")
            .text,
        "omega beta\ngamma"
    );
}

fn workspace_call(name: &str, arguments: serde_json::Value) -> ToolInvocation {
    ToolInvocation {
        call_id: format!("call_{}", name.replace('.', "_")),
        tool_id: ToolId::builtin(name).unwrap(),
        arguments: ToolArguments::decode(Some(&arguments)),
        provider_metadata: serde_json::Value::Null,
    }
}

fn call_args(call: &ToolInvocation) -> &Map<String, Value> {
    call.arguments.as_map().expect("test arguments are objects")
}

struct TestWorkspaceFs {
    files: Mutex<HashMap<String, String>>,
}

impl TestWorkspaceFs {
    fn with_file(path: &str, text: &str) -> ScopedWorkspaceFs {
        let inner = Self {
            files: Mutex::new(HashMap::from([(path.to_string(), text.to_string())])),
        };
        ScopedWorkspaceFs::new(
            Arc::new(inner),
            WorkspaceAccessPolicy {
                visible_roots: vec!["output".to_string()],
                writable_roots: vec!["output".to_string()],
            },
        )
    }
}

#[async_trait]
impl WorkspaceFs for TestWorkspaceFs {
    async fn write_file(
        &self,
        path: &WorkspacePath,
        bytes: &[u8],
        guard: WorkspaceWriteGuard,
    ) -> Result<(), DomainError> {
        let text = std::str::from_utf8(bytes).unwrap();
        let mut files = self.files.lock().expect("workspace files lock");
        let current = files.get(path.as_str()).cloned();
        match guard {
            WorkspaceWriteGuard::Unchecked => {}
            WorkspaceWriteGuard::MustNotExist => {
                if let Some(current) = current {
                    return Err(DomainError::workspace_write_conflict(
                        path.as_str(),
                        WorkspaceWriteConflictKind::AlreadyExists {
                            actual_sha256: sha256_hex(current.as_bytes()),
                        },
                    ));
                }
            }
            WorkspaceWriteGuard::MustMatchSha256(expected_sha256) => {
                let actual_sha256 = current.as_deref().map(|text| sha256_hex(text.as_bytes()));
                if actual_sha256.as_deref() != Some(expected_sha256.as_str()) {
                    return Err(DomainError::workspace_write_conflict(
                        path.as_str(),
                        WorkspaceWriteConflictKind::Stale {
                            expected_sha256,
                            actual_sha256,
                        },
                    ));
                }
            }
        }
        files.insert(path.as_str().to_string(), text.to_string());
        Ok(())
    }

    async fn append_text(
        &self,
        _path: &WorkspacePath,
        _text: &str,
    ) -> Result<WorkspaceAppendResult, DomainError> {
        unreachable!("append is covered by real repository and runtime tests")
    }

    async fn read_file(
        &self,
        path: &WorkspacePath,
        _maximum_bytes: usize,
    ) -> Result<Vec<u8>, DomainError> {
        self.files
            .lock()
            .unwrap()
            .get(path.as_str())
            .map(|text| text.as_bytes().to_vec())
            .ok_or_else(|| DomainError::NotFound(path.as_str().to_owned()))
    }
    async fn metadata(
        &self,
        path: Option<&WorkspacePath>,
    ) -> Result<WorkspaceMetadata, DomainError> {
        let files = self.files.lock().unwrap();
        let text = path.and_then(|path| files.get(path.as_str()));
        if let Some(path) = path
            && text.is_none()
            && path.as_str().contains('/')
            && !files
                .keys()
                .any(|name| name.starts_with(&format!("{}/", path.as_str())))
        {
            return Err(DomainError::NotFound(path.as_str().to_owned()));
        }
        Ok(WorkspaceMetadata {
            kind: if text.is_some() {
                WorkspaceEntryKind::File
            } else {
                WorkspaceEntryKind::Directory
            },
            bytes: text.map_or(0, |t| t.len() as u64),
            modified: None,
            created: None,
        })
    }
    async fn create_dir(&self, _path: &WorkspacePath, _recursive: bool) -> Result<(), DomainError> {
        unreachable!("these fixtures have existing parent directories")
    }
    async fn read_dir(
        &self,
        _path: Option<&WorkspacePath>,
        _maximum_entries: usize,
    ) -> Result<Vec<WorkspaceDirectoryEntry>, DomainError> {
        unreachable!("these tests read named files")
    }
    async fn append_file(&self, _path: &WorkspacePath, _bytes: &[u8]) -> Result<(), DomainError> {
        unreachable!("these tests edit text")
    }
    async fn remove(&self, path: &WorkspacePath, recursive: bool) -> Result<(), DomainError> {
        assert!(!recursive);
        self.files.lock().unwrap().remove(path.as_str()).unwrap();
        Ok(())
    }
    async fn rename(
        &self,
        _source: &WorkspacePath,
        _target: &WorkspacePath,
    ) -> Result<(), DomainError> {
        unreachable!()
    }
    async fn copy_file(
        &self,
        _source: &WorkspacePath,
        _target: &WorkspacePath,
    ) -> Result<(), DomainError> {
        unreachable!()
    }
}

#[tokio::test]
async fn workspace_write_recreates_a_removed_file_despite_old_read_state() {
    let workspace = TestWorkspaceFs::with_file("output/main.md", "old");
    let path = WorkspacePath::parse("output/main.md").unwrap();
    let mut session = AgentToolSession::default();
    let read = workspace_call("workspace.read_file", json!({"path": path.as_str()}));
    read_file(&workspace, &read, call_args(&read), &mut session)
        .await
        .unwrap();
    workspace.remove(&path, false).await.unwrap();
    let write = workspace_call(
        "workspace.write_file",
        json!({"path": path.as_str(), "content": "new"}),
    );
    let (result, _) = write_file(&workspace, &write, call_args(&write), &mut session)
        .await
        .unwrap();
    assert!(!result.is_error, "{}", result.content);
    assert_eq!(
        (&workspace as &dyn WorkspaceFs)
            .read_text(&path)
            .await
            .unwrap()
            .text,
        "new"
    );
}
