use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use chrono::Utc;
use serde_json::json;
use sha2::{Digest, Sha256};

use super::args::{classify_workspace_io_error, optional_list_path_arg};
use super::policy::WorkspaceAccessPolicy;
use super::{apply_patch, read_file, write_file};
use crate::services::agent_tools::AgentToolSession;
use tt_domain::errors::{DomainError, WorkspaceWriteConflictKind};
use tt_domain::models::agent::profile::ResolvedAgentProfile;
use tt_domain::models::agent::{
    AgentChatRef, AgentRun, ArtifactTarget, CommitPolicy, WorkspaceFileWriteMode,
    WorkspaceInputManifest, WorkspaceManifest, WorkspacePath, WorkspacePersistentChangeSet,
    WorkspaceRootCommit, WorkspaceRootLifecycle, WorkspaceRootMount, WorkspaceRootScope,
};
use tt_domain::models::agent::{AgentToolCall, WorkspaceRootSpec};
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
    let repository = TestWorkspaceRepository::with_file("output/main.md", "alpha beta gamma");
    let mut session = AgentToolSession::default();

    read_file(
        &repository,
        "run",
        &workspace_call(
            "workspace.read_file",
            json!({
                "path": "output/main.md",
                "start_char": 0,
                "max_chars": 5
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
        "omega beta gamma"
    );
}

fn workspace_call(name: &str, arguments: serde_json::Value) -> AgentToolCall {
    AgentToolCall {
        id: format!("call_{}", name.replace('.', "_")),
        name: name.to_string(),
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
    format!("{digest:x}")
}
