use std::path::PathBuf;

use chrono::Utc;
use serde_json::Value;
use tokio::fs;
use uuid::Uuid;

use super::FileAgentRepository;
use crate::domain::errors::DomainError;
use crate::domain::models::agent::plan::{AgentPlanMode, AgentPlanPolicy};
use crate::domain::models::agent::profile::{
    AGENT_PROFILE_KIND, AGENT_PROFILE_SCHEMA_VERSION, AgentContextPolicy, AgentDelegationPolicy,
    AgentModelBinding, AgentModelBindingMode, AgentPresetBinding, AgentPresetBindingMode,
    AgentProfileId, AgentProfileInstructions, AgentProfileSourceTrace, AgentRunPolicy,
    AgentSkillPolicy, AgentToolPolicy, AgentWorkspacePolicy, ResolvedAgentOutputPolicy,
    ResolvedAgentProfile,
};
use crate::domain::models::agent::{
    AgentChatRef, AgentInvocation, AgentInvocationExitPolicy, AgentInvocationKind,
    AgentInvocationStatus, AgentRun, AgentRunEventLevel, AgentRunPresentation, AgentRunStatus,
    ArtifactSpec, ArtifactTarget, CommitPolicy, WorkspaceInputManifest, WorkspaceManifest,
    WorkspacePath, WorkspaceRootCommit, WorkspaceRootLifecycle, WorkspaceRootMount,
    WorkspaceRootScope, WorkspaceRootSpec,
};
use crate::domain::repositories::agent_invocation_repository::AgentInvocationRepository;
use crate::domain::repositories::agent_run_repository::{
    AgentRunEventReadQuery, AgentRunRepository,
};
use crate::domain::repositories::agent_workspace_lifecycle_repository::AgentWorkspaceLifecycleRepository;
use crate::domain::repositories::checkpoint_repository::CheckpointRepository;
use crate::domain::repositories::workspace_repository::{WorkspaceRepository, WorkspaceWriteGuard};
fn temp_root() -> PathBuf {
    std::env::temp_dir().join(format!("tauritavern-agent-repo-{}", Uuid::new_v4()))
}

fn sample_run() -> AgentRun {
    sample_run_with_id("run_test")
}

