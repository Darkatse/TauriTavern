use super::*;

#[tokio::test]
async fn agent_runtime_loop_uses_real_file_repositories() {
    let root = temp_root("agent-runtime");
    let fixture = agent_runtime_fixture(&root);
    let registry = BuiltinAgentToolRegistry::phase2c();
    let mut profile = fixture
        .profile_service
        .resolve_profile(AgentProfileResolveInput {
            profile_id: None,
            known_tools: registry.specs(),
        })
        .await
        .expect("resolve default profile");
    profile.run.presentation = AgentRunPresentation::Background;
    profile.tools.max_rounds = 2;

    let run = AgentRun {
        id: "run_contract".to_string(),
        workspace_id: "stable_contract".to_string(),
        stable_chat_id: "stable_contract".to_string(),
        chat_ref: AgentChatRef::Character {
            character_id: "Alice".to_string(),
            file_name: "Alice.png".to_string(),
        },
        generation_type: "normal".to_string(),
        profile_id: Some(profile.id.as_str().to_string()),
        skill_scope_refs: Default::default(),
        persist_base_state_id: None,
        input_message_count: None,
        presentation: AgentRunPresentation::Background,
        status: AgentRunStatus::Created,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    fixture
        .agent_repository
        .create_run(&run)
        .await
        .expect("create run");

    let request = chat_request("write a short file");
    let prompt_snapshot = json!({ "chatCompletionPayload": request.payload.clone() });
    let (_cancel_sender, mut cancel_receiver) = watch::channel(false);

    fixture
        .service
        .execute_agent_loop_run_inner(
            &run.id,
            prompt_snapshot,
            request,
            profile,
            &mut cancel_receiver,
        )
        .await
        .expect("execute agent loop");

    let saved = fixture
        .agent_repository
        .load_run(&run.id)
        .await
        .expect("load run");
    assert_eq!(saved.status, AgentRunStatus::Completed);
    let artifact = fixture
        .agent_repository
        .read_text(&run.id, &WorkspacePath::parse("output/main.md").unwrap())
        .await
        .expect("read artifact");
    assert_eq!(artifact.text, "hello from real repo");
    let events = fixture
        .agent_repository
        .read_events(
            &run.id,
            AgentRunEventReadQuery {
                after_seq: Some(0),
                before_seq: None,
                limit: 100,
                invocation_id: None,
            },
        )
        .await
        .expect("read events");
    let model_completed = events
        .iter()
        .find(|event| event.event_type == "model_completed")
        .expect("model completed event");
    assert_eq!(
        model_completed.payload["modelResponsePath"],
        "model-responses/round-001.json"
    );
    let write_event = events
        .iter()
        .find(|event| event.event_type == "workspace_file_written")
        .expect("workspace file written event");
    assert_eq!(write_event.payload["path"], "output/main.md");
    assert_eq!(write_event.payload["mode"], "replace");
    assert_eq!(write_event.payload["chars"], 20);
    let tool_requested = events
        .iter()
        .find(|event| {
            event.event_type == "tool_call_requested" && event.payload["callId"] == "call_write"
        })
        .expect("tool call requested event");
    let arguments_ref = tool_requested.payload["argumentsRef"]
        .as_str()
        .expect("arguments ref");
    assert!(arguments_ref.starts_with("tool-args/call_"));
    let arguments = read_workspace_json(&fixture.agent_repository, &run.id, arguments_ref).await;
    assert_eq!(arguments["path"], "output/main.md");
    let result_stored = events
        .iter()
        .find(|event| {
            event.event_type == "tool_result_stored" && event.payload["callId"] == "call_write"
        })
        .expect("tool result stored event");
    let result_ref = result_stored.payload["path"].as_str().expect("result ref");
    assert!(result_ref.starts_with("tool-results/call_"));
    let result = read_workspace_json(&fixture.agent_repository, &run.id, result_ref).await;
    assert_eq!(result["name"], "workspace.write_file");
    assert!(
        events
            .iter()
            .any(|event| event.event_type == "model_response_stored")
    );
    assert!(
        fixture
            .model_gateway
            .requests()
            .await
            .iter()
            .any(|request| request
                .tools
                .iter()
                .any(|tool| tool.name == "workspace.write_file"))
    );
    wait_for_closed_sessions(
        &fixture.model_gateway,
        vec!["run_contract:inv_root".to_string()],
    )
    .await;

    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn agent_runtime_agent_list_discovers_callable_profiles_with_real_repositories() {
    let root = temp_root("agent-list");
    let fixture = agent_runtime_fixture_with_responses(
        &root,
        vec![
            json!({
                "choices": [{
                    "message": {
                        "role": "assistant",
                        "content": null,
                        "tool_calls": [{
                            "id": "call_agent_list",
                            "type": "function",
                            "function": {
                                "name": "agent_list",
                                "arguments": "{\"purpose\":\"delegate\"}"
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
                                "id": "call_write_after_list",
                                "type": "function",
                                "function": {
                                    "name": "workspace_write_file",
                                    "arguments": "{\"path\":\"output/main.md\",\"content\":\"listed agents\"}"
                                }
                            },
                            {
                                "id": "call_finish_after_list",
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
        ],
    );
    let mut callable = fixture
        .profile_service
        .load_profile("default-writer")
        .await
        .expect("load default profile")
        .expect("default profile exists");
    callable.id = AgentProfileId::parse("scene-editor").expect("profile id");
    callable.display_name = "Scene Editor".to_string();
    callable.description = Some("Edits a draft scene for continuity.".to_string());
    callable.tools.allow.retain(|name| {
        !matches!(
            name.as_str(),
            "agent.list" | "agent.delegate" | "agent.await"
        )
    });
    callable.delegation = AgentDelegationPolicy {
        callable: true,
        allow_as_subagent: true,
        allowed_callers: vec!["default-writer".to_string()],
        description_for_agents: Some("Continuity editor for scene drafts.".to_string()),
        ..Default::default()
    };
    fixture
        .profile_service
        .save_profile(callable, fixture.service.tool_specs())
        .await
        .expect("save callable profile");
    let mut profile = resolve_contract_profile(&fixture).await;
    profile.run.presentation = AgentRunPresentation::Background;
    profile.tools.max_rounds = 2;
    let run = contract_run(
        "run_agent_list_contract",
        AgentRunPresentation::Background,
        &profile,
    );
    fixture
        .agent_repository
        .create_run(&run)
        .await
        .expect("create run");
    let request = chat_request("list callable agents");
    let prompt_snapshot = json!({ "chatCompletionPayload": request.payload.clone() });
    let (_cancel_sender, mut cancel_receiver) = watch::channel(false);

    fixture
        .service
        .execute_agent_loop_run_inner(
            &run.id,
            prompt_snapshot,
            request,
            profile,
            &mut cancel_receiver,
        )
        .await
        .expect("agent loop");

    let requests = fixture.model_gateway.requests().await;
    let list_results = tool_result_structured_values(&requests[1], "agent.list");
    assert_eq!(list_results.len(), 1);
    assert_eq!(list_results[0]["agents"][0]["profileId"], "scene-editor");
    assert_eq!(
        list_results[0]["agents"][0]["operations"],
        json!(["delegate"])
    );
    assert_eq!(
        list_results[0]["agents"][0]["description"],
        "Continuity editor for scene drafts."
    );

    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn agent_runtime_foreground_commit_guard_and_host_resolution_use_real_repositories() {
    let root = temp_root("agent-foreground");
    let fixture = agent_runtime_fixture_with_responses(
        &root,
        vec![
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
                                    "arguments": "{\"path\":\"output/main.md\",\"content\":\"foreground answer\"}"
                                }
                            },
                            {
                                "id": "call_commit_after_guard",
                                "type": "function",
                                "function": {
                                    "name": "workspace_commit",
                                    "arguments": "{}"
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
        ],
    );
    let mut profile = resolve_contract_profile(&fixture).await;
    profile.run.presentation = AgentRunPresentation::Foreground;
    profile.tools.max_rounds = 3;
    let run = contract_run(
        "run_foreground_contract",
        AgentRunPresentation::Foreground,
        &profile,
    );
    fixture
        .agent_repository
        .create_run(&run)
        .await
        .expect("create run");
    let request = chat_request("finish too early then recover");
    let prompt_snapshot = json!({ "chatCompletionPayload": request.payload.clone() });
    let (_cancel_sender, mut cancel_receiver) = watch::channel(false);

    execute_agent_loop_with_host_resolver(
        fixture.service.clone(),
        run.id.clone(),
        prompt_snapshot,
        request,
        profile,
        &mut cancel_receiver,
        resolve_next_chat_commit_and_persistent_state_update(
            fixture.service.clone(),
            fixture.agent_repository.clone(),
            run.id.clone(),
            "message_1",
        ),
    )
    .await
    .expect("agent loop");

    let saved = fixture
        .agent_repository
        .load_run(&run.id)
        .await
        .expect("load run");
    assert_eq!(saved.status, AgentRunStatus::Completed);
    let events = read_agent_events(&fixture.agent_repository, &run.id).await;
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
    let commit_requested = events
        .iter()
        .find(|event| event.event_type == "chat_commit_requested")
        .expect("chat commit requested event");
    assert_eq!(commit_requested.payload["runId"], run.id);
    assert_eq!(commit_requested.payload["workspaceId"], run.workspace_id);
    assert_eq!(commit_requested.payload["stableChatId"], run.stable_chat_id);
    assert_eq!(commit_requested.payload["path"], "output/main.md");
    assert_eq!(commit_requested.payload["mode"], "replace");
    assert!(commit_requested.payload["sha256"].as_str().is_some());
    assert!(events.iter().any(|event| {
        event.event_type == "chat_commit_completed" && event.payload["messageId"] == "message_1"
    }));
    let metadata_requested = events
        .iter()
        .find(|event| event.event_type == "persistent_state_metadata_update_requested")
        .expect("persistent metadata update requested");
    assert_eq!(metadata_requested.payload["runId"], run.id);
    assert_eq!(metadata_requested.payload["messageId"], "message_1");
    assert!(metadata_requested.payload["changeCount"].as_u64().is_some());
    assert!(metadata_requested.payload["stateId"].as_str().is_some());
    assert!(events.iter().any(|event| {
        event.event_type == "persistent_state_metadata_updated"
            && event.payload["messageId"] == "message_1"
    }));

    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn agent_runtime_retries_retryable_model_errors_with_real_repositories() {
    let root = temp_root("agent-retry");
    let fixture = agent_runtime_fixture_with_results(
        &root,
        vec![
            Err(ApplicationError::Transient(
                "temporary transport failure".to_string(),
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
        ],
    );
    let mut profile = resolve_contract_profile(&fixture).await;
    profile.run.presentation = AgentRunPresentation::Background;
    profile.run.model_retry.max_retries = 1;
    profile.run.model_retry.interval_ms = 1;
    profile.tools.max_rounds = 2;
    let run = contract_run(
        "run_retry_contract",
        AgentRunPresentation::Background,
        &profile,
    );
    fixture
        .agent_repository
        .create_run(&run)
        .await
        .expect("create run");
    let request = chat_request("write with retry");
    let prompt_snapshot = json!({ "chatCompletionPayload": request.payload.clone() });
    let (_cancel_sender, mut cancel_receiver) = watch::channel(false);

    fixture
        .service
        .execute_agent_loop_run_inner(
            &run.id,
            prompt_snapshot,
            request,
            profile,
            &mut cancel_receiver,
        )
        .await
        .expect("agent loop");

    assert_eq!(fixture.model_gateway.requests().await.len(), 3);
    let artifact = fixture
        .agent_repository
        .read_text(&run.id, &WorkspacePath::parse("output/main.md").unwrap())
        .await
        .expect("read artifact");
    assert_eq!(artifact.text, "retry succeeded");
    let events = read_agent_events(&fixture.agent_repository, &run.id).await;
    let retry_failed = events
        .iter()
        .find(|event| event.event_type == "model_call_attempt_failed")
        .expect("retryable failed attempt");
    assert_eq!(retry_failed.payload["attempt"], 1);
    assert_eq!(retry_failed.payload["maxRetries"], 1);
    assert_eq!(retry_failed.payload["retryable"], true);
    assert_eq!(retry_failed.payload["willRetry"], true);
    let retry_scheduled = events
        .iter()
        .find(|event| event.event_type == "model_call_retry_scheduled")
        .expect("retry scheduled event");
    assert_eq!(retry_scheduled.payload["nextAttempt"], 2);
    assert_eq!(retry_scheduled.payload["intervalMs"], 1);

    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn agent_runtime_does_not_retry_non_retryable_model_errors() {
    let root = temp_root("agent-no-retry");
    let fixture = agent_runtime_fixture_with_results(
        &root,
        vec![Err(ApplicationError::ValidationError(
            "model.invalid_tool_call: missing id".to_string(),
        ))],
    );
    let mut profile = resolve_contract_profile(&fixture).await;
    profile.run.model_retry.max_retries = 2;
    profile.run.model_retry.interval_ms = 1;
    let run = contract_run(
        "run_no_retry_contract",
        AgentRunPresentation::Background,
        &profile,
    );
    fixture
        .agent_repository
        .create_run(&run)
        .await
        .expect("create run");
    let request = chat_request("write without retry");
    let prompt_snapshot = json!({ "chatCompletionPayload": request.payload.clone() });
    let (_cancel_sender, mut cancel_receiver) = watch::channel(false);

    let error = fixture
        .service
        .execute_agent_loop_run_inner(
            &run.id,
            prompt_snapshot,
            request,
            profile,
            &mut cancel_receiver,
        )
        .await
        .expect_err("non-retryable error");

    assert!(error.to_string().contains("missing id"));
    assert_eq!(fixture.model_gateway.requests().await.len(), 1);
    let events = read_agent_events(&fixture.agent_repository, &run.id).await;
    assert!(
        events
            .iter()
            .all(|event| event.event_type != "model_call_retry_scheduled")
    );

    let _ = fs::remove_dir_all(root).await;
}
