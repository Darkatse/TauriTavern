use std::collections::VecDeque;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use serde_json::{Value, json};
use tokio::sync::{Mutex, watch};
use uuid::Uuid;

use super::AgentRuntimeService;
use super::artifacts::build_agent_manifest;
use super::ids::workspace_id_for_stable_chat_id;
use crate::application::dto::agent_dto::AgentResolveChatCommitDto;
use crate::application::dto::chat_completion_dto::ChatCompletionGenerateRequestDto;
use crate::application::errors::ApplicationError;
use crate::application::services::agent_model_gateway::{
    AgentModelExchange, AgentModelGateway, decode_chat_completion_response,
};
use crate::application::services::agent_profile_service::{
    AgentProfileResolveInput, AgentProfileService,
};
use crate::application::services::agent_tools::{
    AgentToolDispatcher, AgentToolEffect, AgentToolSession, BuiltinAgentToolRegistry,
};
use crate::application::services::skill_service::SkillService;
use crate::domain::errors::DomainError;
use crate::domain::models::agent::profile::ResolvedAgentProfile;
use crate::domain::models::agent::{
    AgentChatRef, AgentModelContentPart, AgentModelRequest, AgentModelRole, AgentRun,
    AgentRunEventLevel, AgentRunPresentation, AgentRunStatus, AgentToolCall, WorkspacePath,
};
use crate::domain::models::preset::{DefaultPreset, Preset, PresetType};
use crate::domain::repositories::agent_run_repository::{
    AgentRunEventReadQuery, AgentRunRepository,
};
use crate::domain::repositories::chat_repository::ChatRepository;
use crate::domain::repositories::preset_repository::PresetRepository;
use crate::domain::repositories::workspace_repository::WorkspaceRepository;
use crate::infrastructure::repositories::file_agent_profile_repository::FileAgentProfileRepository;
use crate::infrastructure::repositories::file_agent_repository::FileAgentRepository;
use crate::infrastructure::repositories::file_chat_repository::FileChatRepository;
use crate::infrastructure::repositories::file_skill_repository::FileSkillRepository;

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
async fn agent_loop_writes_artifact_and_completes() {
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
        presentation: AgentRunPresentation::Background,
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
        test_skill_service(&root),
        model_gateway,
        test_profile_service(&root),
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
    let profile = test_resolved_profile(&root).await;

    service
        .execute_agent_loop_run_inner(
            &run.id,
            prompt_snapshot,
            request,
            profile,
            &mut cancel_receiver,
        )
        .await
        .expect("agent loop");

    let saved = repository.load_run(&run.id).await.expect("load run");
    assert_eq!(saved.status, AgentRunStatus::Completed);

    let artifact = repository
        .read_text(&run.id, &WorkspacePath::parse("output/main.md").unwrap())
        .await
        .expect("read artifact");
    assert_eq!(artifact.text, "hello from loop");

    let stored_response = repository
        .read_text(
            &run.id,
            &WorkspacePath::parse("model-responses/round-001.json").unwrap(),
        )
        .await
        .expect("read stored model response");
    let stored_response: Value =
        serde_json::from_str(&stored_response.text).expect("stored response JSON");
    assert_eq!(stored_response["round"], json!(1));
    assert!(stored_response["response"]["rawResponse"]["choices"].is_array());

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
    wait_for_closed_sessions(&model_gateway_probe, vec!["run_loop_test".to_string()]).await;

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
    assert!(
        events
            .iter()
            .any(|event| event.event_type == "model_response_stored")
    );

    tokio::fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn agent_loop_retries_retryable_model_errors() {
    let root = std::env::temp_dir().join(format!(
        "tauritavern-agent-retry-loop-{}",
        Uuid::new_v4().simple()
    ));
    let repository = Arc::new(FileAgentRepository::new(root.clone()));
    let run = AgentRun {
        id: "run_retry_loop_test".to_string(),
        workspace_id: "chat_retry_loop_test".to_string(),
        stable_chat_id: "stable_retry_loop_test".to_string(),
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
    };
    repository.create_run(&run).await.expect("create run");

    let model_gateway = Arc::new(MockAgentModelGateway::with_results(vec![
        Err(ApplicationError::Transient(
            "temporary transport failure".to_string(),
        )),
        Err(ApplicationError::RateLimited(
            "provider rate limit".to_string(),
        )),
        Ok(json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_write",
                        "type": "function",
                        "function": {
                            "name": "workspace_write_file",
                            "arguments": "{\"path\":\"output/main.md\",\"content\":\"retry succeeded\"}"
                        }
                    }]
                }
            }]
        })),
        Ok(json!({
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
        })),
    ]));
    let model_gateway_probe = model_gateway.clone();

    let service = AgentRuntimeService::new(
        repository.clone(),
        repository.clone(),
        repository.clone(),
        test_chat_repository(&root),
        test_chat_repository(&root),
        test_skill_service(&root),
        model_gateway,
        test_profile_service(&root),
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
    let mut profile = test_resolved_profile(&root).await;
    profile.run.model_retry.max_retries = 3;
    profile.run.model_retry.interval_ms = 1;

    service
        .execute_agent_loop_run_inner(
            &run.id,
            prompt_snapshot,
            request,
            profile,
            &mut cancel_receiver,
        )
        .await
        .expect("agent loop");

    let model_requests = model_gateway_probe.requests().await;
    assert_eq!(model_requests.len(), 4);

    let artifact = repository
        .read_text(&run.id, &WorkspacePath::parse("output/main.md").unwrap())
        .await
        .expect("read artifact");
    assert_eq!(artifact.text, "retry succeeded");

    let events = repository
        .read_events(
            &run.id,
            AgentRunEventReadQuery {
                after_seq: Some(0),
                before_seq: None,
                limit: 200,
            },
        )
        .await
        .expect("read events");
    assert_eq!(
        events
            .iter()
            .filter(|event| event.event_type == "model_call_retry_scheduled")
            .count(),
        2
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event.event_type == "model_call_attempt_failed")
            .count(),
        2
    );

    tokio::fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn agent_loop_does_not_retry_non_retryable_model_errors() {
    let root = std::env::temp_dir().join(format!(
        "tauritavern-agent-no-retry-loop-{}",
        Uuid::new_v4().simple()
    ));
    let repository = Arc::new(FileAgentRepository::new(root.clone()));
    let run = AgentRun {
        id: "run_no_retry_loop_test".to_string(),
        workspace_id: "chat_no_retry_loop_test".to_string(),
        stable_chat_id: "stable_no_retry_loop_test".to_string(),
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
    };
    repository.create_run(&run).await.expect("create run");

    let model_gateway = Arc::new(MockAgentModelGateway::with_results(vec![Err(
        ApplicationError::ValidationError("model.invalid_tool_call: missing id".to_string()),
    )]));
    let model_gateway_probe = model_gateway.clone();
    let service = AgentRuntimeService::new(
        repository.clone(),
        repository.clone(),
        repository.clone(),
        test_chat_repository(&root),
        test_chat_repository(&root),
        test_skill_service(&root),
        model_gateway,
        test_profile_service(&root),
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
    let mut profile = test_resolved_profile(&root).await;
    profile.run.model_retry.max_retries = 3;
    profile.run.model_retry.interval_ms = 1;

    let error = service
        .execute_agent_loop_run_inner(
            &run.id,
            prompt_snapshot,
            request,
            profile,
            &mut cancel_receiver,
        )
        .await
        .expect_err("non-retryable model error");

    assert!(error.to_string().contains("missing id"));
    assert_eq!(model_gateway_probe.requests().await.len(), 1);

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
            .all(|event| event.event_type != "model_call_retry_scheduled")
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
        presentation: AgentRunPresentation::Background,
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
        test_skill_service(&root),
        model_gateway,
        test_profile_service(&root),
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
    let profile = test_resolved_profile(&root).await;

    service
        .execute_agent_loop_run_inner(
            &run.id,
            prompt_snapshot,
            request,
            profile,
            &mut cancel_receiver,
        )
        .await
        .expect("agent loop");

    let saved = repository.load_run(&run.id).await.expect("load run");
    assert_eq!(saved.status, AgentRunStatus::Completed);

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
async fn finish_promotes_persistent_workspace_projection() {
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
        presentation: AgentRunPresentation::Background,
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
        test_skill_service(&root),
        model_gateway,
        test_profile_service(&root),
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
    let profile = test_resolved_profile(&root).await;

    service
        .execute_agent_loop_run_inner(
            &run.id,
            prompt_snapshot,
            request,
            profile.clone(),
            &mut cancel_receiver,
        )
        .await
        .expect("agent loop");

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
            &build_agent_manifest(&next_run, &profile),
            &json!({"messages": []}),
            &profile,
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
async fn foreground_run_commits_chat_message_before_finish() {
    let root = std::env::temp_dir().join(format!(
        "tauritavern-agent-foreground-commit-{}",
        Uuid::new_v4().simple()
    ));
    let repository = Arc::new(FileAgentRepository::new(root.clone()));
    let run = AgentRun {
        id: "run_foreground_commit_test".to_string(),
        workspace_id: "chat_foreground_commit_test".to_string(),
        stable_chat_id: "stable_foreground_commit_test".to_string(),
        chat_ref: AgentChatRef::Character {
            character_id: "Seraphina".to_string(),
            file_name: "Seraphina.png".to_string(),
        },
        generation_type: "normal".to_string(),
        profile_id: None,
        presentation: AgentRunPresentation::Foreground,
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
                            "arguments": "{\"path\":\"output/main.md\",\"content\":\"foreground answer\"}"
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
                    "tool_calls": [
                        {
                            "id": "call_commit",
                            "type": "function",
                            "function": {
                                "name": "workspace_commit",
                                "arguments": "{}"
                            }
                        },
                        {
                            "id": "call_finish",
                            "type": "function",
                            "function": {
                                "name": "workspace_finish",
                                "arguments": "{}"
                            }
                        }
                    ]
                }
            }]
        }),
    ]));

    let service = Arc::new(AgentRuntimeService::new(
        repository.clone(),
        repository.clone(),
        repository.clone(),
        test_chat_repository(&root),
        test_chat_repository(&root),
        test_skill_service(&root),
        model_gateway,
        test_profile_service(&root),
    ));
    let request = ChatCompletionGenerateRequestDto {
        payload: json!({
            "chat_completion_source": "openai",
            "model": "test-model",
            "messages": [{ "role": "user", "content": "write visibly" }]
        })
        .as_object()
        .cloned()
        .unwrap(),
    };
    let prompt_snapshot = json!({ "chatCompletionPayload": request.payload.clone() });
    let (_cancel_sender, mut cancel_receiver) = watch::channel(false);
    let mut profile = test_resolved_profile(&root).await;
    profile.run.presentation = AgentRunPresentation::Foreground;

    let resolver = tokio::spawn(resolve_next_chat_commit(
        service.clone(),
        repository.clone(),
        run.id.clone(),
        "message_1",
    ));
    service
        .execute_agent_loop_run_inner(
            &run.id,
            prompt_snapshot,
            request,
            profile,
            &mut cancel_receiver,
        )
        .await
        .expect("agent loop");
    resolver.await.expect("resolver task");

    let saved = repository.load_run(&run.id).await.expect("load run");
    assert_eq!(saved.status, AgentRunStatus::Completed);

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
            .any(|event| event.event_type == "chat_commit_requested")
    );
    assert!(
        events
            .iter()
            .any(|event| event.event_type == "chat_commit_completed")
    );
    assert!(
        events
            .iter()
            .any(|event| event.event_type == "run_completed")
    );

    tokio::fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn foreground_finish_before_commit_returns_recoverable_error() {
    let root = std::env::temp_dir().join(format!(
        "tauritavern-agent-foreground-finish-guard-{}",
        Uuid::new_v4().simple()
    ));
    let repository = Arc::new(FileAgentRepository::new(root.clone()));
    let run = AgentRun {
        id: "run_foreground_finish_guard_test".to_string(),
        workspace_id: "chat_foreground_finish_guard_test".to_string(),
        stable_chat_id: "stable_foreground_finish_guard_test".to_string(),
        chat_ref: AgentChatRef::Character {
            character_id: "Seraphina".to_string(),
            file_name: "Seraphina.png".to_string(),
        },
        generation_type: "normal".to_string(),
        profile_id: None,
        presentation: AgentRunPresentation::Foreground,
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
                        "id": "call_finish_too_early",
                        "type": "function",
                        "function": {
                            "name": "workspace_finish",
                            "arguments": "{}"
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
                    "tool_calls": [
                        {
                            "id": "call_write_after_guard",
                            "type": "function",
                            "function": {
                                "name": "workspace_write_file",
                                "arguments": "{\"path\":\"output/main.md\",\"content\":\"guarded answer\"}"
                            }
                        },
                        {
                            "id": "call_commit_after_guard",
                            "type": "function",
                            "function": {
                                "name": "workspace_commit",
                                "arguments": "{\"mode\":\"append\"}"
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
                        "id": "call_finish_after_commit",
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

    let service = Arc::new(AgentRuntimeService::new(
        repository.clone(),
        repository.clone(),
        repository.clone(),
        test_chat_repository(&root),
        test_chat_repository(&root),
        test_skill_service(&root),
        model_gateway,
        test_profile_service(&root),
    ));
    let request = ChatCompletionGenerateRequestDto {
        payload: json!({
            "chat_completion_source": "openai",
            "model": "test-model",
            "messages": [{ "role": "user", "content": "finish too early then recover" }]
        })
        .as_object()
        .cloned()
        .unwrap(),
    };
    let prompt_snapshot = json!({ "chatCompletionPayload": request.payload.clone() });
    let (_cancel_sender, mut cancel_receiver) = watch::channel(false);
    let mut profile = test_resolved_profile(&root).await;
    profile.run.presentation = AgentRunPresentation::Foreground;

    let resolver = tokio::spawn(resolve_next_chat_commit(
        service.clone(),
        repository.clone(),
        run.id.clone(),
        "message_1",
    ));
    service
        .execute_agent_loop_run_inner(
            &run.id,
            prompt_snapshot,
            request,
            profile,
            &mut cancel_receiver,
        )
        .await
        .expect("agent loop");
    resolver.await.expect("resolver task");

    let saved = repository.load_run(&run.id).await.expect("load run");
    assert_eq!(saved.status, AgentRunStatus::Completed);

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
    let guard_failure = events
        .iter()
        .find(|event| {
            event.event_type == "tool_call_failed"
                && event.payload["callId"] == "call_finish_too_early"
        })
        .expect("foreground finish guard failure");
    assert_eq!(guard_failure.level, AgentRunEventLevel::Warn);
    assert_eq!(
        guard_failure.payload["errorCode"],
        "agent.foreground_commit_required"
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
        presentation: AgentRunPresentation::Background,
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
        test_skill_service(&root),
        model_gateway,
        test_profile_service(&root),
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
    let profile = test_resolved_profile(&root).await;

    service
        .execute_agent_loop_run_inner(
            &run.id,
            prompt_snapshot,
            request,
            profile,
            &mut cancel_receiver,
        )
        .await
        .expect("agent loop");

    let saved = repository.load_run(&run.id).await.expect("load run");
    assert_eq!(saved.status, AgentRunStatus::Completed);

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
        presentation: AgentRunPresentation::Background,
        status: AgentRunStatus::Created,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    repository.create_run(&run).await.expect("create run");
    let profile = test_resolved_profile(&root).await;
    repository
        .initialize_run(
            &run,
            &build_agent_manifest(&run, &profile),
            &json!({"messages": []}),
            &profile,
        )
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
        test_skill_service(&root),
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
        .dispatch(&run.id, &patch_call, &mut session, &profile)
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
        .dispatch(&run.id, &read_call, &mut session, &profile)
        .await
        .expect("dispatch read");

    let patched = dispatcher
        .dispatch(&run.id, &patch_call, &mut session, &profile)
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
        presentation: AgentRunPresentation::Background,
        status: AgentRunStatus::Created,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    repository.create_run(&run).await.expect("create run");
    let profile = test_resolved_profile(&root).await;
    repository
        .initialize_run(
            &run,
            &build_agent_manifest(&run, &profile),
            &json!({"messages": []}),
            &profile,
        )
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
        test_skill_service(&root),
    );
    let mut session = AgentToolSession::default();
    let search_call = AgentToolCall {
        id: "call_search".to_string(),
        name: "chat.search".to_string(),
        arguments: json!({ "query": "blue lantern" }),
        provider_metadata: Value::Null,
    };
    let searched = dispatcher
        .dispatch(&run.id, &search_call, &mut session, &profile)
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
        .dispatch(&run.id, &read_call, &mut session, &profile)
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
        presentation: AgentRunPresentation::Background,
        status: AgentRunStatus::Created,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    repository.create_run(&run).await.expect("create run");
    let profile = test_resolved_profile(&root).await;
    repository
        .initialize_run(
            &run,
            &build_agent_manifest(&run, &profile),
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
            &profile,
        )
        .await
        .expect("initialize workspace");

    let dispatcher = AgentToolDispatcher::new(
        repository.clone(),
        chat_repository.clone(),
        chat_repository,
        repository.clone(),
        test_skill_service(&root),
    );
    let mut session = AgentToolSession::default();
    let call = AgentToolCall {
        id: "call_worldinfo".to_string(),
        name: "worldinfo.read_activated".to_string(),
        arguments: json!({}),
        provider_metadata: Value::Null,
    };
    let result = dispatcher
        .dispatch(&run.id, &call, &mut session, &profile)
        .await
        .expect("dispatch worldinfo");

    assert!(!result.result.is_error);
    assert!(result.result.content.contains("hidden blue lantern"));
    assert_eq!(result.result.resource_refs[0], "worldinfo:lorebook#7");

    tokio::fs::remove_dir_all(root).await.expect("cleanup");
}

struct MockAgentModelGateway {
    responses: Mutex<VecDeque<Result<Value, ApplicationError>>>,
    requests: Mutex<Vec<AgentModelRequest>>,
    closed_sessions: Mutex<Vec<String>>,
}

fn test_chat_repository(root: &Path) -> Arc<FileChatRepository> {
    Arc::new(FileChatRepository::new(
        root.join("characters"),
        root.join("chats"),
        root.join("group_chats"),
        root.join("backups"),
    ))
}

fn test_skill_service(root: &Path) -> Arc<SkillService> {
    Arc::new(SkillService::new(Arc::new(FileSkillRepository::new(
        root.join("skills"),
    ))))
}

fn test_profile_service(root: &Path) -> Arc<AgentProfileService> {
    Arc::new(AgentProfileService::new(
        Arc::new(FileAgentProfileRepository::new(root.join("agent-profiles"))),
        Arc::new(NullPresetRepository),
    ))
}

async fn test_resolved_profile(root: &Path) -> ResolvedAgentProfile {
    let registry = BuiltinAgentToolRegistry::phase2c();
    let mut profile = test_profile_service(root)
        .resolve_profile(AgentProfileResolveInput {
            profile_id: None,
            known_tools: registry.specs(),
        })
        .await
        .expect("resolve default profile");
    profile.run.presentation = AgentRunPresentation::Background;
    profile
}

async fn resolve_next_chat_commit(
    service: Arc<AgentRuntimeService>,
    repository: Arc<FileAgentRepository>,
    run_id: String,
    message_id: &'static str,
) {
    let commit_id = tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            let events = repository
                .read_events(
                    &run_id,
                    AgentRunEventReadQuery {
                        after_seq: Some(0),
                        before_seq: None,
                        limit: 100,
                    },
                )
                .await
                .expect("read events");
            if let Some(commit_id) = events
                .iter()
                .find(|event| event.event_type == "chat_commit_requested")
                .and_then(|event| event.payload["commitId"].as_str())
            {
                return commit_id.to_string();
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("chat commit request");

    service
        .resolve_chat_commit(AgentResolveChatCommitDto {
            run_id,
            commit_id,
            message_id: Some(message_id.to_string()),
            error: None,
        })
        .await
        .expect("resolve chat commit");
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
        Self::with_results(responses.into_iter().map(Ok).collect())
    }

    fn with_results(responses: Vec<Result<Value, ApplicationError>>) -> Self {
        Self {
            responses: Mutex::new(responses.into()),
            requests: Mutex::new(Vec::new()),
            closed_sessions: Mutex::new(Vec::new()),
        }
    }

    async fn requests(&self) -> Vec<AgentModelRequest> {
        self.requests.lock().await.clone()
    }

    async fn closed_sessions(&self) -> Vec<String> {
        self.closed_sessions.lock().await.clone()
    }
}

struct NullPresetRepository;

#[async_trait]
impl PresetRepository for NullPresetRepository {
    async fn save_preset(&self, _preset: &Preset) -> Result<(), DomainError> {
        Ok(())
    }

    async fn delete_preset(
        &self,
        _name: &str,
        _preset_type: &PresetType,
    ) -> Result<(), DomainError> {
        Ok(())
    }

    async fn preset_exists(
        &self,
        _name: &str,
        _preset_type: &PresetType,
    ) -> Result<bool, DomainError> {
        Ok(false)
    }

    async fn get_preset(
        &self,
        _name: &str,
        _preset_type: &PresetType,
    ) -> Result<Option<Preset>, DomainError> {
        Ok(None)
    }

    async fn list_presets(&self, _preset_type: &PresetType) -> Result<Vec<String>, DomainError> {
        Ok(Vec::new())
    }

    async fn get_default_preset(
        &self,
        _name: &str,
        _preset_type: &PresetType,
    ) -> Result<Option<DefaultPreset>, DomainError> {
        Ok(None)
    }
}

async fn wait_for_closed_sessions(gateway: &MockAgentModelGateway, expected: Vec<String>) {
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if gateway.closed_sessions().await == expected {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("agent provider session cleanup");
}

#[async_trait]
impl AgentModelGateway for MockAgentModelGateway {
    async fn generate_with_cancel(
        &self,
        request: AgentModelRequest,
        _cancel: watch::Receiver<bool>,
    ) -> Result<AgentModelExchange, ApplicationError> {
        self.requests.lock().await.push(request.clone());
        let response = self.responses.lock().await.pop_front().ok_or_else(|| {
            ApplicationError::ValidationError(
                "mock_model.empty_responses: no response left".to_string(),
            )
        })??;
        let response = decode_chat_completion_response(response, &request.tools)?;
        Ok(AgentModelExchange {
            response,
            provider_state: request.provider_state,
        })
    }

    async fn close_session(&self, session_id: &str) {
        self.closed_sessions
            .lock()
            .await
            .push(session_id.to_string());
    }
}