fn sample_run_with_id(id: &str) -> AgentRun {
    AgentRun {
        id: id.to_string(),
        workspace_id: "chat_test".to_string(),
        stable_chat_id: "stable_chat_test".to_string(),
        chat_ref: AgentChatRef::Character {
            character_id: "Seraphina".to_string(),
            file_name: "Seraphina.png".to_string(),
        },
        generation_type: "normal".to_string(),
        profile_id: None,
        skill_scope_refs: Default::default(),
        persist_base_state_id: None,
        input_message_count: None,
        presentation: AgentRunPresentation::Background,
        status: AgentRunStatus::Created,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

fn sample_manifest(run: &AgentRun) -> WorkspaceManifest {
    WorkspaceManifest {
        workspace_version: 1,
        run_id: run.id.clone(),
        stable_chat_id: run.stable_chat_id.clone(),
        chat_ref: run.chat_ref.clone(),
        created_at: Utc::now(),
        input: WorkspaceInputManifest {
            mode: "prompt_snapshot".to_string(),
            prompt_snapshot_path: "input/prompt_snapshot.json".to_string(),
            resolved_profile_path: "input/resolved_profile.json".to_string(),
        },
        roots: vec![
            WorkspaceRootSpec {
                path: "output".to_string(),
                lifecycle: WorkspaceRootLifecycle::Run,
                scope: WorkspaceRootScope::Run,
                mount: WorkspaceRootMount::Materialized,
                visible: true,
                writable: true,
                commit: WorkspaceRootCommit::Never,
            },
            WorkspaceRootSpec {
                path: "persist".to_string(),
                lifecycle: WorkspaceRootLifecycle::Persistent,
                scope: WorkspaceRootScope::Chat,
                mount: WorkspaceRootMount::ProjectedOverlay,
                visible: true,
                writable: true,
                commit: WorkspaceRootCommit::OnRunCompleted,
            },
        ],
        artifacts: vec![ArtifactSpec {
            id: "main".to_string(),
            path: "output/main.md".to_string(),
            kind: "markdown".to_string(),
            target: ArtifactTarget::MessageBody,
            required: true,
            assembly_order: 0,
        }],
        commit_policy: CommitPolicy {
            default_target: ArtifactTarget::MessageBody,
            combine_template: None,
            store_artifacts_in_extra: true,
        },
    }
}

fn sample_resolved_profile(manifest: &WorkspaceManifest) -> ResolvedAgentProfile {
    ResolvedAgentProfile {
        schema_version: AGENT_PROFILE_SCHEMA_VERSION,
        kind: AGENT_PROFILE_KIND.to_string(),
        id: AgentProfileId::parse("test-profile").expect("profile id"),
        display_name: "Test Profile".to_string(),
        description: None,
        preset: AgentPresetBinding {
            mode: AgentPresetBindingMode::CurrentPromptSnapshot,
            ref_: None,
            required: false,
        },
        model: AgentModelBinding {
            mode: AgentModelBindingMode::CurrentPromptSnapshot,
            connection_ref: None,
            model_id: None,
        },
        run: AgentRunPolicy {
            presentation: AgentRunPresentation::Background,
            direct_runnable: true,
            model_retry: Default::default(),
        },
        context: AgentContextPolicy::default(),
        delegation: AgentDelegationPolicy::default(),
        instructions: AgentProfileInstructions {
            agent_system_prompt: None,
        },
        tools: AgentToolPolicy {
            allow: Vec::new(),
            deny: Vec::new(),
            tool_descriptions: Default::default(),
            max_rounds: 1,
            max_calls_per_run: 1,
            max_calls_per_tool: Default::default(),
        },
        skills: AgentSkillPolicy {
            visible: vec!["*".to_string()],
            deny: Vec::new(),
            max_read_chars_per_call: 1,
            max_read_chars_per_run: 1,
        },
        workspace: AgentWorkspacePolicy {
            visible_roots: manifest
                .roots
                .iter()
                .map(|root| root.path.clone())
                .collect(),
            writable_roots: manifest
                .roots
                .iter()
                .filter(|root| root.writable)
                .map(|root| root.path.clone())
                .collect(),
        },
        plan: AgentPlanPolicy {
            mode: AgentPlanMode::None,
            beta: true,
            nodes: Vec::new(),
        },
        output: ResolvedAgentOutputPolicy {
            artifacts: manifest.artifacts.clone(),
            message_body_artifact_id: "main".to_string(),
            message_body_path: "output/main.md".to_string(),
        },
        source_trace: AgentProfileSourceTrace {
            profile_source: "test".to_string(),
        },
    }
}

#[tokio::test]
async fn repository_round_trips_run_workspace_event_and_checkpoint() {
    let root = temp_root();
    let repository = FileAgentRepository::new(root.clone());
    let run = sample_run();
    let manifest = sample_manifest(&run);
    let profile = sample_resolved_profile(&manifest);

    repository.create_run(&run).await.expect("create run");
    repository
        .initialize_run(
            &run,
            &manifest,
            &serde_json::json!({"messages": []}),
            &profile,
        )
        .await
        .expect("initialize workspace");

    let path = WorkspacePath::parse("output/main.md").expect("workspace path");
    let written = repository
        .write_text(&run.id, &path, "hello")
        .await
        .expect("write text");
    assert_eq!(written.sha256.len(), 64);

    let event = repository
        .append_event(
            &run.id,
            AgentRunEventLevel::Info,
            "artifact_written",
            Value::Null,
        )
        .await
        .expect("append event");
    assert_eq!(event.seq, 1);

    let events = repository
        .read_events(
            &run.id,
            AgentRunEventReadQuery {
                after_seq: Some(0),
                before_seq: None,
                limit: 10,
                invocation_id: None,
            },
        )
        .await
        .expect("read events");
    assert_eq!(events.len(), 1);

    let checkpoint = repository
        .create_checkpoint(&run.id, "test", event.seq, &[path])
        .await
        .expect("checkpoint");
    assert_eq!(checkpoint.files[0].bytes, 5);

    fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn guarded_workspace_writes_are_atomic_per_path() {
    let root = temp_root();
    let repository = FileAgentRepository::new(root.clone());
    let run = sample_run_with_id("run_guarded_workspace_write");
    let manifest = sample_manifest(&run);
    let profile = sample_resolved_profile(&manifest);

    repository.create_run(&run).await.expect("create run");
    repository
        .initialize_run(
            &run,
            &manifest,
            &serde_json::json!({"messages": []}),
            &profile,
        )
        .await
        .expect("initialize workspace");

    let path = WorkspacePath::parse("output/main.md").expect("workspace path");
    let seeded = repository
        .write_text(&run.id, &path, "first")
        .await
        .expect("seed text");
    let guard = WorkspaceWriteGuard::MustMatchSha256(seeded.sha256);

    let (left, right) = tokio::join!(
        repository.write_text_guarded(&run.id, &path, "left", guard.clone()),
        repository.write_text_guarded(&run.id, &path, "right", guard),
    );

    let successes = [&left, &right]
        .iter()
        .filter(|result| result.is_ok())
        .count();
    let conflicts = [&left, &right]
        .iter()
        .filter(|result| matches!(result, Err(DomainError::WorkspaceWriteConflict { .. })))
        .count();
    assert_eq!(successes, 1);
    assert_eq!(conflicts, 1);

    let final_text = repository
        .read_text(&run.id, &path)
        .await
        .expect("read final text")
        .text;
    assert!(final_text == "left" || final_text == "right");

    fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn append_text_is_atomic_per_path_and_creates_missing_files() {
    let root = temp_root();
    let repository = FileAgentRepository::new(root.clone());
    let run = sample_run_with_id("run_append_workspace_write");
    let manifest = sample_manifest(&run);
    let profile = sample_resolved_profile(&manifest);

    repository.create_run(&run).await.expect("create run");
    repository
        .initialize_run(
            &run,
            &manifest,
            &serde_json::json!({"messages": []}),
            &profile,
        )
        .await
        .expect("initialize workspace");

    let path = WorkspacePath::parse("output/main.md").expect("workspace path");
    let created = repository
        .append_text(&run.id, &path, "first")
        .await
        .expect("append missing file");
    assert_eq!(created.previous_sha256, None);
    assert_eq!(created.file.text, "first");

    let (left, right) = tokio::join!(
        repository.append_text(&run.id, &path, " left"),
        repository.append_text(&run.id, &path, " right"),
    );
    assert!(left.expect("append left").previous_sha256.is_some());
    assert!(right.expect("append right").previous_sha256.is_some());

    let final_text = repository
        .read_text(&run.id, &path)
        .await
        .expect("read final text")
        .text;
    assert!(
        final_text == "first left right" || final_text == "first right left",
        "unexpected final text: {final_text}"
    );

    fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn repository_round_trips_invocations() {
    let root = temp_root();
    let repository = FileAgentRepository::new(root.clone());
    let run = sample_run_with_id("run_invocation");
    repository.create_run(&run).await.expect("create run");

    assert!(
        repository
            .try_load_invocation(&run.id, "inv_root")
            .await
            .expect("try load missing invocation")
            .is_none()
    );
    assert!(matches!(
        repository.load_invocation(&run.id, "inv_root").await,
        Err(DomainError::NotFound(_))
    ));

    let now = Utc::now();
    let invocation = AgentInvocation {
        id: "inv_root".to_string(),
        run_id: run.id.clone(),
        parent_invocation_id: None,
        profile_id: "default-writer".to_string(),
        kind: AgentInvocationKind::Root,
        status: AgentInvocationStatus::Running,
        exit_policy: AgentInvocationExitPolicy::RunFinishAllowed,
        created_at: now,
        updated_at: now,
    };
    repository
        .save_invocation(&invocation)
        .await
        .expect("save invocation");
    let loaded = repository
        .load_invocation(&run.id, "inv_root")
        .await
        .expect("load invocation");
    assert_eq!(loaded.profile_id, "default-writer");
    let loaded_optional = repository
        .try_load_invocation(&run.id, "inv_root")
        .await
        .expect("try load invocation")
        .expect("invocation exists");
    assert_eq!(loaded_optional.profile_id, "default-writer");
    assert_eq!(repository.list_invocations(&run.id).await.unwrap().len(), 1);

    fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn read_text_on_directory_returns_typed_workspace_error() {
    // Issue #54: workspace_read_file used to bubble up the raw EISDIR
    // ("Is a directory") OS error as `agent.internal_error` (retryable=false)
    // and tear down the whole run. We now translate it into a structured
    // domain error so the tool layer can surface it as a recoverable
    // `workspace.path_is_directory` business error.
    let root = temp_root();
    let repository = FileAgentRepository::new(root.clone());
    let run = sample_run_with_id("run_dir_read");
    let manifest = sample_manifest(&run);
    let profile = sample_resolved_profile(&manifest);

    repository.create_run(&run).await.expect("create run");
    repository
        .initialize_run(
            &run,
            &manifest,
            &serde_json::json!({"messages": []}),
            &profile,
        )
        .await
        .expect("initialize workspace");

    let persist_path = WorkspacePath::parse("persist").expect("persist root path");
    let error = repository
        .read_text(&run.id, &persist_path)
        .await
        .expect_err("reading a directory must fail");

    match error {
        DomainError::WorkspacePathIsDirectory { path } => {
            assert_eq!(path, "persist");
        }
        other => panic!("expected DomainError::WorkspacePathIsDirectory, got {other:?}"),
    }

    fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn write_text_on_directory_returns_typed_workspace_error() {
    // Same guard for write_text so workspace_write_file cannot wipe out a
    // directory through the temp-file swap path.
    let root = temp_root();
    let repository = FileAgentRepository::new(root.clone());
    let run = sample_run_with_id("run_dir_write");
    let manifest = sample_manifest(&run);
    let profile = sample_resolved_profile(&manifest);

    repository.create_run(&run).await.expect("create run");
    repository
        .initialize_run(
            &run,
            &manifest,
            &serde_json::json!({"messages": []}),
            &profile,
        )
        .await
        .expect("initialize workspace");

    let output_root = WorkspacePath::parse("output").expect("output root path");
    let error = repository
        .write_text(&run.id, &output_root, "should not land")
        .await
        .expect_err("writing to a directory must fail");

    match error {
        DomainError::WorkspacePathIsDirectory { path } => {
            assert_eq!(path, "output");
        }
        other => panic!("expected DomainError::WorkspacePathIsDirectory, got {other:?}"),
    }

    fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn delete_chat_workspace_removes_runs_and_indexes() {
    let root = temp_root();
    let repository = FileAgentRepository::new(root.clone());
    let first = sample_run_with_id("run_delete_a");
    let second = sample_run_with_id("run_delete_b");

    repository
        .create_run(&first)
        .await
        .expect("create first run");
    repository
        .create_run(&second)
        .await
        .expect("create second run");
    fs::write(
        root.join("chats")
            .join(&first.workspace_id)
            .join("runs")
            .join(".DS_Store"),
        b"finder metadata",
    )
    .await
    .expect("write platform metadata");

    let deletion = repository
        .delete_chat_workspace(&first.workspace_id)
        .await
        .expect("delete chat workspace");

    assert!(deletion.removed);
    assert_eq!(
        deletion.run_ids,
        vec!["run_delete_a".to_string(), "run_delete_b".to_string()]
    );
    assert!(!root.join("chats").join(&first.workspace_id).exists());
    assert!(!root.join("index/runs/run_delete_a.json").exists());
    assert!(!root.join("index/runs/run_delete_b.json").exists());

    fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn prune_persistent_states_ignores_platform_metadata_files() {
    let root = temp_root();
    let repository = FileAgentRepository::new(root.clone());
    let states_dir = root
        .join("chats")
        .join("chat_prune")
        .join("persistent-states");
    fs::create_dir_all(states_dir.join("state_keep"))
        .await
        .expect("create retained state");
    fs::create_dir_all(states_dir.join("state_drop"))
        .await
        .expect("create removed state");
    fs::write(states_dir.join(".DS_Store"), b"finder metadata")
        .await
        .expect("write platform metadata");

    let prune = repository
        .prune_persistent_states("chat_prune", &["state_keep".to_string()])
        .await
        .expect("prune persistent states");

    assert_eq!(prune.removed_state_ids, vec!["state_drop".to_string()]);
    assert!(states_dir.join("state_keep").exists());
    assert!(!states_dir.join("state_drop").exists());
    assert!(states_dir.join(".DS_Store").exists());

    fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn delete_missing_chat_workspace_is_idempotent() {
    let root = temp_root();
    let repository = FileAgentRepository::new(root.clone());

    let deletion = repository
        .delete_chat_workspace("chat_missing")
        .await
        .expect("delete missing chat workspace");

    assert!(!deletion.removed);
    assert!(deletion.run_ids.is_empty());

    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn persistent_workspace_projects_run_changes_only_after_commit() {
    let root = temp_root();
    let repository = FileAgentRepository::new(root.clone());
    let run = sample_run_with_id("run_persist_a");
    let manifest = sample_manifest(&run);
    let profile = sample_resolved_profile(&manifest);

    repository.create_run(&run).await.expect("create run");
    repository
        .initialize_run(
            &run,
            &manifest,
            &serde_json::json!({"messages": []}),
            &profile,
        )
        .await
        .expect("initialize workspace");

    let persist_path = WorkspacePath::parse("persist/MEMORY.md").unwrap();
    repository
        .write_text(&run.id, &persist_path, "long running thread note")
        .await
        .expect("write persist projection");
    fs::write(
        root.join("chats")
            .join(&run.workspace_id)
            .join("runs")
            .join(&run.id)
            .join("persist")
            .join(".DS_Store"),
        b"finder metadata",
    )
    .await
    .expect("write platform metadata");

    let pre_commit_run = sample_run_with_id("run_persist_before_commit");
    repository
        .create_run(&pre_commit_run)
        .await
        .expect("create pre-commit run");
    repository
        .initialize_run(
            &pre_commit_run,
            &sample_manifest(&pre_commit_run),
            &serde_json::json!({"messages": []}),
            &profile,
        )
        .await
        .expect("initialize pre-commit run");
    assert!(
        repository
            .read_text(&pre_commit_run.id, &persist_path)
            .await
            .is_err(),
        "uncommitted persist projection must not leak into another run"
    );

    let changes = repository
        .commit_persistent_changes(&run.id)
        .await
        .expect("commit persist changes");
    assert_eq!(changes.changes.len(), 1);
    assert_eq!(changes.changes[0].path, "persist/MEMORY.md");
    assert!(
        !root
            .join("chats")
            .join(&run.workspace_id)
            .join("persistent-states")
            .join(&run.id)
            .join("persist")
            .join(".DS_Store")
            .exists(),
        "platform metadata must not be committed into persistent state"
    );

    let empty_next_run = sample_run_with_id("run_persist_empty_next");
    repository
        .create_run(&empty_next_run)
        .await
        .expect("create empty next run");
    repository
        .initialize_run(
            &empty_next_run,
            &sample_manifest(&empty_next_run),
            &serde_json::json!({"messages": []}),
            &profile,
        )
        .await
        .expect("initialize empty next run");
    assert!(
        repository
            .read_text(&empty_next_run.id, &persist_path)
            .await
            .is_err(),
        "result-scoped persist must not leak into runs without an explicit base state"
    );

    let mut next_run = sample_run_with_id("run_persist_next");
    next_run.persist_base_state_id = Some(run.id.clone());
    repository
        .create_run(&next_run)
        .await
        .expect("create next run");
    repository
        .initialize_run(
            &next_run,
            &sample_manifest(&next_run),
            &serde_json::json!({"messages": []}),
            &profile,
        )
        .await
        .expect("initialize next run");
    let projected = repository
        .read_text(&next_run.id, &persist_path)
        .await
        .expect("read committed persist projection");
    assert_eq!(projected.text, "long running thread note");

    fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn persistent_workspace_commits_parallel_branch_states() {
    let root = temp_root();
    let repository = FileAgentRepository::new(root.clone());
    let first = sample_run_with_id("run_conflict_a");
    let second = sample_run_with_id("run_conflict_b");
    let persist_path = WorkspacePath::parse("persist/MEMORY.md").unwrap();

    for run in [&first, &second] {
        repository.create_run(run).await.expect("create run");
        let manifest = sample_manifest(run);
        let profile = sample_resolved_profile(&manifest);
        repository
            .initialize_run(
                run,
                &manifest,
                &serde_json::json!({"messages": []}),
                &profile,
            )
            .await
            .expect("initialize run");
    }

    repository
        .write_text(&first.id, &persist_path, "first")
        .await
        .expect("write first projection");
    repository
        .commit_persistent_changes(&first.id)
        .await
        .expect("commit first projection");

    repository
        .write_text(&second.id, &persist_path, "second")
        .await
        .expect("write second projection");
    repository
        .commit_persistent_changes(&second.id)
        .await
        .expect("commit second projection");

    let mut child_of_first = sample_run_with_id("run_conflict_child_first");
    child_of_first.persist_base_state_id = Some(first.id.clone());
    repository
        .create_run(&child_of_first)
        .await
        .expect("create child of first");
    repository
        .initialize_run(
            &child_of_first,
            &sample_manifest(&child_of_first),
            &serde_json::json!({"messages": []}),
            &sample_resolved_profile(&sample_manifest(&child_of_first)),
        )
        .await
        .expect("initialize child of first");
    assert_eq!(
        repository
            .read_text(&child_of_first.id, &persist_path)
            .await
            .expect("read first branch state")
            .text,
        "first"
    );

    let mut child_of_second = sample_run_with_id("run_conflict_child_second");
    child_of_second.persist_base_state_id = Some(second.id.clone());
    repository
        .create_run(&child_of_second)
        .await
        .expect("create child of second");
    repository
        .initialize_run(
            &child_of_second,
            &sample_manifest(&child_of_second),
            &serde_json::json!({"messages": []}),
            &sample_resolved_profile(&sample_manifest(&child_of_second)),
        )
        .await
        .expect("initialize child of second");
    assert_eq!(
        repository
            .read_text(&child_of_second.id, &persist_path)
            .await
            .expect("read second branch state")
            .text,
        "second"
    );

    fs::remove_dir_all(root).await.expect("cleanup");
}
