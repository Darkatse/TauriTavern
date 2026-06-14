use std::path::PathBuf;

use chrono::{DateTime, Utc};
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
    AGENT_RUN_SUMMARY_PROJECTION_SCHEMA_VERSION, AgentChatRef, AgentInvocation,
    AgentInvocationExitPolicy, AgentInvocationKind, AgentInvocationStatus, AgentRun,
    AgentRunCommittedMessageProjection, AgentRunEventLevel, AgentRunPresentation, AgentRunStatus,
    AgentRunSummaryProjection, ArtifactSpec, ArtifactTarget, CommitPolicy, WorkspaceInputManifest,
    WorkspaceManifest, WorkspacePath, WorkspaceRootCommit, WorkspaceRootLifecycle,
    WorkspaceRootMount, WorkspaceRootScope, WorkspaceRootSpec,
};
use crate::domain::repositories::agent_invocation_repository::AgentInvocationRepository;
use crate::domain::repositories::agent_run_repository::{
    AgentRunEventReadQuery, AgentRunListCursor, AgentRunListQuery, AgentRunRepository,
};
use crate::domain::repositories::agent_workspace_lifecycle_repository::{
    AgentPersistentStatePruneRequest, AgentWorkspaceLifecycleRepository,
};
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

fn instant(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value)
        .expect("valid timestamp")
        .with_timezone(&Utc)
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
async fn list_runs_reads_index_sorted_and_cursor_paginated() {
    let root = temp_root();
    let repository = FileAgentRepository::new(root.clone());
    let mut oldest = sample_run_with_id("run_page_oldest");
    let mut tied_a = sample_run_with_id("run_page_a");
    let mut tied_b = sample_run_with_id("run_page_b");
    let mut newest = sample_run_with_id("run_page_newest");

    oldest.created_at = instant("2026-01-01T00:00:00Z");
    oldest.updated_at = oldest.created_at;
    tied_a.created_at = instant("2026-01-02T00:00:00Z");
    tied_a.updated_at = tied_a.created_at;
    tied_b.created_at = tied_a.created_at;
    tied_b.updated_at = tied_b.created_at;
    newest.created_at = instant("2026-01-03T00:00:00Z");
    newest.updated_at = newest.created_at;

    for run in [&oldest, &tied_a, &tied_b, &newest] {
        repository.create_run(run).await.expect("create run");
    }

    let first_page = repository
        .list_runs(AgentRunListQuery {
            chat_ref: None,
            stable_chat_id: None,
            statuses: None,
            before: None,
            limit: 2,
        })
        .await
        .expect("list first page");
    assert_eq!(
        first_page
            .iter()
            .map(|run| run.id.as_str())
            .collect::<Vec<_>>(),
        vec!["run_page_newest", "run_page_b"]
    );

    let second_page = repository
        .list_runs(AgentRunListQuery {
            chat_ref: None,
            stable_chat_id: None,
            statuses: None,
            before: Some(AgentRunListCursor {
                created_at: tied_b.created_at,
                run_id: tied_b.id.clone(),
            }),
            limit: 10,
        })
        .await
        .expect("list second page");
    assert_eq!(
        second_page
            .iter()
            .map(|run| run.id.as_str())
            .collect::<Vec<_>>(),
        vec!["run_page_a", "run_page_oldest"]
    );

    fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn list_runs_filters_by_chat_stable_id_and_status() {
    let root = temp_root();
    let repository = FileAgentRepository::new(root.clone());
    let mut matching = sample_run_with_id("run_filter_match");
    matching.status = AgentRunStatus::Completed;
    matching.stable_chat_id = "stable_filter".to_string();
    matching.profile_id = Some("writer".to_string());

    let mut wrong_status = sample_run_with_id("run_filter_wrong_status");
    wrong_status.status = AgentRunStatus::Failed;
    wrong_status.stable_chat_id = matching.stable_chat_id.clone();

    let mut wrong_chat = sample_run_with_id("run_filter_wrong_chat");
    wrong_chat.status = AgentRunStatus::Completed;
    wrong_chat.stable_chat_id = matching.stable_chat_id.clone();
    wrong_chat.chat_ref = AgentChatRef::Group {
        chat_id: "group_filter".to_string(),
    };

    for run in [&matching, &wrong_status, &wrong_chat] {
        repository.create_run(run).await.expect("create run");
    }

    let listed = repository
        .list_runs(AgentRunListQuery {
            chat_ref: Some(matching.chat_ref.clone()),
            stable_chat_id: Some(matching.stable_chat_id.clone()),
            statuses: Some(vec![AgentRunStatus::Completed]),
            before: None,
            limit: 10,
        })
        .await
        .expect("list filtered runs");

    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, matching.id);
    assert_eq!(listed[0].profile_id.as_deref(), Some("writer"));

    fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn list_runs_accepts_legacy_index_without_presentation() {
    let root = temp_root();
    let repository = FileAgentRepository::new(root.clone());
    let run = sample_run_with_id("run_legacy_without_presentation");
    repository.create_run(&run).await.expect("create run");

    let index_path = root.join("index/runs/run_legacy_without_presentation.json");
    let mut legacy = serde_json::to_value(&run).expect("serialize legacy run");
    legacy
        .as_object_mut()
        .expect("run json object")
        .remove("presentation");
    FileAgentRepository::write_json_atomic(&index_path, &legacy)
        .await
        .expect("write legacy index");

    let listed = repository
        .list_runs(AgentRunListQuery {
            chat_ref: None,
            stable_chat_id: None,
            statuses: None,
            before: None,
            limit: 10,
        })
        .await
        .expect("list legacy run");

    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, run.id);
    assert_eq!(listed[0].presentation, AgentRunPresentation::Foreground);

    let loaded = repository.load_run(&run.id).await.expect("load legacy run");
    assert_eq!(loaded.presentation, AgentRunPresentation::Foreground);

    fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn list_runs_accepts_legacy_awaiting_commit_status() {
    let root = temp_root();
    let repository = FileAgentRepository::new(root.clone());
    let mut run = sample_run_with_id("run_legacy_awaiting_commit");
    run.status = AgentRunStatus::AwaitingHostCommit;
    repository.create_run(&run).await.expect("create run");

    let index_path = root.join("index/runs/run_legacy_awaiting_commit.json");
    let mut legacy = serde_json::to_value(&run).expect("serialize legacy run");
    legacy.as_object_mut().expect("run json object").insert(
        "status".to_string(),
        Value::String("awaiting_commit".to_string()),
    );
    FileAgentRepository::write_json_atomic(&index_path, &legacy)
        .await
        .expect("write legacy index");

    let listed = repository
        .list_runs(AgentRunListQuery {
            chat_ref: None,
            stable_chat_id: None,
            statuses: None,
            before: None,
            limit: 10,
        })
        .await
        .expect("list legacy run");

    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, run.id);
    assert_eq!(listed[0].status, AgentRunStatus::AwaitingHostCommit);

    let loaded = repository.load_run(&run.id).await.expect("load legacy run");
    assert_eq!(loaded.status, AgentRunStatus::AwaitingHostCommit);

    fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn list_runs_returns_empty_when_index_is_missing() {
    let root = temp_root();
    let repository = FileAgentRepository::new(root.clone());

    let listed = repository
        .list_runs(AgentRunListQuery {
            chat_ref: None,
            stable_chat_id: None,
            statuses: None,
            before: None,
            limit: 10,
        })
        .await
        .expect("list empty index");

    assert!(listed.is_empty());
    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn run_summary_projection_round_trips_by_run_id() {
    let root = temp_root();
    let repository = FileAgentRepository::new(root.clone());
    let run = sample_run_with_id("run_summary_projection");
    repository.create_run(&run).await.expect("create run");

    let projection = AgentRunSummaryProjection {
        schema_version: AGENT_RUN_SUMMARY_PROJECTION_SCHEMA_VERSION,
        run_id: run.id.clone(),
        source_run_updated_at: run.updated_at,
        commit_count: 1,
        committed_message: Some(AgentRunCommittedMessageProjection {
            commit_id: "commit_test".to_string(),
            message_id: "7".to_string(),
            message_index: Some(7),
            committed_at: instant("2026-01-04T00:00:00Z"),
        }),
        terminal_at: Some(instant("2026-01-04T00:01:00Z")),
    };

    assert!(
        repository
            .load_run_summary_projection(&run.id)
            .await
            .expect("load missing projection")
            .is_none()
    );
    repository
        .save_run_summary_projection(&projection)
        .await
        .expect("save projection");
    let loaded = repository
        .load_run_summary_projection(&run.id)
        .await
        .expect("load projection")
        .expect("projection exists");

    assert_eq!(loaded.run_id, run.id);
    assert_eq!(loaded.commit_count, 1);
    assert_eq!(
        loaded
            .committed_message
            .as_ref()
            .map(|message| message.message_index),
        Some(Some(7))
    );

    fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn inspect_run_storage_counts_total_and_heavy_artifacts() {
    let root = temp_root();
    let repository = FileAgentRepository::new(root.clone());
    let run = sample_run_with_id("run_prune_stats");
    repository.create_run(&run).await.expect("create run");
    repository
        .append_event(
            &run.id,
            AgentRunEventLevel::Info,
            "run_completed",
            Value::Null,
        )
        .await
        .expect("append event");

    let run_dir = root
        .join("chats")
        .join(&run.workspace_id)
        .join("runs")
        .join(&run.id);
    fs::write(run_dir.join("manifest.json"), b"{}")
        .await
        .expect("write manifest");
    fs::create_dir_all(run_dir.join("input"))
        .await
        .expect("create input");
    fs::write(run_dir.join("input").join("prompt_snapshot.json"), b"12345")
        .await
        .expect("write prompt snapshot");
    fs::create_dir_all(run_dir.join("output"))
        .await
        .expect("create output");
    fs::write(run_dir.join("output").join("main.md"), b"hi")
        .await
        .expect("write output");
    repository
        .save_run_summary_projection(&AgentRunSummaryProjection {
            schema_version: AGENT_RUN_SUMMARY_PROJECTION_SCHEMA_VERSION,
            run_id: run.id.clone(),
            source_run_updated_at: run.updated_at,
            commit_count: 0,
            committed_message: None,
            terminal_at: Some(run.updated_at),
        })
        .await
        .expect("save summary");

    let stats = repository
        .inspect_run_storage(&run)
        .await
        .expect("inspect storage");

    assert_eq!(stats.heavy_artifacts.file_count, 3);
    assert_eq!(stats.heavy_artifacts.byte_count, 9);
    assert_eq!(stats.total.file_count, 7);
    assert!(stats.total.byte_count > stats.heavy_artifacts.byte_count);

    fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn slim_run_heavy_artifacts_removes_only_non_core_run_files() {
    let root = temp_root();
    let repository = FileAgentRepository::new(root.clone());
    let run = sample_run_with_id("run_prune_slim");
    repository.create_run(&run).await.expect("create run");
    repository
        .append_event(
            &run.id,
            AgentRunEventLevel::Info,
            "run_completed",
            Value::Null,
        )
        .await
        .expect("append event");

    let run_dir = root
        .join("chats")
        .join(&run.workspace_id)
        .join("runs")
        .join(&run.id);
    fs::write(run_dir.join("manifest.json"), b"{}")
        .await
        .expect("write manifest");
    fs::create_dir_all(run_dir.join("input"))
        .await
        .expect("create input");
    fs::write(run_dir.join("input").join("prompt_snapshot.json"), b"12345")
        .await
        .expect("write prompt snapshot");
    fs::create_dir_all(run_dir.join("output"))
        .await
        .expect("create output");
    fs::write(run_dir.join("output").join("main.md"), b"hi")
        .await
        .expect("write output");
    repository
        .save_run_summary_projection(&AgentRunSummaryProjection {
            schema_version: AGENT_RUN_SUMMARY_PROJECTION_SCHEMA_VERSION,
            run_id: run.id.clone(),
            source_run_updated_at: run.updated_at,
            commit_count: 0,
            committed_message: None,
            terminal_at: Some(run.updated_at),
        })
        .await
        .expect("save summary");

    let removed = repository
        .slim_run_heavy_artifacts(&run)
        .await
        .expect("slim heavy artifacts");

    assert_eq!(removed.file_count, 3);
    assert_eq!(removed.byte_count, 9);
    assert!(run_dir.join("run.json").exists());
    assert!(run_dir.join("events.jsonl").exists());
    assert!(root.join("index/runs/run_prune_slim.json").exists());
    assert!(
        root.join("index/run-summaries/run_prune_slim.json")
            .exists()
    );
    assert!(!run_dir.join("manifest.json").exists());
    assert!(!run_dir.join("input").exists());
    assert!(!run_dir.join("output").exists());

    fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn delete_run_removes_workspace_index_and_summary_projection() {
    let root = temp_root();
    let repository = FileAgentRepository::new(root.clone());
    let run = sample_run_with_id("run_prune_delete");
    repository.create_run(&run).await.expect("create run");
    repository
        .append_event(&run.id, AgentRunEventLevel::Info, "run_failed", Value::Null)
        .await
        .expect("append event");
    repository
        .save_run_summary_projection(&AgentRunSummaryProjection {
            schema_version: AGENT_RUN_SUMMARY_PROJECTION_SCHEMA_VERSION,
            run_id: run.id.clone(),
            source_run_updated_at: run.updated_at,
            commit_count: 0,
            committed_message: None,
            terminal_at: Some(run.updated_at),
        })
        .await
        .expect("save summary");

    let run_dir = root
        .join("chats")
        .join(&run.workspace_id)
        .join("runs")
        .join(&run.id);
    fs::create_dir_all(run_dir.join("input"))
        .await
        .expect("create input");
    fs::write(
        run_dir.join("input").join("prompt_snapshot.json"),
        b"payload",
    )
    .await
    .expect("write prompt snapshot");

    let removed = repository.delete_run(&run).await.expect("delete run");

    assert!(removed.file_count >= 5);
    assert!(!run_dir.exists());
    assert!(!root.join("index/runs/run_prune_delete.json").exists());
    assert!(
        !root
            .join("index/run-summaries/run_prune_delete.json")
            .exists()
    );

    fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn list_runs_rejects_index_file_with_mismatched_run_id() {
    let root = temp_root();
    let repository = FileAgentRepository::new(root.clone());
    let run = sample_run_with_id("run_index_real");
    repository.create_run(&run).await.expect("create run");
    FileAgentRepository::write_json_atomic(&root.join("index/runs/run_index_wrong.json"), &run)
        .await
        .expect("write mismatched index");

    let error = repository
        .list_runs(AgentRunListQuery {
            chat_ref: None,
            stable_chat_id: None,
            statuses: None,
            before: None,
            limit: 10,
        })
        .await
        .expect_err("mismatched index file must fail");

    assert!(matches!(error, DomainError::InvalidData(_)));
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
    repository
        .save_run_summary_projection(&AgentRunSummaryProjection {
            schema_version: AGENT_RUN_SUMMARY_PROJECTION_SCHEMA_VERSION,
            run_id: first.id.clone(),
            source_run_updated_at: first.updated_at,
            commit_count: 0,
            committed_message: None,
            terminal_at: None,
        })
        .await
        .expect("save first summary");
    repository
        .save_run_summary_projection(&AgentRunSummaryProjection {
            schema_version: AGENT_RUN_SUMMARY_PROJECTION_SCHEMA_VERSION,
            run_id: second.id.clone(),
            source_run_updated_at: second.updated_at,
            commit_count: 0,
            committed_message: None,
            terminal_at: None,
        })
        .await
        .expect("save second summary");
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
    assert!(!root.join("index/run-summaries/run_delete_a.json").exists());
    assert!(!root.join("index/run-summaries/run_delete_b.json").exists());

    fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn prune_persistent_states_removes_only_unretained_candidates() {
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
    fs::create_dir_all(states_dir.join("state_orphan_not_candidate"))
        .await
        .expect("create non-candidate state");
    fs::write(states_dir.join(".DS_Store"), b"finder metadata")
        .await
        .expect("write platform metadata");

    let prune = repository
        .prune_persistent_states(
            "chat_prune",
            AgentPersistentStatePruneRequest {
                retained_state_ids: vec!["state_keep".to_string()],
                candidate_state_ids: vec![
                    "state_keep".to_string(),
                    "state_drop".to_string(),
                    "state_missing".to_string(),
                ],
            },
        )
        .await
        .expect("prune persistent states");

    assert_eq!(prune.removed_state_ids, vec!["state_drop".to_string()]);
    assert!(states_dir.join("state_keep").exists());
    assert!(!states_dir.join("state_drop").exists());
    assert!(states_dir.join("state_orphan_not_candidate").exists());
    assert!(states_dir.join(".DS_Store").exists());

    fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn prune_persistent_states_rejects_candidate_file() {
    let root = temp_root();
    let repository = FileAgentRepository::new(root.clone());
    let states_dir = root
        .join("chats")
        .join("chat_prune")
        .join("persistent-states");
    fs::create_dir_all(&states_dir)
        .await
        .expect("create states dir");
    fs::write(states_dir.join("state_file"), b"not a directory")
        .await
        .expect("write candidate file");

    let error = repository
        .prune_persistent_states(
            "chat_prune",
            AgentPersistentStatePruneRequest {
                retained_state_ids: Vec::new(),
                candidate_state_ids: vec!["state_file".to_string()],
            },
        )
        .await
        .expect_err("candidate file should fail");

    assert!(matches!(error, DomainError::InvalidData(_)));
    assert!(states_dir.join("state_file").exists());

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
