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
use crate::application::dto::agent_dto::{AgentReadModelTurnDto, AgentResolveChatCommitDto};
use crate::application::dto::chat_completion_dto::ChatCompletionGenerateRequestDto;
use crate::application::errors::ApplicationError;
use crate::application::services::agent_identity::workspace_id_for_stable_chat_id;
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
    AgentRunEventLevel, AgentRunPresentation, AgentRunStatus, AgentToolCall, WorkspaceManifest,
    WorkspacePath, WorkspacePersistentChangeSet,
};
use crate::domain::models::preset::{DefaultPreset, Preset, PresetType};
use crate::domain::models::skill::{SkillImportInput, SkillInlineFile, SkillInstallRequest};
use crate::domain::repositories::agent_run_repository::{
    AgentRunEventReadQuery, AgentRunRepository,
};
use crate::domain::repositories::chat_repository::ChatRepository;
use crate::domain::repositories::preset_repository::PresetRepository;
use crate::domain::repositories::skill_repository::SkillRepository;
use crate::domain::repositories::workspace_repository::{
    WorkspaceFile, WorkspaceFileList, WorkspaceRepository,
};
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
        persist_base_state_id: None,
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
                    "content": "I will write the artifact.",
                    "reasoning_content": "Need to create output/main.md.",
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
            "messages": prompt_messages("write a message")
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

    let model_turn = service
        .read_model_turn(AgentReadModelTurnDto {
            run_id: run.id.clone(),
            round: 1,
            max_chars: 40_000,
        })
        .await
        .expect("read model turn");
    assert_eq!(model_turn.assistant.text, "I will write the artifact.");
    assert_eq!(model_turn.assistant.bytes, 26);
    assert!(!model_turn.assistant.truncated);
    assert_eq!(model_turn.reasoning.len(), 1);
    assert_eq!(
        model_turn.reasoning[0].text,
        "Need to create output/main.md."
    );
    assert_eq!(model_turn.reasoning[0].source, "reasoning_content");
    assert_eq!(model_turn.tool_calls.len(), 1);
    assert_eq!(model_turn.tool_calls[0].call_id, "call_write");
    assert_eq!(model_turn.tool_calls[0].name, "workspace.write_file");

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
    let model_completed = events
        .iter()
        .find(|event| event.event_type == "model_completed" && event.payload["round"] == json!(1))
        .expect("model completed event");
    assert_eq!(model_completed.payload["hasAssistantText"], json!(true));
    assert_eq!(model_completed.payload["hasReasoning"], json!(true));
    assert_eq!(model_completed.payload["assistantTextBytes"], json!(26));

    let tool_requested = events
        .iter()
        .find(|event| {
            event.event_type == "tool_call_requested"
                && event.payload["callId"].as_str() == Some("call_write")
        })
        .expect("tool call requested");
    assert_eq!(tool_requested.payload["round"], json!(1));

    tokio::fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn agent_loop_stores_tool_audit_files_with_hashed_call_id_paths() {
    let root = std::env::temp_dir().join(format!(
        "tauritavern-agent-tool-audit-{}",
        Uuid::new_v4().simple()
    ));
    let repository = Arc::new(FileAgentRepository::new(root.clone()));
    let run = AgentRun {
        id: "run_tool_audit_test".to_string(),
        workspace_id: "chat_tool_audit_test".to_string(),
        stable_chat_id: "stable_tool_audit_test".to_string(),
        chat_ref: AgentChatRef::Character {
            character_id: "Seraphina".to_string(),
            file_name: "Seraphina.png".to_string(),
        },
        generation_type: "normal".to_string(),
        profile_id: None,
        persist_base_state_id: None,
        presentation: AgentRunPresentation::Background,
        status: AgentRunStatus::Created,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    repository.create_run(&run).await.expect("create run");

    let opaque_call_id = format!(
        "call_{}___thought__{}/{}\\{} {}",
        "A".repeat(240),
        "B".repeat(240),
        "C".repeat(240),
        "思考".repeat(80),
        "D".repeat(240)
    );
    let model_gateway = Arc::new(MockAgentModelGateway::new(vec![
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": opaque_call_id,
                        "type": "function",
                        "function": {
                            "name": "workspace_write_file",
                            "arguments": "{\"path\":\"output/main.md\",\"content\":\"opaque call id survived\"}"
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
                        "id": "call_finish_after_opaque_id",
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
            "messages": prompt_messages("write a message")
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

    let arguments_ref = events
        .iter()
        .find(|event| {
            event.event_type == "tool_call_requested"
                && event.payload["callId"].as_str() == Some(opaque_call_id.as_str())
        })
        .and_then(|event| event.payload["argumentsRef"].as_str())
        .expect("arguments ref");
    assert_hashed_tool_audit_path(arguments_ref, "tool-args");

    let arguments_file = repository
        .read_text(&run.id, &WorkspacePath::parse(arguments_ref).unwrap())
        .await
        .expect("read arguments file");
    let arguments: Value = serde_json::from_str(&arguments_file.text).expect("arguments JSON");
    assert_eq!(arguments["path"], "output/main.md");

    let result_ref = events
        .iter()
        .find(|event| {
            event.event_type == "tool_result_stored"
                && event.payload["callId"].as_str() == Some(opaque_call_id.as_str())
        })
        .and_then(|event| event.payload["path"].as_str())
        .expect("result ref");
    assert_hashed_tool_audit_path(result_ref, "tool-results");

    let result_file = repository
        .read_text(&run.id, &WorkspacePath::parse(result_ref).unwrap())
        .await
        .expect("read result file");
    let result: Value = serde_json::from_str(&result_file.text).expect("result JSON");
    assert_eq!(result["callId"].as_str(), Some(opaque_call_id.as_str()));
    assert_eq!(result["structured"]["path"], "output/main.md");

    let model_requests = model_gateway_probe.requests().await;
    let second_request = model_requests.get(1).expect("second model request");
    let echoed_tool_result = second_request
        .messages
        .iter()
        .find(|message| message.role == AgentModelRole::Tool)
        .and_then(|message| message.parts.first())
        .and_then(|part| match part {
            AgentModelContentPart::ToolResult { result } => Some(result),
            _ => None,
        })
        .expect("tool result");
    assert_eq!(echoed_tool_result.call_id, opaque_call_id);
    wait_for_closed_sessions(
        &model_gateway_probe,
        vec!["run_tool_audit_test".to_string()],
    )
    .await;

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
        persist_base_state_id: None,
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
            "messages": prompt_messages("write a message")
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
        persist_base_state_id: None,
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
            "messages": prompt_messages("write a message")
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
        persist_base_state_id: None,
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
            "messages": prompt_messages("revise a message")
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
        persist_base_state_id: None,
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
            "messages": prompt_messages("write and remember")
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
        persist_base_state_id: Some(run.id.clone()),
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
        persist_base_state_id: None,
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
            "messages": prompt_messages("write visibly")
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

/// Issue #55 + #64 plus partial success: when the model commits, then
/// drifts by replying with plain text (no tool calls), the run gives the
/// model **one** corrective nudge. If it drifts AGAIN, the error remains
/// visible, but the already host-confirmed chat commit is preserved and
/// the terminal state becomes `partial_success` instead of rolling the
/// message back. This is the failure-after-recovery path; the success path
/// is covered by
/// [`foreground_run_recovers_from_post_commit_drift_with_nudge`].
#[tokio::test]
async fn foreground_run_keeps_committed_chat_as_partial_success_on_tool_call_required_drift() {
    let root = std::env::temp_dir().join(format!(
        "tauritavern-agent-drift-partial-success-{}",
        Uuid::new_v4().simple()
    ));
    let repository = Arc::new(FileAgentRepository::new(root.clone()));
    let run = AgentRun {
        id: "run_drift_rollback_test".to_string(),
        workspace_id: "chat_drift_rollback_test".to_string(),
        stable_chat_id: "stable_drift_rollback_test".to_string(),
        chat_ref: AgentChatRef::Character {
            character_id: "Seraphina".to_string(),
            file_name: "Seraphina.png".to_string(),
        },
        generation_type: "normal".to_string(),
        profile_id: None,
        persist_base_state_id: None,
        presentation: AgentRunPresentation::Foreground,
        status: AgentRunStatus::Created,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    repository.create_run(&run).await.expect("create run");

    let model_gateway = Arc::new(MockAgentModelGateway::new(vec![
        // Round 1: legitimately write the artifact.
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_write_drift",
                        "type": "function",
                        "function": {
                            "name": "workspace_write_file",
                            "arguments": "{\"path\":\"output/main.md\",\"content\":\"drift answer\"}"
                        }
                    }]
                }
            }]
        }),
        // Round 2: commit the artifact. This is the host-confirmed chat
        // output partial success must preserve if the later run fails.
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_commit_drift",
                        "type": "function",
                        "function": {
                            "name": "workspace_commit",
                            "arguments": "{}"
                        }
                    }]
                }
            }]
        }),
        // Round 3: drift — return plain text and no tool calls. With #64
        // this triggers ONE soft recovery attempt (a corrective `user`
        // message gets injected); the run does not fail yet.
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "Sorry, here is the full answer one more time...",
                }
            }]
        }),
        // Round 4: stubborn drift — model ignores the nudge and replies
        // with plain text again. Recovery budget is now exhausted, so the
        // run records a partial success preserving the committed message.
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "Sorry, here it is one more time...",
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
            "messages": prompt_messages("drift after commit")
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
        "message_42",
    ));
    let outcome = service
        .execute_agent_loop_run_inner(
            &run.id,
            prompt_snapshot,
            request,
            profile,
            &mut cancel_receiver,
        )
        .await;
    resolver.await.expect("resolver task");

    assert!(
        outcome.is_err(),
        "partial success keeps the committed chat but must still expose the underlying error"
    );

    let saved = repository.load_run(&run.id).await.expect("load run");
    assert_eq!(saved.status, AgentRunStatus::PartialSuccess);

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

    // #64: the first drift must have produced exactly one
    // `drift_recovery_attempted` event before we surrendered to the terminal
    // partial-success path. If this assertion fails, the recovery attempt
    // was bypassed — a regression of #64.
    let recovery_events: Vec<_> = events
        .iter()
        .filter(|event| event.event_type == "drift_recovery_attempted")
        .collect();
    assert_eq!(
        recovery_events.len(),
        1,
        "exactly one drift_recovery_attempted event must precede partial success"
    );
    assert_eq!(recovery_events[0].level, AgentRunEventLevel::Warn);
    assert_eq!(recovery_events[0].payload["attempt"], 1);
    assert_eq!(recovery_events[0].payload["maxAttempts"], 1);
    assert_eq!(recovery_events[0].payload["committedCount"], 1);
    assert_eq!(
        recovery_events[0].payload["reasonCode"],
        "model.tool_call_required"
    );

    assert!(
        !events
            .iter()
            .any(|event| event.event_type == "run_rollback_targets"),
        "partial success must not auto-rollback committed chat output"
    );
    assert!(
        !events.iter().any(|event| event.event_type == "run_failed"),
        "partial success is its own terminal event, not run_failed"
    );
    assert!(
        !events
            .iter()
            .any(|event| event.event_type == "run_completed"),
        "partial success must not masquerade as clean completion"
    );

    let partial = events
        .iter()
        .find(|event| event.event_type == "run_partial_success")
        .expect("run_partial_success event must be emitted on drift after commit");
    assert_eq!(partial.level, AgentRunEventLevel::Warn);
    assert_eq!(partial.payload["code"], "model.tool_call_required");
    assert_eq!(partial.payload["retryable"], false);
    assert_eq!(partial.payload["userRetryable"], false);
    assert_eq!(partial.payload["preservedCommitCount"], 1);
    let targets = partial.payload["preservedCommits"]
        .as_array()
        .expect("preserved commits array");
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0]["messageId"], "message_42");
    assert_eq!(targets[0]["path"], "output/main.md");

    tokio::fs::remove_dir_all(root).await.expect("cleanup");
}

