use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use chrono::Utc;
use serde_json::json;
use sha2::{Digest, Sha256};

use super::args::classify_workspace_io_error;
use super::policy::WorkspaceAccessPolicy;
use super::{MAX_READ_CHARS, MAX_READ_LINES, apply_patch, read_file, write_file};
use crate::services::agent_tools::{AgentToolEffect, AgentToolSession};
use crate::services::hashing::hex_lower;
use tt_domain::errors::{DomainError, WorkspaceWriteConflictKind};
use tt_domain::models::agent::WorkspaceRootSpec;
use tt_domain::models::agent::profile::ResolvedAgentProfile;
use tt_domain::models::agent::{
    AgentChatRef, AgentRun, ArtifactTarget, CommitPolicy, WorkspaceFileWriteMode,
    WorkspaceInputManifest, WorkspaceManifest, WorkspacePath, WorkspacePersistentChangeSet,
    WorkspaceRootCommit, WorkspaceRootLifecycle, WorkspaceRootMount, WorkspaceRootScope,
};
use tt_domain::models::tool::{ToolId, ToolInvocation};
use tt_ports::repositories::workspace_repository::{
    WorkspaceAppendResult, WorkspaceEntry, WorkspaceFile, WorkspaceFileList, WorkspaceRepository,
    WorkspaceWriteGuard,
};

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

