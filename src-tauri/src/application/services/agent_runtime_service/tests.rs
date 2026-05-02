use std::collections::VecDeque;
use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use serde_json::{Value, json};
use tokio::sync::{Mutex, watch};
use uuid::Uuid;

use super::AgentRuntimeService;
use super::artifacts::build_agent_manifest;
use super::ids::workspace_id_for_stable_chat_id;
use crate::application::dto::agent_dto::{AgentFinalizeCommitDto, AgentPrepareCommitDto};
use crate::application::dto::chat_completion_dto::ChatCompletionGenerateRequestDto;
use crate::application::errors::ApplicationError;
use crate::application::services::agent_model_gateway::{
    AgentModelGateway, decode_chat_completion_response,
};
use crate::application::services::agent_tools::{
    AgentToolDispatcher, AgentToolEffect, AgentToolSession,
};
use crate::domain::models::agent::{
    AgentChatRef, AgentModelContentPart, AgentModelRequest, AgentModelResponse, AgentModelRole,
    AgentRun, AgentRunEventLevel, AgentRunStatus, AgentToolCall, WorkspacePath,
};
use crate::domain::repositories::agent_run_repository::{
    AgentRunEventReadQuery, AgentRunRepository,
};
use crate::domain::repositories::chat_repository::ChatRepository;
use crate::domain::repositories::workspace_repository::WorkspaceRepository;
use crate::infrastructure::repositories::file_agent_repository::FileAgentRepository;
use crate::infrastructure::repositories::file_chat_repository::FileChatRepository;

#[test]
fn workspace_id_uses_stable_chat_id_not_character_chat_file_name() {
    let first = AgentChatRef::Character {
        character_id: "Seraphina".to_string(),
        file_name: "old-chat".to_string(),
    };
    let second = AgentChatRef::Character {
        character_id: "Seraphina".to_string(),
        file_name: "renamed-chat".to_string(),
    };

    let first_id = workspace_id_for_stable_chat_id(&first, "stable-chat").unwrap();
    let second_id = workspace_id_for_stable_chat_id(&second, "stable-chat").unwrap();

    assert_eq!(first_id, second_id);
}