/// Issue #64: when the model commits and then drifts (plain text, no
/// tool calls), the loop must inject a corrective `user` reminder and
/// give the model one more chance to call `workspace_finish`. If the
/// model complies, the run completes normally — the commit is NOT
/// rolled back. This is the happy-path complement to
/// [`foreground_run_keeps_committed_chat_as_partial_success_on_tool_call_required_drift`].
#[tokio::test]
async fn foreground_run_recovers_from_post_commit_drift_with_nudge() {
    let root = std::env::temp_dir().join(format!(
        "tauritavern-agent-drift-recovery-{}",
        Uuid::new_v4().simple()
    ));
    let repository = Arc::new(FileAgentRepository::new(root.clone()));
    let run = AgentRun {
        id: "run_drift_recovery_test".to_string(),
        workspace_id: "chat_drift_recovery_test".to_string(),
        stable_chat_id: "stable_drift_recovery_test".to_string(),
        chat_ref: AgentChatRef::Character {
            character_id: "Seraphina".to_string(),
            file_name: "Seraphina.png".to_string(),
        },
        generation_type: "normal".to_string(),
        profile_id: None,
        persist_base_state_id: None,
        presentation: AgentRunPresentation::Foreground,
        status: AgentRunStatus::Created,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    repository.create_run(&run).await.expect("create run");

    let model_gateway = Arc::new(MockAgentModelGateway::new(vec![
        // Round 1: write artifact.
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_write_recovery",
                        "type": "function",
                        "function": {
                            "name": "workspace_write_file",
                            "arguments": "{\"path\":\"output/main.md\",\"content\":\"recovered answer\"}"
                        }
                    }]
                }
            }]
        }),
        // Round 2: commit it.
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_commit_recovery",
                        "type": "function",
                        "function": {
                            "name": "workspace_commit",
                            "arguments": "{}"
                        }
                    }]
                }
            }]
        }),
        // Round 3: drift — model replies in plain text instead of calling
        // workspace_finish. #64 injects a corrective nudge.
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "Oh and here's the answer in chat form...",
                }
            }]
        }),
        // Round 4: model reads the nudge and complies — calls
        // workspace_finish. Run should complete cleanly.
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_finish_recovery",
                        "type": "function",
                        "function": {
                            "name": "workspace_finish",
                            "arguments": "{\"reason\":\"recovered after drift\"}"
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
        model_gateway.clone(),
        test_profile_service(&root),
    ));
    let request = ChatCompletionGenerateRequestDto {
        payload: json!({
            "chat_completion_source": "openai",
            "model": "test-model",
            "messages": prompt_messages("drift recovery happy path")
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
        "message_recovery_42",
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
        .expect("recovery should let the run complete");
    resolver.await.expect("resolver task");

    let saved = repository.load_run(&run.id).await.expect("load run");
    assert_eq!(saved.status, AgentRunStatus::Completed);

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

    let recovery_events: Vec<_> = events
        .iter()
        .filter(|event| event.event_type == "drift_recovery_attempted")
        .collect();
    assert_eq!(
        recovery_events.len(),
        1,
        "recovery must fire exactly once for the single drift event"
    );
    assert_eq!(recovery_events[0].level, AgentRunEventLevel::Warn);
    assert_eq!(recovery_events[0].payload["attempt"], 1);
    assert_eq!(recovery_events[0].payload["maxAttempts"], 1);
    assert_eq!(recovery_events[0].payload["committedCount"], 1);

    // The commit must NOT be rolled back when recovery succeeds.
    assert!(
        !events
            .iter()
            .any(|event| event.event_type == "run_rollback_targets"),
        "no rollback target events when recovery succeeds"
    );
    // The corrective nudge must reach the model — verify the message
    // list grew with both the drifted assistant turn and our synthetic
    // user reminder before round 4. The 4th model request should have
    // received them.
    let requests = model_gateway.requests().await;
    assert_eq!(
        requests.len(),
        4,
        "model must be called exactly 4 times (3 normal + 1 recovery)"
    );
    let last_request = requests.last().unwrap();
    let drift_user_message = last_request
        .messages
        .iter()
        .rev()
        .find(|m| matches!(m.role, AgentModelRole::User))
        .expect("recovery nudge must be present as user message");
    let nudge_text = drift_user_message
        .parts
        .iter()
        .find_map(|part| match part {
            AgentModelContentPart::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .unwrap_or("");
    assert!(
        nudge_text.contains("drift recovery attempt 1/1"),
        "nudge must include attempt counter; got: {nudge_text}"
    );
    assert!(
        nudge_text.contains("workspace_finish"),
        "nudge must reference workspace_finish; got: {nudge_text}"
    );
    assert!(
        nudge_text.contains("workspace_commit again before workspace_finish"),
        "nudge must require another commit after revising workspace files; got: {nudge_text}"
    );
}

/// Issue #64: when the model drifts WITHOUT having committed anything,
/// the loop still tries one corrective nudge (per user decision). On
/// recovery the model proceeds normally.
#[tokio::test]
async fn foreground_run_recovers_from_no_commit_drift_with_nudge() {
    let root = std::env::temp_dir().join(format!(
        "tauritavern-agent-drift-recovery-nocommit-{}",
        Uuid::new_v4().simple()
    ));
    let repository = Arc::new(FileAgentRepository::new(root.clone()));
    let run = AgentRun {
        id: "run_drift_recovery_nocommit_test".to_string(),
        workspace_id: "chat_drift_recovery_nocommit_test".to_string(),
        stable_chat_id: "stable_drift_recovery_nocommit_test".to_string(),
        chat_ref: AgentChatRef::Character {
            character_id: "Seraphina".to_string(),
            file_name: "Seraphina.png".to_string(),
        },
        generation_type: "normal".to_string(),
        profile_id: None,
        persist_base_state_id: None,
        presentation: AgentRunPresentation::Foreground,
        status: AgentRunStatus::Created,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    repository.create_run(&run).await.expect("create run");

    let model_gateway = Arc::new(MockAgentModelGateway::new(vec![
        // Round 1: drift right away — no tool calls.
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "Sure, here is the answer directly...",
                }
            }]
        }),
        // Round 2: model recovers and writes a file.
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_write_nocommit",
                        "type": "function",
                        "function": {
                            "name": "workspace_write_file",
                            "arguments": "{\"path\":\"output/main.md\",\"content\":\"nudge worked\"}"
                        }
                    }]
                }
            }]
        }),
        // Round 3: commit.
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_commit_nocommit",
                        "type": "function",
                        "function": {
                            "name": "workspace_commit",
                            "arguments": "{}"
                        }
                    }]
                }
            }]
        }),
        // Round 4: finish.
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_finish_nocommit",
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
        model_gateway.clone(),
        test_profile_service(&root),
    ));
    let request = ChatCompletionGenerateRequestDto {
        payload: json!({
            "chat_completion_source": "openai",
            "model": "test-model",
            "messages": prompt_messages("no-commit drift recovery")
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
        "message_nocommit_42",
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
        .expect("recovery should let the run complete");
    resolver.await.expect("resolver task");

    let saved = repository.load_run(&run.id).await.expect("load run");
    assert_eq!(saved.status, AgentRunStatus::Completed);

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
    let recovery_events: Vec<_> = events
        .iter()
        .filter(|event| event.event_type == "drift_recovery_attempted")
        .collect();
    assert_eq!(recovery_events.len(), 1);
    assert_eq!(recovery_events[0].payload["committedCount"], 0);

    assert!(
        !events
            .iter()
            .any(|event| event.event_type == "run_rollback_targets"),
        "no rollback when no commits existed before recovery"
    );
}

#[tokio::test]
async fn foreground_run_without_commit_still_fails_after_drift_recovery_exhausts() {
    let root = std::env::temp_dir().join(format!(
        "tauritavern-agent-drift-no-commit-failure-{}",
        Uuid::new_v4().simple()
    ));
    let repository = Arc::new(FileAgentRepository::new(root.clone()));
    let run = AgentRun {
        id: "run_drift_no_commit_failure_test".to_string(),
        workspace_id: "chat_drift_no_commit_failure_test".to_string(),
        stable_chat_id: "stable_drift_no_commit_failure_test".to_string(),
        chat_ref: AgentChatRef::Character {
            character_id: "Seraphina".to_string(),
            file_name: "Seraphina.png".to_string(),
        },
        generation_type: "normal".to_string(),
        profile_id: None,
        persist_base_state_id: None,
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
                    "content": "I will answer directly instead of using tools.",
                }
            }]
        }),
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "Still answering directly.",
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
            "messages": prompt_messages("no commit stubborn drift")
        })
        .as_object()
        .cloned()
        .unwrap(),
    };
    let prompt_snapshot = json!({ "chatCompletionPayload": request.payload.clone() });
    let (_cancel_sender, mut cancel_receiver) = watch::channel(false);
    let mut profile = test_resolved_profile(&root).await;
    profile.run.presentation = AgentRunPresentation::Foreground;

    let outcome = service
        .execute_agent_loop_run_inner(
            &run.id,
            prompt_snapshot,
            request,
            profile,
            &mut cancel_receiver,
        )
        .await;
    assert!(
        outcome.is_err(),
        "no-commit drift must remain a hard failure"
    );

    let saved = repository.load_run(&run.id).await.expect("load run");
    assert_eq!(saved.status, AgentRunStatus::Failed);

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
            .any(|event| event.event_type == "drift_recovery_attempted")
    );
    assert!(
        events.iter().any(|event| {
            event.event_type == "run_failed"
                && event.payload["code"] == json!("model.tool_call_required")
                && event.payload["userRetryable"] == json!(true)
        }),
        "no-commit drift should expose the existing user-retryable failure"
    );
    assert!(
        !events
            .iter()
            .any(|event| event.event_type == "run_partial_success"),
        "partial success requires at least one successful chat commit"
    );

    tokio::fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn foreground_run_with_commit_becomes_partial_success_when_persistent_commit_fails() {
    let root = std::env::temp_dir().join(format!(
        "tauritavern-agent-persistent-partial-success-{}",
        Uuid::new_v4().simple()
    ));
    let repository = Arc::new(FileAgentRepository::new(root.clone()));
    let workspace_repository = Arc::new(FailingPersistentCommitWorkspaceRepository {
        inner: repository.clone(),
    });
    let run = AgentRun {
        id: "run_persistent_partial_success_test".to_string(),
        workspace_id: "chat_persistent_partial_success_test".to_string(),
        stable_chat_id: "stable_persistent_partial_success_test".to_string(),
        chat_ref: AgentChatRef::Character {
            character_id: "Seraphina".to_string(),
            file_name: "Seraphina.png".to_string(),
        },
        generation_type: "normal".to_string(),
        profile_id: None,
        persist_base_state_id: None,
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
                        "id": "call_write_persistent_failure",
                        "type": "function",
                        "function": {
                            "name": "workspace_write_file",
                            "arguments": "{\"path\":\"output/main.md\",\"content\":\"visible answer\"}"
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
                        "id": "call_commit_persistent_failure",
                        "type": "function",
                        "function": {
                            "name": "workspace_commit",
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
                    "tool_calls": [{
                        "id": "call_finish_persistent_failure",
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
        workspace_repository,
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
            "messages": prompt_messages("persistent failure after commit")
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
        "message_persistent_failure",
    ));
    let outcome = service
        .execute_agent_loop_run_inner(
            &run.id,
            prompt_snapshot,
            request,
            profile,
            &mut cancel_receiver,
        )
        .await;
    resolver.await.expect("resolver task");
    assert!(
        outcome.is_err(),
        "persistent commit failure must still expose the underlying error"
    );

    let saved = repository.load_run(&run.id).await.expect("load run");
    assert_eq!(saved.status, AgentRunStatus::PartialSuccess);

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
    assert!(
        events
            .iter()
            .any(|event| event.event_type == "persistent_changes_commit_failed")
    );
    assert!(
        events.iter().any(|event| {
            event.event_type == "run_partial_success"
                && event.payload["code"] == json!("agent.test_persistent_failure")
                && event.payload["preservedCommitCount"] == json!(1)
        }),
        "persistent commit failure after chat commit should become partial success"
    );
    assert!(
        !events
            .iter()
            .any(|event| event.event_type == "run_completed"),
        "persistent failure must not masquerade as clean completion"
    );
    assert!(
        !events.iter().any(|event| event.event_type == "run_failed"),
        "partial success is its own terminal event"
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
        persist_base_state_id: None,
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
            "messages": prompt_messages("finish too early then recover")
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
        persist_base_state_id: None,
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
            "messages": prompt_messages("recover from tool error")
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
        persist_base_state_id: None,
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

    let dispatcher = test_dispatcher(repository.clone(), &root);
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
        persist_base_state_id: None,
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
async fn dispatcher_searches_visible_workspace_files_and_reads_char_ranges() {
    let root = std::env::temp_dir().join(format!(
        "tauritavern-agent-workspace-search-{}",
        Uuid::new_v4().simple()
    ));
    let repository = Arc::new(FileAgentRepository::new(root.clone()));
    let run = AgentRun {
        id: "run_workspace_search_test".to_string(),
        workspace_id: "workspace_search_test".to_string(),
        stable_chat_id: "stable_workspace_search_test".to_string(),
        chat_ref: AgentChatRef::Character {
            character_id: "alice".to_string(),
            file_name: "session".to_string(),
        },
        generation_type: "normal".to_string(),
        profile_id: None,
        persist_base_state_id: None,
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
            &WorkspacePath::parse("persist/memory.md").unwrap(),
            "alpha\nblue lantern under the bridge\nomega",
        )
        .await
        .expect("seed persist");

    let dispatcher = test_dispatcher(repository.clone(), &root);
    let mut session = AgentToolSession::default();
    let search_call = AgentToolCall {
        id: "call_workspace_search".to_string(),
        name: "workspace.search_files".to_string(),
        arguments: json!({ "query": "blue lantern", "path": "persist/", "context_lines": 0 }),
        provider_metadata: Value::Null,
    };
    let searched = dispatcher
        .dispatch(&run.id, &search_call, &mut session, &profile)
        .await
        .expect("dispatch workspace search");
    assert!(!searched.result.is_error);
    assert_eq!(
        searched.result.structured["hits"][0]["path"],
        "persist/memory.md"
    );
    assert_eq!(searched.result.structured["hits"][0]["startLine"], 2);

    let char_read_call = AgentToolCall {
        id: "call_workspace_char_read".to_string(),
        name: "workspace.read_file".to_string(),
        arguments: json!({ "path": "persist/memory.md", "start_char": 6, "max_chars": 12 }),
        provider_metadata: Value::Null,
    };
    let read = dispatcher
        .dispatch(&run.id, &char_read_call, &mut session, &profile)
        .await
        .expect("dispatch char read");
    assert!(!read.result.is_error);
    assert!(read.result.content.contains("blue lantern"));
    assert_eq!(read.result.structured["startChar"], 6);
    assert_eq!(read.result.structured["endChar"], 18);

    tokio::fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn dispatcher_searches_skills_and_reads_skill_ranges() {
    let root = std::env::temp_dir().join(format!(
        "tauritavern-agent-skill-search-{}",
        Uuid::new_v4().simple()
    ));
    let repository = Arc::new(FileAgentRepository::new(root.clone()));
    let skill_repository = Arc::new(FileSkillRepository::new(root.join("skills")));
    skill_repository
        .install_import(SkillInstallRequest {
            input: SkillImportInput::InlineFiles {
                files: vec![
                    SkillInlineFile {
                        path: "SKILL.md".to_string(),
                        encoding: "utf8".to_string(),
                        content: "---\nname: test-skill\ndescription: Skill for search tests.\n---\n\n# Test\n".to_string(),
                        media_type: None,
                        size_bytes: None,
                        sha256: None,
                    },
                    SkillInlineFile {
                        path: "references/guide.md".to_string(),
                        encoding: "utf8".to_string(),
                        content: "alpha\nblue lantern under the bridge\nomega".to_string(),
                        media_type: None,
                        size_bytes: None,
                        sha256: None,
                    },
                ],
                source: json!({ "kind": "test" }),
            },
            conflict_strategy: None,
        })
        .await
        .expect("install skill");
    let skill_service = Arc::new(SkillService::new(skill_repository));
    let profile = test_resolved_profile(&root).await;
    let dispatcher = AgentToolDispatcher::new(
        repository.clone(),
        test_chat_repository(&root),
        test_chat_repository(&root),
        repository,
        skill_service,
    );
    let mut session = AgentToolSession::default();

    let search_call = AgentToolCall {
        id: "call_skill_search".to_string(),
        name: "skill.search".to_string(),
        arguments: json!({ "name": "test-skill", "query": "blue lantern", "path": "references", "context_lines": 0 }),
        provider_metadata: Value::Null,
    };
    let searched = dispatcher
        .dispatch("unused", &search_call, &mut session, &profile)
        .await
        .expect("dispatch skill search");
    assert!(!searched.result.is_error);
    assert_eq!(
        searched.result.structured["hits"][0]["path"],
        "references/guide.md"
    );

    let read_call = AgentToolCall {
        id: "call_skill_read_range".to_string(),
        name: "skill.read".to_string(),
        arguments: json!({
            "name": "test-skill",
            "path": "references/guide.md",
            "start_line": 2,
            "line_count": 1
        }),
        provider_metadata: Value::Null,
    };
    let read = dispatcher
        .dispatch("unused", &read_call, &mut session, &profile)
        .await
        .expect("dispatch skill read");
    assert!(!read.result.is_error);
    assert!(
        read.result
            .content
            .contains("blue lantern under the bridge")
    );
    assert_eq!(read.result.structured["startLine"], 2);
    assert_eq!(read.result.structured["endLine"], 2);

    tokio::fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn dispatcher_progressively_reads_worldinfo_activation_from_run_snapshot() {
    let root = std::env::temp_dir().join(format!(
        "tauritavern-agent-worldinfo-tool-{}",
        Uuid::new_v4().simple()
    ));
    let repository = Arc::new(FileAgentRepository::new(root.clone()));
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
        persist_base_state_id: None,
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
                    "messages": prompt_messages("hello")
                },
                "worldInfoActivation": {
                    "timestampMs": 1,
                    "trigger": "normal",
                    "entries": [
                        {
                            "world": "lorebook",
                            "uid": 7,
                            "displayName": "Hidden bridge",
                            "constant": false,
                            "position": "before",
                            "content": "The bridge has a hidden blue lantern."
                        },
                        {
                            "world": "lorebook",
                            "uid": 8,
                            "displayName": "Clock tower",
                            "constant": true,
                            "position": "after",
                            "content": "The clock tower bell rings only for agents."
                        }
                    ]
                }
            }),
            &profile,
        )
        .await
        .expect("initialize workspace");

    let dispatcher = test_dispatcher(repository.clone(), &root);
    let mut session = AgentToolSession::default();
    let index_call = AgentToolCall {
        id: "call_worldinfo_index".to_string(),
        name: "worldinfo.read_activated".to_string(),
        arguments: json!({}),
        provider_metadata: Value::Null,
    };
    let index = dispatcher
        .dispatch(&run.id, &index_call, &mut session, &profile)
        .await
        .expect("dispatch worldinfo index");

    assert!(!index.result.is_error);
    assert_eq!(index.result.structured["mode"], "index");
    assert_eq!(index.result.structured["totalEntries"], 2);
    assert_eq!(
        index.result.structured["entries"][0]["ref"],
        "worldinfo:lorebook#7"
    );
    assert_eq!(
        index.result.structured["entries"][0]["totalChars"],
        "The bridge has a hidden blue lantern.".chars().count()
    );
    assert!(
        index.result.structured["entries"][0]
            .get("content")
            .is_none()
    );
    assert!(index.result.content.contains("Content is omitted"));
    assert!(index.result.content.contains("worldinfo:lorebook#7"));
    assert!(!index.result.content.contains("hidden blue lantern"));
    assert_eq!(index.result.resource_refs[0], "worldinfo:lorebook#7");
    assert_eq!(index.result.resource_refs[1], "worldinfo:lorebook#8");

    let read_call = AgentToolCall {
        id: "call_worldinfo_read".to_string(),
        name: "worldinfo.read_activated".to_string(),
        arguments: json!({
            "entries": [{
                "ref": "worldinfo:lorebook#7",
                "start_char": 4,
                "max_chars": 6
            }]
        }),
        provider_metadata: Value::Null,
    };
    let read = dispatcher
        .dispatch(&run.id, &read_call, &mut session, &profile)
        .await
        .expect("dispatch worldinfo read");

    assert!(!read.result.is_error);
    assert_eq!(read.result.structured["mode"], "content");
    assert_eq!(read.result.structured["entries"][0]["content"], "bridge");
    assert_eq!(read.result.structured["entries"][0]["startChar"], 4);
    assert_eq!(read.result.structured["entries"][0]["endChar"], 10);
    assert_eq!(read.result.structured["entries"][0]["truncated"], true);
    assert_eq!(read.result.resource_refs[0], "worldinfo:lorebook#7");

    let missing_ref_call = AgentToolCall {
        id: "call_worldinfo_missing".to_string(),
        name: "worldinfo.read_activated".to_string(),
        arguments: json!({
            "entries": [{ "ref": "worldinfo:lorebook#404" }]
        }),
        provider_metadata: Value::Null,
    };
    let missing_ref = dispatcher
        .dispatch(&run.id, &missing_ref_call, &mut session, &profile)
        .await
        .expect("dispatch missing worldinfo ref");
    assert!(missing_ref.result.is_error);
    assert_eq!(
        missing_ref.result.error_code.as_deref(),
        Some("worldinfo.entry_not_found")
    );

    let old_max_chars_call = AgentToolCall {
        id: "call_worldinfo_old_arg".to_string(),
        name: "worldinfo.read_activated".to_string(),
        arguments: json!({ "max_chars": 20000 }),
        provider_metadata: Value::Null,
    };
    let old_max_chars = dispatcher
        .dispatch(&run.id, &old_max_chars_call, &mut session, &profile)
        .await
        .expect("dispatch obsolete worldinfo arg");
    assert!(old_max_chars.result.is_error);
    assert_eq!(
        old_max_chars.result.error_code.as_deref(),
        Some("tool.invalid_arguments")
    );

    tokio::fs::remove_dir_all(root).await.expect("cleanup");
}

fn prompt_messages(user_content: &str) -> Value {
    json!([
        agent_system_marker(),
        {
            "role": "user",
            "content": user_content,
        }
    ])
}

fn assert_hashed_tool_audit_path(path: &str, root: &str) {
    const TOOL_CALL_AUDIT_DIGEST_HEX_CHARS: usize = 16;

    let prefix = format!("{root}/call_");
    assert!(path.starts_with(&prefix), "{path}");
    assert!(path.ends_with(".json"), "{path}");
    assert_eq!(
        path.len(),
        prefix.len() + TOOL_CALL_AUDIT_DIGEST_HEX_CHARS + ".json".len(),
        "{path}"
    );
    let digest = &path[prefix.len()..path.len() - ".json".len()];
    assert!(
        digest.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "{path}"
    );
}

fn agent_system_marker() -> Value {
    json!({
        "role": "system",
        "content": "[marker]",
        "_tauritavern_agent_prompt_marker": "agentSystemPrompt"
    })
}

struct MockAgentModelGateway {
    responses: Mutex<VecDeque<Result<Value, ApplicationError>>>,
    requests: Mutex<Vec<AgentModelRequest>>,
    closed_sessions: Mutex<Vec<String>>,
}

fn test_skill_service(root: &Path) -> Arc<SkillService> {
    Arc::new(SkillService::new(Arc::new(FileSkillRepository::new(
        root.join("skills"),
    ))))
}

fn test_chat_repository(root: &Path) -> Arc<FileChatRepository> {
    Arc::new(FileChatRepository::new(
        root.join("characters"),
        root.join("chats"),
        root.join("group_chats"),
        root.join("backups"),
    ))
}

fn test_dispatcher(repository: Arc<FileAgentRepository>, root: &Path) -> AgentToolDispatcher {
    let chat_repository = test_chat_repository(root);
    AgentToolDispatcher::new(
        repository.clone(),
        chat_repository.clone(),
        chat_repository,
        repository,
        test_skill_service(root),
    )
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

struct FailingPersistentCommitWorkspaceRepository {
    inner: Arc<FileAgentRepository>,
}

#[async_trait]
impl WorkspaceRepository for FailingPersistentCommitWorkspaceRepository {
    async fn initialize_run(
        &self,
        run: &AgentRun,
        manifest: &WorkspaceManifest,
        prompt_snapshot: &Value,
        resolved_profile: &ResolvedAgentProfile,
    ) -> Result<(), DomainError> {
        self.inner
            .initialize_run(run, manifest, prompt_snapshot, resolved_profile)
            .await
    }

    async fn read_manifest(&self, run_id: &str) -> Result<WorkspaceManifest, DomainError> {
        self.inner.read_manifest(run_id).await
    }

    async fn write_text(
        &self,
        run_id: &str,
        path: &WorkspacePath,
        text: &str,
    ) -> Result<WorkspaceFile, DomainError> {
        self.inner.write_text(run_id, path, text).await
    }

    async fn read_text(
        &self,
        run_id: &str,
        path: &WorkspacePath,
    ) -> Result<WorkspaceFile, DomainError> {
        self.inner.read_text(run_id, path).await
    }

    async fn list_files(
        &self,
        run_id: &str,
        path: Option<&WorkspacePath>,
        depth: usize,
        max_entries: usize,
    ) -> Result<WorkspaceFileList, DomainError> {
        self.inner
            .list_files(run_id, path, depth, max_entries)
            .await
    }

    async fn prepare_persistent_changes(
        &self,
        run_id: &str,
    ) -> Result<WorkspacePersistentChangeSet, DomainError> {
        self.inner.prepare_persistent_changes(run_id).await
    }

    async fn commit_persistent_changes(
        &self,
        _run_id: &str,
    ) -> Result<WorkspacePersistentChangeSet, DomainError> {
        Err(DomainError::InternalError(
            "agent.test_persistent_failure: simulated persistent commit failure".to_string(),
        ))
    }
}

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
