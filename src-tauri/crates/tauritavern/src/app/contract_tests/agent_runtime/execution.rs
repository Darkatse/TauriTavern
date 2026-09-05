use super::*;
use tt_domain::models::upstream_failure::{UPSTREAM_NETWORK_TIMEOUT, UpstreamFailure};

#[tokio::test]
async fn agent_runtime_background_run_finish_uses_run_presentation() {
    let root = temp_root("agent-runtime");
    let fixture = agent_runtime_fixture(&root);
    let registry = BuiltinAgentToolRegistry::all();
    let mut profile = fixture
        .profile_service
        .resolve_profile(AgentProfileResolveInput {
            profile_id: None,
            tool_catalog: registry.catalog(),
        })
        .await
        .expect("resolve default profile");
    profile.run.presentation = AgentRunPresentation::Foreground;
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
    assert_eq!(
        tool_requested.payload["toolId"],
        "builtin:workspace.write_file"
    );
    assert_eq!(
        tool_requested.payload["snapshotId"],
        ROOT_AGENT_INVOCATION_ID
    );
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
    assert_eq!(
        result_stored.payload["toolId"],
        "builtin:workspace.write_file"
    );
    let result_ref = result_stored.payload["path"].as_str().expect("result ref");
    assert!(result_ref.starts_with("tool-results/call_"));
    let result = read_workspace_json(&fixture.agent_repository, &run.id, result_ref).await;
    assert_eq!(result["toolId"], "builtin:workspace.write_file");
    let tool_snapshot = read_workspace_json(
        &fixture.agent_repository,
        &run.id,
        "input/invocations/inv_root/tool_snapshot.json",
    )
    .await;
    assert_eq!(tool_snapshot["schemaVersion"], 1);
    assert_eq!(tool_snapshot["id"], ROOT_AGENT_INVOCATION_ID);
    assert!(
        tool_snapshot["bindings"]
            .as_array()
            .is_some_and(|bindings| {
                bindings.iter().any(|binding| {
                    binding["descriptor"]["id"] == "builtin:workspace.write_file"
                        && binding["modelAlias"] == "workspace_write_file"
                })
            })
    );
    let context_assembled = events
        .iter()
        .find(|event| event.event_type == "context_assembled")
        .expect("context assembled event");
    assert_eq!(
        context_assembled.payload["toolSnapshotPath"],
        "input/invocations/inv_root/tool_snapshot.json"
    );
    let model_request_created = events
        .iter()
        .find(|event| event.event_type == "model_request_created")
        .expect("model request created event");
    assert_eq!(
        model_request_created.payload["toolTurn"]["snapshotId"],
        ROOT_AGENT_INVOCATION_ID
    );
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
                .any(|tool| tool.tool_id.native_name() == "workspace.write_file"))
    );
    wait_for_closed_sessions(
        &fixture.model_gateway,
        vec!["run_contract:inv_root".to_string()],
    )
    .await;

    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn agent_runtime_streaming_keeps_the_existing_final_execution_path() {
    let root = temp_root("agent-runtime-streaming");
    let fixture = agent_runtime_fixture(&root);
    let registry = BuiltinAgentToolRegistry::all();
    let mut definition = fixture
        .profile_service
        .load_profile(DEFAULT_AGENT_PROFILE_ID)
        .await
        .expect("load default profile")
        .expect("default profile exists");
    definition.id = AgentProfileId::parse("streaming-profile").unwrap();
    definition.display_name = "Streaming Profile".to_string();
    definition.run.stream = true;
    fixture
        .profile_service
        .save_profile(definition, registry.catalog())
        .await
        .expect("save streaming profile");
    let profile = fixture
        .profile_service
        .resolve_profile(AgentProfileResolveInput {
            profile_id: Some("streaming-profile"),
            tool_catalog: registry.catalog(),
        })
        .await
        .expect("resolve streaming profile");
    let handle = start_contract_agent_run(
        &fixture,
        &profile,
        AgentRunPresentation::Background,
        "streaming-final",
        None,
    )
    .await;

    let run = wait_for_terminal_agent_run(&fixture.agent_repository, &handle.run_id).await;
    assert_eq!(run.status, AgentRunStatus::Completed);
    assert_eq!(fixture.model_gateway.stream_requests().await, [true, true]);
    if let Some(mut receiver) = fixture
        .service
        .subscribe_live_projection(&handle.run_id)
        .await
        .unwrap()
    {
        tokio::time::timeout(AGENT_CONTRACT_ASYNC_TIMEOUT, async {
            while receiver.changed().await.is_ok() {}
        })
        .await
        .expect("terminal run did not close its live projection");
    }
    assert!(
        fixture
            .service
            .subscribe_live_projection(&handle.run_id)
            .await
            .unwrap()
            .is_none()
    );
    let artifact = fixture
        .agent_repository
        .read_text(
            &handle.run_id,
            &WorkspacePath::parse("output/main.md").unwrap(),
        )
        .await
        .expect("read streamed run artifact");
    assert_eq!(artifact.text, "hello from real repo");

    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn agent_runtime_returns_missing_chat_reads_to_the_agent() {
    let root = temp_root("agent-missing-chat-recovery");
    let fixture = agent_runtime_fixture_with_responses(
        &root,
        vec![
            model_tool_response(vec![
                model_tool_call(
                    "call_search_missing_chat",
                    "chat_search",
                    json!({ "query": "context" }),
                ),
                model_tool_call(
                    "call_read_missing_chat",
                    "chat_read_messages",
                    json!({ "messages": [{ "index": 0 }] }),
                ),
            ]),
            model_tool_response(vec![
                model_tool_call(
                    "call_write_after_missing_chat",
                    "workspace_write_file",
                    json!({ "path": "output/main.md", "content": "continued safely" }),
                ),
                model_tool_call(
                    "call_finish_after_missing_chat",
                    "workspace_finish",
                    json!({}),
                ),
            ]),
        ],
    );
    let mut profile = resolve_contract_profile(&fixture).await;
    profile.tools.max_rounds = 2;
    let mut run = contract_run(
        "run_missing_chat_recovery",
        AgentRunPresentation::Background,
        &profile,
    );
    run.input_message_count = Some(1);
    fixture
        .agent_repository
        .create_run(&run)
        .await
        .expect("create run");
    let request = chat_request("continue without the missing chat");
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
        .expect("recoverable chat errors must not stop the run");

    let requests = fixture.model_gateway.requests().await;
    let results = requests[1]
        .messages
        .iter()
        .flat_map(|message| message.parts.iter())
        .filter_map(|part| match part {
            AgentModelContentPart::ToolResult { result } => Some(result),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(results.len(), 2);
    assert!(results.iter().all(|result| result.is_error));
    assert!(
        results
            .iter()
            .all(|result| result.error_code.as_deref() == Some("chat.not_found"))
    );
    assert!(
        results
            .iter()
            .all(|result| result.content.contains("Continue with the context"))
    );

    let saved = fixture
        .agent_repository
        .load_run(&run.id)
        .await
        .expect("load completed run");
    assert_eq!(saved.status, AgentRunStatus::Completed);

    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn agent_runtime_duplicate_tool_call_id_preserves_first_audit_facts() {
    let root = temp_root("agent-duplicate-tool-call-id");
    let fixture = agent_runtime_fixture_with_responses(
        &root,
        vec![model_tool_response(vec![
            model_tool_call(
                "duplicate_call",
                "workspace_write_file",
                json!({ "path": "output/first.md", "content": "first" }),
            ),
            model_tool_call(
                "duplicate_call",
                "workspace_write_file",
                json!({ "path": "output/second.md", "content": "second" }),
            ),
        ])],
    );
    let mut profile = resolve_contract_profile(&fixture).await;
    profile.tools.max_rounds = 1;
    let handle = start_contract_agent_run(
        &fixture,
        &profile,
        AgentRunPresentation::Background,
        "duplicate-tool-call-id",
        Some(false),
    )
    .await;

    let run = wait_for_terminal_agent_run(&fixture.agent_repository, &handle.run_id).await;
    assert_eq!(run.status, AgentRunStatus::Failed);
    let events = read_agent_events(&fixture.agent_repository, &handle.run_id).await;
    let requested = events
        .iter()
        .filter(|event| {
            event.event_type == "tool_call_requested" && event.payload["callId"] == "duplicate_call"
        })
        .collect::<Vec<_>>();
    assert_eq!(requested.len(), 1);
    let arguments_ref = requested[0].payload["argumentsRef"]
        .as_str()
        .expect("arguments ref");
    let arguments =
        read_workspace_json(&fixture.agent_repository, &handle.run_id, arguments_ref).await;
    assert_eq!(arguments["path"], "output/first.md");

    let stored_results = events
        .iter()
        .filter(|event| {
            event.event_type == "tool_result_stored" && event.payload["callId"] == "duplicate_call"
        })
        .collect::<Vec<_>>();
    assert_eq!(stored_results.len(), 1);
    let result_ref = stored_results[0].payload["path"]
        .as_str()
        .expect("result ref");
    let result = read_workspace_json(&fixture.agent_repository, &handle.run_id, result_ref).await;
    assert_eq!(result["structured"]["path"], "output/first.md");

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
            "builtin:agent.list" | "builtin:agent.delegate" | "builtin:agent.await"
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
        .save_profile(callable, fixture.service.tool_catalog())
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
async fn agent_runtime_foreground_auto_commits_once_per_round_until_explicit_commit() {
    let root = temp_root("agent-foreground");
    let fixture = agent_runtime_fixture_with_responses(
        &root,
        vec![
            model_tool_response(vec![
                model_tool_call(
                    "call_write",
                    "workspace_write_file",
                    json!({ "path": "output/main.md", "content": "foreground answer" }),
                ),
                model_tool_call(
                    "call_patch",
                    "workspace_apply_patch",
                    json!({
                        "path": "output/main.md",
                        "old_string": "foreground answer",
                        "new_string": "revised foreground answer",
                    }),
                ),
                model_tool_call("call_commit", "workspace_commit", json!({})),
                model_tool_call("call_finish_after_auto", "workspace_finish", json!({})),
            ]),
            model_tool_response(vec![
                model_tool_call("call_commit_retry", "workspace_commit", json!({})),
                model_tool_call(
                    "call_write_after_explicit",
                    "workspace_write_file",
                    json!({ "path": "scratch/after.txt", "content": "must not auto commit" }),
                ),
                model_tool_call("call_finish", "workspace_finish", json!({})),
            ]),
        ],
    );
    let mut profile = resolve_contract_profile(&fixture).await;
    profile.run.presentation = AgentRunPresentation::Background;
    profile.tools.max_rounds = 2;
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
    let request = chat_request("write, revise, commit, and finish");
    let prompt_snapshot = json!({ "chatCompletionPayload": request.payload.clone() });
    let (_cancel_sender, mut cancel_receiver) = watch::channel(false);
    let rejected_commit_calls = ["call_commit"];

    execute_agent_loop_with_host_resolver(
        fixture.service.clone(),
        run.id.clone(),
        prompt_snapshot,
        request,
        profile,
        &mut cancel_receiver,
        resolve_chat_commits_and_persistent_state_update(
            fixture.service.clone(),
            fixture.agent_repository.clone(),
            run.id.clone(),
            "message_1",
            &rejected_commit_calls,
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
    let post_auto_guard_failure = events
        .iter()
        .find(|event| {
            event.event_type == "tool_call_failed"
                && event.payload["callId"] == "call_finish_after_auto"
        })
        .expect("automatic commit must not satisfy foreground finish guard");
    assert_eq!(post_auto_guard_failure.level, AgentRunEventLevel::Warn);
    assert_eq!(
        post_auto_guard_failure.payload["errorCode"],
        "agent.foreground_commit_required"
    );
    let commit_requests = events
        .iter()
        .filter(|event| event.event_type == "chat_commit_requested")
        .collect::<Vec<_>>();
    assert_eq!(commit_requests.len(), 3);
    let automatic_commit_request = commit_requests
        .iter()
        .find(|event| event.payload["callId"] == "call_patch")
        .expect("the round must auto-commit only its final text mutation");
    assert_eq!(automatic_commit_request.payload["isExplicit"], false);
    assert_eq!(automatic_commit_request.payload["path"], "output/main.md");
    assert_eq!(automatic_commit_request.payload["mode"], "replace");
    assert!(
        automatic_commit_request.payload["sha256"]
            .as_str()
            .is_some()
    );
    assert!(
        commit_requests
            .iter()
            .all(|event| event.payload["callId"] != "call_write")
    );
    let final_commit_request = commit_requests.last().unwrap();
    assert_eq!(final_commit_request.payload["isExplicit"], true);
    assert_eq!(final_commit_request.payload["runId"], run.id);
    assert_eq!(
        final_commit_request.payload["workspaceId"],
        run.workspace_id
    );
    assert_eq!(
        final_commit_request.payload["stableChatId"],
        run.stable_chat_id
    );
    assert_eq!(final_commit_request.payload["path"], "output/main.md");
    let commit_failures = events
        .iter()
        .filter(|event| event.event_type == "chat_commit_failed")
        .collect::<Vec<_>>();
    assert_eq!(commit_failures.len(), 1);
    assert!(
        commit_failures
            .iter()
            .all(|event| event.level == AgentRunEventLevel::Warn)
    );
    let explicit_commit_failure = events
        .iter()
        .find(|event| {
            event.event_type == "tool_call_failed" && event.payload["callId"] == "call_commit"
        })
        .expect("recoverable explicit commit tool result");
    assert_eq!(
        explicit_commit_failure.payload["errorCode"],
        "agent.chat_commit_rejected"
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| {
                event.event_type == "chat_commit_completed"
                    && event.payload["messageId"] == "message_1"
            })
            .count(),
        2
    );
    let commit_recorded = events
        .iter()
        .filter(|event| event.event_type == "chat_commit_recorded")
        .collect::<Vec<_>>();
    assert_eq!(commit_recorded.len(), 2);
    assert_eq!(commit_recorded[0].payload["commitCount"], 1);
    assert_eq!(commit_recorded[1].payload["commitCount"], 2);
    let loop_finished = events
        .iter()
        .find(|event| event.event_type == "agent_loop_finished")
        .expect("agent loop finished event");
    assert_eq!(loop_finished.payload["commitCount"], 2);
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
async fn agent_runtime_streamed_writes_defer_to_patch_and_explicit_commits() {
    let root = temp_root("agent-streamed-write-commit");
    let fixture = agent_runtime_fixture_with_responses(
        &root,
        vec![
            model_tool_response(vec![
                model_tool_call(
                    "call_write_first",
                    "workspace_write_file",
                    json!({ "path": "output/main.md", "content": "draft" }),
                ),
                model_tool_call(
                    "call_patch_first",
                    "workspace_apply_patch",
                    json!({
                        "path": "output/main.md",
                        "old_string": "draft",
                        "new_string": "patched",
                    }),
                ),
            ]),
            model_tool_response(vec![
                model_tool_call(
                    "call_patch_second",
                    "workspace_apply_patch",
                    json!({
                        "path": "output/main.md",
                        "old_string": "patched",
                        "new_string": "second patch",
                    }),
                ),
                model_tool_call(
                    "call_write_second",
                    "workspace_write_file",
                    json!({
                        "path": "output/main.md",
                        "content": "streamed replacement",
                    }),
                ),
            ]),
            model_tool_response(vec![
                model_tool_call("call_commit", "workspace_commit", json!({})),
                model_tool_call("call_finish", "workspace_finish", json!({})),
            ]),
        ],
    );
    let mut profile = resolve_contract_profile(&fixture).await;
    profile.tools.max_rounds = 3;
    let handle = start_contract_agent_run(
        &fixture,
        &profile,
        AgentRunPresentation::Foreground,
        "streamed-write-commit",
        Some(true),
    )
    .await;

    let (run, resolver) = tokio::join!(
        wait_for_terminal_agent_run(&fixture.agent_repository, &handle.run_id),
        resolve_chat_commits_and_persistent_state_update(
            fixture.service.clone(),
            fixture.agent_repository.clone(),
            handle.run_id.clone(),
            "message_streamed",
            &[],
        ),
    );
    resolver.expect("host resolver");
    assert_eq!(run.status, AgentRunStatus::Completed);

    let events = read_agent_events(&fixture.agent_repository, &handle.run_id).await;
    let commits = events
        .iter()
        .filter(|event| event.event_type == "chat_commit_requested")
        .collect::<Vec<_>>();
    assert_eq!(commits.len(), 2);
    assert_eq!(commits[0].payload["callId"], "call_patch_first");
    assert_eq!(commits[0].payload["isExplicit"], false);
    assert_eq!(commits[1].payload["callId"], "call_commit");
    assert_eq!(commits[1].payload["isExplicit"], true);
    assert!(commits.iter().all(|event| {
        event.payload["callId"] != "call_write_first"
            && event.payload["callId"] != "call_write_second"
            && event.payload["callId"] != "call_patch_second"
    }));

    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn agent_runtime_retries_retryable_model_errors_with_real_repositories() {
    let root = temp_root("agent-retry");
    let fixture = agent_runtime_fixture_with_results(
        &root,
        vec![
            Err(ApplicationError::UpstreamFailure(UpstreamFailure::network(
                UPSTREAM_NETWORK_TIMEOUT,
                None,
                "tauritavern.error.network.timeout",
            ))),
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

#[tokio::test]
async fn agent_runtime_replays_frozen_macros_before_reading_and_searching() {
    use tt_domain::models::chat::ChatMessage;
    use tt_domain::models::skill::{
        SkillImportInput, SkillInlineFile, SkillInstallRequest, SkillScope,
    };
    use tt_ports::repositories::skill_repository::SkillRepository;

    let root = temp_root("frozen-macros");
    let source = "heading\n{{description}}\n{{char}}\n{{greeting::1}} / {{getvar::Name}} / {{getglobalvar::Name}} / {{outlet::Lore}} / {{date}} / {{instructUserPrefix}}";
    let fixture = agent_runtime_fixture_with_responses(
        &root,
        vec![
            model_tool_response(vec![
                model_tool_call(
                    "write",
                    "workspace_write_file",
                    json!({ "path": "output/main.md", "content": source }),
                ),
                model_tool_call(
                    "skill_read",
                    "skill_read",
                    json!({ "name": "macro-demo", "path": "references/template.md", "start_line": 3, "line_count": 1 }),
                ),
                model_tool_call(
                    "skill_search",
                    "skill_search",
                    json!({ "name": "macro-demo", "query": "lantern" }),
                ),
                model_tool_call(
                    "chat_read",
                    "chat_read_messages",
                    json!({ "messages": [{ "index": 0, "start_line": 3, "line_count": 1 }] }),
                ),
                model_tool_call("chat_search", "chat_search", json!({ "query": "lantern" })),
                model_tool_call(
                    "file_read",
                    "workspace_read_file",
                    json!({ "path": "output/main.md" }),
                ),
                model_tool_call(
                    "file_search",
                    "workspace_search_files",
                    json!({ "path": "output", "query": "lantern" }),
                ),
                model_tool_call(
                    "script",
                    "skill_run_script",
                    json!({ "skill": "macro-demo", "script": "helper" }),
                ),
            ]),
            model_tool_response(vec![model_tool_call(
                "finish",
                "workspace_finish",
                json!({}),
            )]),
        ],
    );
    FileSkillRepository::new(root.join("_tauritavern/skills"))
        .install_import(SkillInstallRequest {
            target_scope: SkillScope::Global,
            conflict_strategy: None,
            input: SkillImportInput::InlineFiles {
                source: json!({ "kind": "test" }),
                files: [
                    ("SKILL.md", "---\nname: macro-demo\ndescription: Macro test\n---\n{{char}}"),
                    ("references/template.md", source),
                    ("scripts/helper.js", "import { macros, workspace } from '@tauritavern/runtime'; export default () => macros.render(workspace.readText('output/main.md'));"),
                ].into_iter().map(|(path, content)| SkillInlineFile {
                    path: path.into(), content: content.into(), encoding: "utf8".into(),
                    media_type: None, size_bytes: None, sha256: None,
                }).collect(),
            },
        }).await.unwrap();
    let mut profile = resolve_contract_profile(&fixture).await;
    profile.tools.max_rounds = 2;
    profile.tools.max_calls_per_run = 20;
    let mut run = contract_run("run_macros", AgentRunPresentation::Background, &profile);
    run.chat_ref = AgentChatRef::Character {
        character_id: "Alice".into(),
        file_name: "macros.jsonl".into(),
    };
    run.input_message_count = Some(1);
    let mut chat = Chat::new("User", "Alice");
    chat.file_name = Some("macros.jsonl".into());
    chat.add_message(ChatMessage::user("User", source));
    fixture.chat_repository.save(&chat).await.unwrap();
    fixture.agent_repository.create_run(&run).await.unwrap();
    let request = chat_request("read the frozen text");
    let prompt = json!({
        "chatCompletionPayload": request.payload,
        "worldInfoActivation": { "entries": [] },
        "frozenRunInputSnapshot": {
            "promptInputs": {
                "extensionPrompts": { "customWIOutlet_Lore": { "value": "Outlet" } }
            },
            "macroContext": {
                "names": { "char": "Frozen name" },
                "character": {
                    "description": "first line\nblue lantern\nlast line",
                    "firstMessage": "Main",
                    "alternateGreetings": ["Alternate"]
                },
                "variableValues": { "local": { "Name": "Local" }, "global": { "Name": "Global" } },
                "builtins": { "date": "2026-09-05", "instructInput": "USER:" }
            }
        }
    });
    let (_cancel, mut receiver) = watch::channel(false);
    fixture
        .service
        .execute_agent_loop_run_inner(&run.id, prompt, request, profile, &mut receiver)
        .await
        .unwrap();
    let requests = fixture.model_gateway.requests().await;
    let results = requests[1]
        .messages
        .iter()
        .flat_map(|message| &message.parts)
        .filter_map(|part| match part {
            AgentModelContentPart::ToolResult { result } => Some((result.call_id.as_str(), result)),
            _ => None,
        })
        .collect::<std::collections::HashMap<_, _>>();
    for result in results.values() {
        assert!(!result.is_error, "{}: {}", result.call_id, result.content);
    }
    assert!(results["skill_read"].content.contains("3 | blue lantern"));
    assert_eq!(results["skill_read"].structured["totalLines"], 6);
    assert_eq!(
        results["chat_read"].structured["messages"][0]["text"],
        "blue lantern"
    );
    for id in ["skill_search", "chat_search"] {
        assert_eq!(
            results[id].structured["hits"].as_array().unwrap().len(),
            1,
            "{id}"
        );
    }
    assert!(
        results["file_search"].structured["hits"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(results["file_read"].content.contains("{{description}}"));
    assert_eq!(results["file_read"].structured["totalLines"], 4);
    assert!(results["script"].content.contains("blue lantern"));
    assert!(results["script"].content.contains("Frozen name"));
    assert!(
        results["script"]
            .content
            .contains("Alternate / Local / Global / Outlet / 2026-09-05 / USER:")
    );
    assert_eq!(
        fixture
            .agent_repository
            .read_text(&run.id, &WorkspacePath::parse("output/main.md").unwrap())
            .await
            .unwrap()
            .text,
        source
    );
    let _ = fs::remove_dir_all(root).await;
}