#[tokio::test]
async fn agent_loop_writes_artifact_and_reaches_awaiting_commit() {
    let root = std::env::temp_dir().join(format!(
        "tauritavern-agent-loop-{}",
        Uuid::new_v4().simple()
    ));
    let repository = Arc::new(FileAgentRepository::new(root.clone()));
    let run = AgentRun {
        id: "run_loop_test".to_string(),
        workspace_id: "chat_loop_test".to_string(),
        stable_chat_id: "stable_loop_test".to_string(),
        chat_ref: AgentChatRef::Character {
            character_id: "Seraphina".to_string(),
            file_name: "Seraphina.png".to_string(),
        },
        generation_type: "normal".to_string(),
        profile_id: None,
        status: AgentRunStatus::Created,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    repository.create_run(&run).await.expect("create run");

    let model_gateway = Arc::new(MockAgentModelGateway::new(vec![
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_write",
                        "type": "function",
                        "function": {
                            "name": "workspace_write_file",
                            "arguments": "{\"path\":\"output/main.md\",\"content\":\"hello from loop\"}"
                        }
                    }]
                }
            }]
        }),
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_finish",
                        "type": "function",
                        "function": {
                            "name": "workspace_finish",
                            "arguments": "{}"
                        }
                    }]
                }
            }]
        }),
    ]));
    let model_gateway_probe = model_gateway.clone();

    let service = AgentRuntimeService::new(
        repository.clone(),
        repository.clone(),
        repository.clone(),
        test_chat_repository(&root),
        test_chat_repository(&root),
        model_gateway,
    );
    let request = ChatCompletionGenerateRequestDto {
        payload: json!({
            "chat_completion_source": "openai",
            "model": "test-model",
            "messages": [{ "role": "user", "content": "write a message" }]
        })
        .as_object()
        .cloned()
        .unwrap(),
    };
    let prompt_snapshot = json!({ "chatCompletionPayload": request.payload.clone() });
    let (_cancel_sender, mut cancel_receiver) = watch::channel(false);

    service
        .execute_agent_loop_run_inner(&run.id, prompt_snapshot, request, &mut cancel_receiver)
        .await
        .expect("agent loop");

    let saved = repository.load_run(&run.id).await.expect("load run");
    assert_eq!(saved.status, AgentRunStatus::AwaitingCommit);

    let artifact = repository
        .read_text(&run.id, &WorkspacePath::parse("output/main.md").unwrap())
        .await
        .expect("read artifact");
    assert_eq!(artifact.text, "hello from loop");

    let model_requests = model_gateway_probe.requests().await;
    let second_request = model_requests.get(1).expect("second model request");
    let hydrated_tool_result = second_request
        .messages
        .iter()
        .find(|message| message.role == AgentModelRole::Tool)
        .and_then(|message| message.parts.first())
        .and_then(|part| match part {
            AgentModelContentPart::ToolResult { result } => Some(result),
            _ => None,
        })
        .expect("hydrated tool result");
    assert!(
        hydrated_tool_result
            .content
            .contains("Full content of output/main.md")
    );
    assert!(hydrated_tool_result.content.contains("hello from loop"));

    let events = repository
        .read_events(
            &run.id,
            AgentRunEventReadQuery {
                after_seq: Some(0),
                before_seq: None,
                limit: 100,
            },
        )
        .await
        .expect("read events");
    assert!(
        events
            .iter()
            .any(|event| event.event_type == "agent_loop_finished")
    );

    tokio::fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn agent_loop_reads_and_patches_workspace_artifact() {
    let root = std::env::temp_dir().join(format!(
        "tauritavern-agent-patch-loop-{}",
        Uuid::new_v4().simple()
    ));
    let repository = Arc::new(FileAgentRepository::new(root.clone()));
    let run = AgentRun {
        id: "run_patch_loop_test".to_string(),
        workspace_id: "chat_patch_loop_test".to_string(),
        stable_chat_id: "stable_patch_loop_test".to_string(),
        chat_ref: AgentChatRef::Character {
            character_id: "Seraphina".to_string(),
            file_name: "Seraphina.png".to_string(),
        },
        generation_type: "normal".to_string(),
        profile_id: None,
        status: AgentRunStatus::Created,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    repository.create_run(&run).await.expect("create run");

    let model_gateway = Arc::new(MockAgentModelGateway::new(vec![
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_write",
                        "type": "function",
                        "function": {
                            "name": "workspace_write_file",
                            "arguments": "{\"path\":\"output/main.md\",\"content\":\"rough draft\"}"
                        }
                    }]
                }
            }]
        }),
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_read",
                        "type": "function",
                        "function": {
                            "name": "workspace_read_file",
                            "arguments": "{\"path\":\"output/main.md\"}"
                        }
                    }]
                }
            }]
        }),
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_patch",
                        "type": "function",
                        "function": {
                            "name": "workspace_apply_patch",
                            "arguments": "{\"path\":\"output/main.md\",\"old_string\":\"rough\",\"new_string\":\"polished\"}"
                        }
                    }]
                }
            }]
        }),
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_finish",
                        "type": "function",
                        "function": {
                            "name": "workspace_finish",
                            "arguments": "{}"
                        }
                    }]
                }
            }]
        }),
    ]));

    let service = AgentRuntimeService::new(
        repository.clone(),
        repository.clone(),
        repository.clone(),
        test_chat_repository(&root),
        test_chat_repository(&root),
        model_gateway,
    );
    let request = ChatCompletionGenerateRequestDto {
        payload: json!({
            "chat_completion_source": "openai",
            "model": "test-model",
            "messages": [{ "role": "user", "content": "revise a message" }]
        })
        .as_object()
        .cloned()
        .unwrap(),
    };
    let prompt_snapshot = json!({ "chatCompletionPayload": request.payload.clone() });
    let (_cancel_sender, mut cancel_receiver) = watch::channel(false);

    service
        .execute_agent_loop_run_inner(&run.id, prompt_snapshot, request, &mut cancel_receiver)
        .await
        .expect("agent loop");

    let saved = repository.load_run(&run.id).await.expect("load run");
    assert_eq!(saved.status, AgentRunStatus::AwaitingCommit);

    let artifact = repository
        .read_text(&run.id, &WorkspacePath::parse("output/main.md").unwrap())
        .await
        .expect("read artifact");
    assert_eq!(artifact.text, "polished draft");

    let events = repository
        .read_events(
            &run.id,
            AgentRunEventReadQuery {
                after_seq: Some(0),
                before_seq: None,
                limit: 100,
            },
        )
        .await
        .expect("read events");
    assert!(
        events
            .iter()
            .any(|event| event.event_type == "workspace_patch_applied")
    );

    tokio::fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn finalize_commit_promotes_persistent_workspace_projection() {
    let root = std::env::temp_dir().join(format!(
        "tauritavern-agent-persist-loop-{}",
        Uuid::new_v4().simple()
    ));
    let repository = Arc::new(FileAgentRepository::new(root.clone()));
    let run = AgentRun {
        id: "run_persist_loop_test".to_string(),
        workspace_id: "chat_persist_loop_test".to_string(),
        stable_chat_id: "stable_persist_loop_test".to_string(),
        chat_ref: AgentChatRef::Character {
            character_id: "Seraphina".to_string(),
            file_name: "Seraphina.png".to_string(),
        },
        generation_type: "normal".to_string(),
        profile_id: None,
        status: AgentRunStatus::Created,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    repository.create_run(&run).await.expect("create run");

    let model_gateway = Arc::new(MockAgentModelGateway::new(vec![
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [
                        {
                            "id": "call_write_output",
                            "type": "function",
                            "function": {
                                "name": "workspace_write_file",
                                "arguments": "{\"path\":\"output/main.md\",\"content\":\"committed story\"}"
                            }
                        },
                        {
                            "id": "call_write_persist",
                            "type": "function",
                            "function": {
                                "name": "workspace_write_file",
                                "arguments": "{\"path\":\"persist/MEMORY.md\",\"content\":\"The theatre sister thread is unresolved.\"}"
                            }
                        }
                    ]
                }
            }]
        }),
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_finish",
                        "type": "function",
                        "function": {
                            "name": "workspace_finish",
                            "arguments": "{}"
                        }
                    }]
                }
            }]
        }),
    ]));

    let service = AgentRuntimeService::new(
        repository.clone(),
        repository.clone(),
        repository.clone(),
        test_chat_repository(&root),
        test_chat_repository(&root),
        model_gateway,
    );
    let request = ChatCompletionGenerateRequestDto {
        payload: json!({
            "chat_completion_source": "openai",
            "model": "test-model",
            "messages": [{ "role": "user", "content": "write and remember" }]
        })
        .as_object()
        .cloned()
        .unwrap(),
    };
    let prompt_snapshot = json!({ "chatCompletionPayload": request.payload.clone() });
    let (_cancel_sender, mut cancel_receiver) = watch::channel(false);

    service
        .execute_agent_loop_run_inner(&run.id, prompt_snapshot, request, &mut cancel_receiver)
        .await
        .expect("agent loop");
    service
        .prepare_commit(AgentPrepareCommitDto {
            run_id: run.id.clone(),
        })
        .await
        .expect("prepare commit");
    service
        .finalize_commit(AgentFinalizeCommitDto {
            run_id: run.id.clone(),
            message_id: Some("message_1".to_string()),
        })
        .await
        .expect("finalize commit");

    let next_run = AgentRun {
        id: "run_persist_loop_next".to_string(),
        status: AgentRunStatus::Created,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        ..run.clone()
    };
    repository
        .create_run(&next_run)
        .await
        .expect("create next run");
    repository
        .initialize_run(
            &next_run,
            &build_agent_manifest(&next_run),
            &json!({"messages": []}),
        )
        .await
        .expect("initialize next run");
    let projected = repository
        .read_text(
            &next_run.id,
            &WorkspacePath::parse("persist/MEMORY.md").unwrap(),
        )
        .await
        .expect("read projected persist");
    assert_eq!(projected.text, "The theatre sister thread is unresolved.");

    let events = repository
        .read_events(
            &run.id,
            AgentRunEventReadQuery {
                after_seq: Some(0),
                before_seq: None,
                limit: 100,
            },
        )
        .await
        .expect("read events");
    assert!(
        events
            .iter()
            .any(|event| event.event_type == "persistent_changes_committed")
    );

    tokio::fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn agent_loop_returns_recoverable_tool_errors_to_model() {
    let root = std::env::temp_dir().join(format!(
        "tauritavern-agent-tool-error-loop-{}",
        Uuid::new_v4().simple()
    ));
    let repository = Arc::new(FileAgentRepository::new(root.clone()));
    let run = AgentRun {
        id: "run_tool_error_loop_test".to_string(),
        workspace_id: "chat_tool_error_loop_test".to_string(),
        stable_chat_id: "stable_tool_error_loop_test".to_string(),
        chat_ref: AgentChatRef::Character {
            character_id: "Seraphina".to_string(),
            file_name: "Seraphina.png".to_string(),
        },
        generation_type: "normal".to_string(),
        profile_id: None,
        status: AgentRunStatus::Created,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    repository.create_run(&run).await.expect("create run");

    let model_gateway = Arc::new(MockAgentModelGateway::new(vec![
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_bad_read",
                        "type": "function",
                        "function": {
                            "name": "workspace_read_file",
                            "arguments": "{\"path\":\".\"}"
                        }
                    }]
                }
            }]
        }),
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_write_after_error",
                        "type": "function",
                        "function": {
                            "name": "workspace_write_file",
                            "arguments": "{\"path\":\"output/main.md\",\"content\":\"recovered\"}"
                        }
                    }]
                }
            }]
        }),
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_finish_after_error",
                        "type": "function",
                        "function": {
                            "name": "workspace_finish",
                            "arguments": "{}"
                        }
                    }]
                }
            }]
        }),
    ]));

    let service = AgentRuntimeService::new(
        repository.clone(),
        repository.clone(),
        repository.clone(),
        test_chat_repository(&root),
        test_chat_repository(&root),
        model_gateway,
    );
    let request = ChatCompletionGenerateRequestDto {
        payload: json!({
            "chat_completion_source": "openai",
            "model": "test-model",
            "messages": [{ "role": "user", "content": "recover from tool error" }]
        })
        .as_object()
        .cloned()
        .unwrap(),
    };
    let prompt_snapshot = json!({ "chatCompletionPayload": request.payload.clone() });
    let (_cancel_sender, mut cancel_receiver) = watch::channel(false);

    service
        .execute_agent_loop_run_inner(&run.id, prompt_snapshot, request, &mut cancel_receiver)
        .await
        .expect("agent loop");

    let saved = repository.load_run(&run.id).await.expect("load run");
    assert_eq!(saved.status, AgentRunStatus::AwaitingCommit);

    let artifact = repository
        .read_text(&run.id, &WorkspacePath::parse("output/main.md").unwrap())
        .await
        .expect("read artifact");
    assert_eq!(artifact.text, "recovered");

    let events = repository
        .read_events(
            &run.id,
            AgentRunEventReadQuery {
                after_seq: Some(0),
                before_seq: None,
                limit: 100,
            },
        )
        .await
        .expect("read events");
    let failed = events
        .iter()
        .find(|event| {
            event.event_type == "tool_call_failed" && event.payload["callId"] == "call_bad_read"
        })
        .expect("tool failure event");
    assert_eq!(failed.level, AgentRunEventLevel::Warn);
    assert_eq!(failed.payload["errorCode"], "workspace.invalid_path");
    assert!(
        failed.payload["message"]
            .as_str()
            .expect("message")
            .contains("Workspace path cannot be empty")
    );

    tokio::fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn workspace_patch_requires_full_read_state() {
    let root = std::env::temp_dir().join(format!(
        "tauritavern-agent-patch-guard-{}",
        Uuid::new_v4().simple()
    ));
    let repository = Arc::new(FileAgentRepository::new(root.clone()));
    let run = AgentRun {
        id: "run_patch_guard_test".to_string(),
        workspace_id: "chat_patch_guard_test".to_string(),
        stable_chat_id: "stable_patch_guard_test".to_string(),
        chat_ref: AgentChatRef::Character {
            character_id: "Seraphina".to_string(),
            file_name: "Seraphina.png".to_string(),
        },
        generation_type: "normal".to_string(),
        profile_id: None,
        status: AgentRunStatus::Created,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    repository.create_run(&run).await.expect("create run");
    repository
        .initialize_run(&run, &build_agent_manifest(&run), &json!({"messages": []}))
        .await
        .expect("initialize workspace");
    repository
        .write_text(
            &run.id,
            &WorkspacePath::parse("output/main.md").unwrap(),
            "hello draft",
        )
        .await
        .expect("seed artifact");

    let chat_repository = test_chat_repository(&root);
    let dispatcher = AgentToolDispatcher::new(
        repository.clone(),
        chat_repository.clone(),
        chat_repository,
        repository.clone(),
    );
    let mut session = AgentToolSession::default();
    let patch_call = AgentToolCall {
        id: "call_patch".to_string(),
        name: "workspace.apply_patch".to_string(),
        arguments: json!({
            "path": "output/main.md",
            "old_string": "draft",
            "new_string": "final",
        }),
        provider_metadata: Value::Null,
    };

    let first = dispatcher
        .dispatch(&run.id, &patch_call, &mut session)
        .await
        .expect("dispatch patch");
    assert!(first.result.is_error);
    assert_eq!(
        first.result.error_code.as_deref(),
        Some("workspace.patch_requires_read")
    );

    let read_call = AgentToolCall {
        id: "call_read".to_string(),
        name: "workspace.read_file".to_string(),
        arguments: json!({ "path": "output/main.md" }),
        provider_metadata: Value::Null,
    };
    dispatcher
        .dispatch(&run.id, &read_call, &mut session)
        .await
        .expect("dispatch read");

    let patched = dispatcher
        .dispatch(&run.id, &patch_call, &mut session)
        .await
        .expect("dispatch patch after read");
    assert!(!patched.result.is_error);
    assert!(matches!(
        patched.effect,
        AgentToolEffect::WorkspaceFilePatched {
            replacements: 1,
            ..
        }
    ));

    let artifact = repository
        .read_text(&run.id, &WorkspacePath::parse("output/main.md").unwrap())
        .await
        .expect("read artifact");
    assert_eq!(artifact.text, "hello final");

    tokio::fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn dispatcher_searches_and_reads_current_chat_messages() {
    let root = std::env::temp_dir().join(format!(
        "tauritavern-agent-chat-tools-{}",
        Uuid::new_v4().simple()
    ));
    let repository = Arc::new(FileAgentRepository::new(root.clone()));
    let chat_repository = test_chat_repository(&root);
    let run = AgentRun {
        id: "run_chat_tools_test".to_string(),
        workspace_id: "chat_tools_test".to_string(),
        stable_chat_id: "stable_chat_tools_test".to_string(),
        chat_ref: AgentChatRef::Character {
            character_id: "alice".to_string(),
            file_name: "session".to_string(),
        },
        generation_type: "normal".to_string(),
        profile_id: None,
        status: AgentRunStatus::Created,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    repository.create_run(&run).await.expect("create run");
    repository
        .initialize_run(&run, &build_agent_manifest(&run), &json!({"messages": []}))
        .await
        .expect("initialize workspace");
    save_character_payload(
        &chat_repository,
        &root,
        "alice",
        "session",
        &[
            json!({
                "chat_metadata": {},
                "user_name": "unused",
                "character_name": "unused",
            }),
            json!({
                "name": "User",
                "is_user": true,
                "is_system": false,
                "send_date": "2026-01-01T00:00:00.000Z",
                "mes": "hello",
                "extra": {},
            }),
            json!({
                "name": "Alice",
                "is_user": false,
                "is_system": false,
                "send_date": "2026-01-01T00:00:01.000Z",
                "mes": "the blue lantern is hidden under the bridge",
                "extra": {},
            }),
        ],
    )
    .await;

    let dispatcher = AgentToolDispatcher::new(
        repository.clone(),
        chat_repository.clone(),
        chat_repository,
        repository.clone(),
    );
    let mut session = AgentToolSession::default();
    let search_call = AgentToolCall {
        id: "call_search".to_string(),
        name: "chat.search".to_string(),
        arguments: json!({ "query": "blue lantern" }),
        provider_metadata: Value::Null,
    };
    let searched = dispatcher
        .dispatch(&run.id, &search_call, &mut session)
        .await
        .expect("dispatch search");
    assert!(!searched.result.is_error);
    assert_eq!(searched.result.structured["hits"][0]["index"], 1);
    assert!(searched.result.structured["hits"][0].get("text").is_none());

    let read_call = AgentToolCall {
        id: "call_read_messages".to_string(),
        name: "chat.read_messages".to_string(),
        arguments: json!({
            "messages": [{ "index": 1, "start_char": 4, "max_chars": 12 }]
        }),
        provider_metadata: Value::Null,
    };
    let read = dispatcher
        .dispatch(&run.id, &read_call, &mut session)
        .await
        .expect("dispatch read messages");
    assert!(!read.result.is_error);
    assert_eq!(
        read.result.structured["messages"][0]["text"],
        "blue lantern"
    );
    assert_eq!(read.result.resource_refs[0], "chat:current#1:chars=4..16");

    tokio::fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn dispatcher_reads_worldinfo_activation_from_run_snapshot() {
    let root = std::env::temp_dir().join(format!(
        "tauritavern-agent-worldinfo-tool-{}",
        Uuid::new_v4().simple()
    ));
    let repository = Arc::new(FileAgentRepository::new(root.clone()));
    let chat_repository = test_chat_repository(&root);
    let run = AgentRun {
        id: "run_worldinfo_tool_test".to_string(),
        workspace_id: "worldinfo_tool_test".to_string(),
        stable_chat_id: "stable_worldinfo_tool_test".to_string(),
        chat_ref: AgentChatRef::Character {
            character_id: "alice".to_string(),
            file_name: "session".to_string(),
        },
        generation_type: "normal".to_string(),
        profile_id: None,
        status: AgentRunStatus::Created,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    repository.create_run(&run).await.expect("create run");
    repository
        .initialize_run(
            &run,
            &build_agent_manifest(&run),
            &json!({
                "chatCompletionPayload": {
                    "messages": [{ "role": "user", "content": "hello" }]
                },
                "worldInfoActivation": {
                    "timestampMs": 1,
                    "trigger": "normal",
                    "entries": [{
                        "world": "lorebook",
                        "uid": 7,
                        "displayName": "Hidden bridge",
                        "constant": false,
                        "position": "before",
                        "content": "The bridge has a hidden blue lantern."
                    }]
                }
            }),
        )
        .await
        .expect("initialize workspace");

    let dispatcher = AgentToolDispatcher::new(
        repository.clone(),
        chat_repository.clone(),
        chat_repository,
        repository.clone(),
    );
    let mut session = AgentToolSession::default();
    let call = AgentToolCall {
        id: "call_worldinfo".to_string(),
        name: "worldinfo.read_activated".to_string(),
        arguments: json!({}),
        provider_metadata: Value::Null,
    };
    let result = dispatcher
        .dispatch(&run.id, &call, &mut session)
        .await
        .expect("dispatch worldinfo");

    assert!(!result.result.is_error);
    assert!(result.result.content.contains("hidden blue lantern"));
    assert_eq!(result.result.resource_refs[0], "worldinfo:lorebook#7");

    tokio::fs::remove_dir_all(root).await.expect("cleanup");
}

struct MockAgentModelGateway {
    responses: Mutex<VecDeque<Value>>,
    requests: Mutex<Vec<AgentModelRequest>>,
}

fn test_chat_repository(root: &Path) -> Arc<FileChatRepository> {
    Arc::new(FileChatRepository::new(
        root.join("characters"),
        root.join("chats"),
        root.join("group_chats"),
        root.join("backups"),
    ))
}

async fn save_character_payload(
    repository: &FileChatRepository,
    root: &Path,
    character_name: &str,
    file_name: &str,
    payload: &[Value],
) {
    let source_path = root.join(format!("chat-payload-{}.jsonl", Uuid::new_v4().simple()));
    tokio::fs::write(&source_path, payload_to_jsonl(payload))
        .await
        .expect("write payload");
    repository
        .save_chat_payload_from_path(character_name, file_name, &source_path, false)
        .await
        .expect("save payload");
}

fn payload_to_jsonl(payload: &[Value]) -> String {
    let mut text = String::new();
    for value in payload {
        text.push_str(&serde_json::to_string(value).expect("serialize jsonl value"));
        text.push('\n');
    }
    text
}

impl MockAgentModelGateway {
    fn new(responses: Vec<Value>) -> Self {
        Self {
            responses: Mutex::new(responses.into()),
            requests: Mutex::new(Vec::new()),
        }
    }

    async fn requests(&self) -> Vec<AgentModelRequest> {
        self.requests.lock().await.clone()
    }
}

#[async_trait]
impl AgentModelGateway for MockAgentModelGateway {
    async fn generate_with_cancel(
        &self,
        request: AgentModelRequest,
        _cancel: watch::Receiver<bool>,
    ) -> Result<AgentModelResponse, ApplicationError> {
        self.requests.lock().await.push(request.clone());
        let response = self.responses.lock().await.pop_front().ok_or_else(|| {
            ApplicationError::ValidationError(
                "mock_model.empty_responses: no response left".to_string(),
            )
        })?;
        decode_chat_completion_response(response, &request.tools)
    }
}
