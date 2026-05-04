use std::path::PathBuf;

use chrono::Utc;
use serde_json::Value;
use tokio::fs;
use uuid::Uuid;

use super::FileAgentRepository;
use crate::domain::models::agent::plan::{AgentPlanMode, AgentPlanPolicy};
use crate::domain::models::agent::profile::{
    AGENT_PROFILE_KIND, AGENT_PROFILE_SCHEMA_VERSION, AgentModelBinding, AgentModelBindingMode,
    AgentPresetBinding, AgentPresetBindingMode, AgentProfileId, AgentProfileInstructions,
    AgentProfileSourceTrace, AgentRunPolicy, AgentSkillPolicy, AgentToolPolicy,
    AgentWorkspacePolicy, ResolvedAgentOutputPolicy, ResolvedAgentProfile,
};
use crate::domain::models::agent::{
    AgentChatRef, AgentRun, AgentRunEventLevel, AgentRunPresentation, AgentRunStatus, ArtifactSpec,
    ArtifactTarget, CommitPolicy, WorkspaceInputManifest, WorkspaceManifest, WorkspacePath,
    WorkspaceRootCommit, WorkspaceRootLifecycle, WorkspaceRootMount, WorkspaceRootScope,
    WorkspaceRootSpec,
};
use crate::domain::repositories::agent_run_repository::{
    AgentRunEventReadQuery, AgentRunRepository,
};
use crate::domain::repositories::agent_workspace_lifecycle_repository::AgentWorkspaceLifecycleRepository;
use crate::domain::repositories::checkpoint_repository::CheckpointRepository;
use crate::domain::repositories::workspace_repository::WorkspaceRepository;
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
        },
        run: AgentRunPolicy {
            presentation: AgentRunPresentation::Background,
            model_retry: Default::default(),
        },
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

    let changes = repository
        .prepare_persistent_changes(&run.id)
        .await
        .expect("prepare persist changes");
    assert_eq!(changes.changes.len(), 1);
    assert_eq!(changes.changes[0].path, "persist/MEMORY.md");

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

    repository
        .commit_persistent_changes(&run.id)
        .await
        .expect("commit persist changes");

    let next_run = sample_run_with_id("run_persist_next");
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
async fn persistent_workspace_detects_conflicting_parallel_runs() {
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
    let error = repository
        .prepare_persistent_changes(&second.id)
        .await
        .expect_err("second run must conflict");
    assert!(error.to_string().contains("persistent_workspace_conflict"));

    fs::remove_dir_all(root).await.expect("cleanup");
}