fn make_test_tool_call(name: &str) -> ToolInvocation {
    ToolInvocation {
        call_id: "call_test".to_string(),
        tool_id: ToolId::builtin(name).unwrap(),
        arguments: json!({}),
        provider_metadata: json!({}),
    }
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

#[tokio::test]
async fn workspace_read_invalid_path_returns_canonical_tool_error() {
    let repository = TestWorkspaceRepository::with_file("output/main.md", "existing");
    let mut session = AgentToolSession::default();
    let call = workspace_call("workspace.read_file", json!({ "path": "../secrets.json" }));

    let (result, effect) = read_file(&repository, "run", &call, &mut session)
        .await
        .expect("invalid model path must remain recoverable");

    let message = "Invalid data: Workspace path cannot contain ..";
    assert!(matches!(effect, AgentToolEffect::None));
    assert_eq!(result.call_id, call.call_id);
    assert_eq!(result.tool_id, call.tool_id);
    assert_eq!(result.content, message);
    assert_eq!(
        result.structured,
        json!({
            "error": {
                "code": "workspace.invalid_path",
                "message": message,
            }
        })
    );
    assert!(result.is_error);
    assert_eq!(result.error_code.as_deref(), Some("workspace.invalid_path"));
    assert!(result.resource_refs.is_empty());
}

#[tokio::test]
async fn workspace_read_hidden_path_returns_recoverable_tool_error() {
    let repository = TestWorkspaceRepository::with_file("output/main.md", "existing");
    let mut session = AgentToolSession::default();
    let call = workspace_call(
        "workspace.read_file",
        json!({ "path": "input/prompt_snapshot.json" }),
    );

    let (result, effect) = read_file(&repository, "run", &call, &mut session)
        .await
        .expect("hidden model path must remain recoverable");

    assert!(matches!(effect, AgentToolEffect::None));
    assert!(result.is_error);
    assert_eq!(
        result.error_code.as_deref(),
        Some("workspace.path_not_visible")
    );
    assert_eq!(
        result.content,
        "Permission denied: agent.workspace_read_denied: path `input/prompt_snapshot.json` is not visible in the current workspace policy"
    );
}

#[tokio::test]
async fn workspace_read_defaults_to_a_preview_for_oversized_files() {
    let text = (1..=MAX_READ_LINES + 1)
        .map(|line| format!("line {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    let repository = TestWorkspaceRepository::with_file("output/large.md", &text);
    let mut session = AgentToolSession::default();
    let call = workspace_call("workspace.read_file", json!({ "path": "output/large.md" }));

    let (result, _) = read_file(&repository, "run", &call, &mut session)
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
    let repository = TestWorkspaceRepository::with_file("output/large.txt", &text);
    let mut session = AgentToolSession::default();
    let call = workspace_call("workspace.read_file", json!({ "path": "output/large.txt" }));

    let (result, _) = read_file(&repository, "run", &call, &mut session)
        .await
        .expect("large read should return a preview");

    assert!(!result.is_error);
    assert_eq!(result.structured["lineTruncated"], true);
    assert_eq!(result.structured["fullRead"], false);
    assert!(result.content.contains("only its beginning is shown"));
}

#[tokio::test]
async fn workspace_write_root_returns_recoverable_tool_error() {
    let repository = TestWorkspaceRepository::with_file("output/main.md", "existing");
    let mut session = AgentToolSession::default();
    let call = workspace_call(
        "workspace.write_file",
        json!({
            "path": "output",
            "content": "replacement",
        }),
    );

    let (result, effect) = write_file(&repository, "run", &call, &mut session)
        .await
        .expect("non-writable model path must remain recoverable");

    assert!(matches!(effect, AgentToolEffect::None));
    assert!(result.is_error);
    assert_eq!(
        result.error_code.as_deref(),
        Some("workspace.path_not_writable")
    );
    assert_eq!(
        result.content,
        "Permission denied: agent.workspace_write_denied: path `output` is not writable in the current workspace policy"
    );
}

#[tokio::test]
async fn workspace_write_existing_file_requires_prior_read() {
    let repository = TestWorkspaceRepository::with_file("output/main.md", "old text");
    let mut session = AgentToolSession::default();

    let (result, _) = write_file(
        &repository,
        "run",
        &workspace_call(
            "workspace.write_file",
            json!({
                "path": "output/main.md",
                "content": "new text",
            }),
        ),
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
        repository
            .read_text("run", &WorkspacePath::parse("output/main.md").unwrap())
            .await
            .expect("read file")
            .text,
        "old text"
    );

    let read_call = workspace_call("workspace.read_file", json!({ "path": "output/main.md" }));
    read_file(&repository, "run", &read_call, &mut session)
        .await
        .expect("read file");
    let (result, effect) = write_file(
        &repository,
        "run",
        &workspace_call(
            "workspace.write_file",
            json!({
                "path": "output/main.md",
                "content": "new text",
            }),
        ),
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
    let repository = TestWorkspaceRepository::with_file("output/main.md", "alpha beta\ngamma");
    let mut session = AgentToolSession::default();

    read_file(
        &repository,
        "run",
        &workspace_call(
            "workspace.read_file",
            json!({
                "path": "output/main.md",
                "start_line": 1,
                "line_count": 1
            }),
        ),
        &mut session,
    )
    .await
    .expect("partial read");

    let (result, _) = apply_patch(
        &repository,
        "run",
        &workspace_call(
            "workspace.apply_patch",
            json!({
                "path": "output/main.md",
                "old_string": "delta",
                "new_string": "omega"
            }),
        ),
        &mut session,
    )
    .await
    .expect("patch miss");
    assert_eq!(
        result.error_code.as_deref(),
        Some("workspace.patch_requires_full_read")
    );

    let (result, _) = apply_patch(
        &repository,
        "run",
        &workspace_call(
            "workspace.apply_patch",
            json!({
                "path": "output/main.md",
                "old_string": "alpha",
                "new_string": "omega"
            }),
        ),
        &mut session,
    )
    .await
    .expect("patch blocked after partial failure");
    assert_eq!(
        result.error_code.as_deref(),
        Some("workspace.patch_requires_full_read")
    );

    read_file(
        &repository,
        "run",
        &workspace_call("workspace.read_file", json!({ "path": "output/main.md" })),
        &mut session,
    )
    .await
    .expect("full read");
    let (result, _) = apply_patch(
        &repository,
        "run",
        &workspace_call(
            "workspace.apply_patch",
            json!({
                "path": "output/main.md",
                "old_string": "alpha",
                "new_string": "omega"
            }),
        ),
        &mut session,
    )
    .await
    .expect("patch after full read");

    assert!(!result.is_error);
    assert_eq!(
        repository
            .read_text("run", &WorkspacePath::parse("output/main.md").unwrap())
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
        arguments,
        provider_metadata: serde_json::Value::Null,
    }
}

struct TestWorkspaceRepository {
    files: Mutex<HashMap<String, String>>,
}

impl TestWorkspaceRepository {
    fn with_file(path: &str, text: &str) -> Self {
        Self {
            files: Mutex::new(HashMap::from([(path.to_string(), text.to_string())])),
        }
    }

    fn workspace_file(path: &WorkspacePath, text: &str) -> WorkspaceFile {
        WorkspaceFile {
            path: path.clone(),
            text: text.to_string(),
            bytes: text.len() as u64,
            sha256: sha256_hex(text),
        }
    }
}

#[async_trait]
impl WorkspaceRepository for TestWorkspaceRepository {
    async fn initialize_run(
        &self,
        _run: &AgentRun,
        _manifest: &WorkspaceManifest,
        _prompt_snapshot: &serde_json::Value,
        _resolved_profile: &ResolvedAgentProfile,
    ) -> Result<(), DomainError> {
        Ok(())
    }

    async fn read_manifest(&self, _run_id: &str) -> Result<WorkspaceManifest, DomainError> {
        Ok(test_manifest())
    }

    async fn write_text(
        &self,
        run_id: &str,
        path: &WorkspacePath,
        text: &str,
    ) -> Result<WorkspaceFile, DomainError> {
        self.write_text_guarded(run_id, path, text, WorkspaceWriteGuard::Unchecked)
            .await
    }

    async fn write_text_guarded(
        &self,
        _run_id: &str,
        path: &WorkspacePath,
        text: &str,
        guard: WorkspaceWriteGuard,
    ) -> Result<WorkspaceFile, DomainError> {
        let mut files = self.files.lock().expect("workspace files lock");
        let current = files.get(path.as_str()).cloned();
        match guard {
            WorkspaceWriteGuard::Unchecked => {}
            WorkspaceWriteGuard::MustNotExist => {
                if let Some(current) = current {
                    return Err(DomainError::workspace_write_conflict(
                        path.as_str(),
                        WorkspaceWriteConflictKind::AlreadyExists {
                            actual_sha256: sha256_hex(&current),
                        },
                    ));
                }
            }
            WorkspaceWriteGuard::MustMatchSha256(expected_sha256) => {
                let actual_sha256 = current.as_deref().map(sha256_hex);
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
        Ok(Self::workspace_file(path, text))
    }

    async fn append_text(
        &self,
        _run_id: &str,
        path: &WorkspacePath,
        text: &str,
    ) -> Result<WorkspaceAppendResult, DomainError> {
        let mut files = self.files.lock().expect("workspace files lock");
        let previous = files.get(path.as_str()).cloned();
        let mut next = previous.clone().unwrap_or_default();
        next.push_str(text);
        files.insert(path.as_str().to_string(), next.clone());
        Ok(WorkspaceAppendResult {
            file: Self::workspace_file(path, &next),
            previous_sha256: previous.as_deref().map(sha256_hex),
        })
    }

    async fn read_text(
        &self,
        _run_id: &str,
        path: &WorkspacePath,
    ) -> Result<WorkspaceFile, DomainError> {
        let files = self.files.lock().expect("workspace files lock");
        let text = files.get(path.as_str()).ok_or_else(|| {
            DomainError::NotFound(format!("Workspace file not found: {}", path.as_str()))
        })?;
        Ok(Self::workspace_file(path, text))
    }

    async fn list_files(
        &self,
        _run_id: &str,
        _path: Option<&WorkspacePath>,
        _depth: usize,
        _max_entries: usize,
    ) -> Result<WorkspaceFileList, DomainError> {
        Ok(WorkspaceFileList {
            entries: Vec::<WorkspaceEntry>::new(),
            truncated: false,
        })
    }

    async fn commit_persistent_changes(
        &self,
        _run_id: &str,
    ) -> Result<WorkspacePersistentChangeSet, DomainError> {
        Ok(WorkspacePersistentChangeSet {
            state_id: "state".to_string(),
            base_state_id: None,
            changes: Vec::new(),
        })
    }
}

fn test_manifest() -> WorkspaceManifest {
    WorkspaceManifest {
        workspace_version: 1,
        run_id: "run".to_string(),
        stable_chat_id: "stable".to_string(),
        chat_ref: AgentChatRef::Character {
            character_id: "Alice".to_string(),
            file_name: "Alice.png".to_string(),
        },
        created_at: Utc::now(),
        input: WorkspaceInputManifest {
            mode: "snapshot".to_string(),
            prompt_snapshot_path: "input/prompt_snapshot.json".to_string(),
            resolved_profile_path: "input/resolved_profile.json".to_string(),
        },
        roots: vec![WorkspaceRootSpec {
            path: "output".to_string(),
            lifecycle: WorkspaceRootLifecycle::Run,
            scope: WorkspaceRootScope::Run,
            mount: WorkspaceRootMount::Materialized,
            visible: true,
            writable: true,
            commit: WorkspaceRootCommit::Never,
        }],
        artifacts: Vec::new(),
        commit_policy: CommitPolicy {
            default_target: ArtifactTarget::MessageBody,
            combine_template: None,
            store_artifacts_in_extra: false,
        },
    }
}

fn sha256_hex(text: &str) -> String {
    let digest = Sha256::digest(text.as_bytes());
    hex_lower(&digest)
}
