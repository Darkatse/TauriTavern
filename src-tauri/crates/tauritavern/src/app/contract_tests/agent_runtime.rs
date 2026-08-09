use super::*;
use tt_domain::models::agent::{
    AgentDelegationContinuation, AgentInvocationExitPolicy, AgentInvocationKind,
    AgentInvocationStatus, AgentModelRole, AgentTaskStatus, ROOT_AGENT_INVOCATION_ID,
};

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
async fn agent_runtime_executes_cached_mcp_tool_through_readable_alias() {
    let root = temp_root("agent-mcp-vertical-slice");
    let fixture = agent_runtime_fixture_with_responses(
        &root,
        vec![
            model_tool_response(vec![model_tool_call(
                "call_mcp",
                "mcp__my_server__issue_create",
                json!({ "title": "Contract issue" }),
            )]),
            model_tool_response(vec![model_tool_call(
                "call_read",
                "workspace_read_file",
                json!({
                    "path": "tool-results/call_fe79da5f09df9787.json",
                    "start_char": 0,
                    "max_chars": 2_000
                }),
            )]),
            model_tool_response(vec![
                model_tool_call(
                    "call_write",
                    "workspace_write_file",
                    json!({ "path": "output/main.md", "content": "MCP complete" }),
                ),
                model_tool_call("call_finish", "workspace_finish", json!({})),
            ]),
        ],
    );
    let (profile, mcp_tool_id) = configure_mcp_profile(&fixture, "mcp-writer", 3, 10_000).await;

    let handle = start_contract_agent_run(
        &fixture,
        &profile,
        AgentRunPresentation::Background,
        "mcp-vertical-slice",
    )
    .await;
    let run = wait_for_terminal_agent_run(&fixture.agent_repository, &handle.run_id).await;
    assert_eq!(run.status, AgentRunStatus::Completed);

    let calls = fixture.mcp_gateway.calls.lock().await;
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "issue.create");
    assert_eq!(calls[0].1["title"], "Contract issue");
    drop(calls);

    let requests = fixture.model_gateway.requests().await;
    let advertised = requests[0]
        .tools
        .iter()
        .find(|tool| tool.tool_id.as_str() == mcp_tool_id)
        .expect("MCP tool advertised");
    assert_eq!(advertised.model_alias, "mcp__my_server__issue_create");
    let mcp_result = requests[1]
        .messages
        .iter()
        .flat_map(|message| message.parts.iter())
        .find_map(|part| match part {
            AgentModelContentPart::ToolResult { result }
                if result.tool_id.as_str() == mcp_tool_id =>
            {
                Some(result)
            }
            _ => None,
        })
        .expect("MCP result returned to model");
    assert!(mcp_result.content.contains("too large to place in context"));
    assert!(mcp_result.content.contains("workspace_read_file"));
    assert!(mcp_result.content.contains("<mcp-result-preview>"));
    assert_eq!(mcp_result.structured["externalized"], true);
    assert_eq!(mcp_result.structured["charLimit"], 10_000);
    let result_path = mcp_result.resource_refs[0].as_str();
    assert_eq!(result_path, "tool-results/call_fe79da5f09df9787.json");
    let stored = read_workspace_json(&fixture.agent_repository, &handle.run_id, result_path).await;
    assert_eq!(stored["content"].as_str().unwrap().len(), 60_000);
    assert_eq!(stored["structured"]["structuredContent"]["issueId"], 42);
    let read_result = requests[2]
        .messages
        .iter()
        .flat_map(|message| message.parts.iter())
        .find_map(|part| match part {
            AgentModelContentPart::ToolResult { result }
                if result.tool_id.as_str() == "builtin:workspace.read_file" =>
            {
                Some(result)
            }
            _ => None,
        })
        .expect("Agent can read the externalized MCP result through workspace VFS");
    assert!(read_result.content.contains("call_mcp"));

    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn agent_runtime_stops_after_unknown_mcp_call_outcome() {
    let root = temp_root("agent-mcp-outcome-unknown");
    let fixture = agent_runtime_fixture_with_responses(
        &root,
        vec![model_tool_response(vec![
            model_tool_call(
                "call_unknown",
                "mcp__my_server__issue_create",
                json!({ "title": "Maybe created" }),
            ),
            model_tool_call(
                "call_after_unknown",
                "workspace_write_file",
                json!({ "path": "output/must-not-exist.md", "content": "must not run" }),
            ),
        ])],
    );
    fixture
        .mcp_gateway
        .outcomes
        .lock()
        .await
        .push_back(McpCallOutcome::OutcomeUnknown(McpCallIssue {
            code: "mcp.response_too_large".to_string(),
            message: "response exceeded the wire limit".to_string(),
        }));
    let (profile, _) = configure_mcp_profile(&fixture, "mcp-unknown-writer", 1, 50_000).await;

    let handle = start_contract_agent_run(
        &fixture,
        &profile,
        AgentRunPresentation::Background,
        "mcp-outcome-unknown",
    )
    .await;
    let run = wait_for_terminal_agent_run(&fixture.agent_repository, &handle.run_id).await;
    assert_eq!(run.status, AgentRunStatus::Failed);

    wait_for_event_type(&fixture.agent_repository, &handle.run_id, "run_failed").await;
    let events = read_agent_events(&fixture.agent_repository, &handle.run_id).await;
    assert!(events.iter().any(|event| {
        event.event_type == "run_failed" && event.payload["code"] == "mcp.call_outcome_unknown"
    }));
    assert!(events.iter().any(|event| {
        event.event_type == "tool_call_failed"
            && event.payload["callId"] == "call_unknown"
            && event.payload["errorCode"] == "mcp.call_outcome_unknown"
    }));
    assert!(
        events
            .iter()
            .all(|event| { event.payload["callId"] != "call_after_unknown" })
    );
    assert_eq!(fixture.model_gateway.requests().await.len(), 1);

    let _ = fs::remove_dir_all(root).await;
}

async fn configure_mcp_profile(
    fixture: &AgentRuntimeFixture,
    profile_id: &str,
    max_rounds: usize,
    mcp_result_inline_char_limit: usize,
) -> (
    tt_domain::models::agent::profile::ResolvedAgentProfile,
    String,
) {
    let server = fixture
        .mcp_service
        .create_server(
            "my server".to_string(),
            "http://127.0.0.1:3333/mcp".to_string(),
        )
        .await
        .unwrap();
    fixture
        .mcp_service
        .set_server_state(&server.id, tt_domain::models::mcp::McpServerState::Active)
        .await
        .unwrap();
    fixture
        .mcp_service
        .discover_tools(&server.id)
        .await
        .unwrap();
    fixture
        .mcp_service
        .set_tool_permission(
            &server.id,
            "issue.create".to_string(),
            McpToolPermission::Ask,
        )
        .await
        .unwrap();

    let mut definition = fixture
        .profile_service
        .load_profile("default-writer")
        .await
        .unwrap()
        .unwrap();
    definition.id = AgentProfileId::parse(profile_id).unwrap();
    let mcp_tool_id = format!("mcp/{}:issue.create", server.id);
    definition.tools.allow.push(mcp_tool_id.clone());
    definition.tools.max_rounds = max_rounds;
    definition.tools.mcp_result_inline_char_limit = mcp_result_inline_char_limit;
    fixture
        .profile_service
        .save_profile(definition, fixture.service.tool_catalog())
        .await
        .unwrap();
    let profile = fixture
        .profile_service
        .resolve_profile(AgentProfileResolveInput {
            profile_id: Some(profile_id),
            tool_catalog: fixture.service.tool_catalog(),
        })
        .await
        .unwrap();
    (profile, mcp_tool_id)
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
async fn agent_runtime_foreground_commit_guard_uses_run_presentation() {
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
                                "id": "call_finish_still_too_early",
                                "type": "function",
                                "function": {
                                    "name": "workspace_finish",
                                    "arguments": "{}"
                                }
                            },
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
    profile.run.presentation = AgentRunPresentation::Background;
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
    let second_guard_failure = events
        .iter()
        .find(|event| {
            event.event_type == "tool_call_failed"
                && event.payload["callId"] == "call_finish_still_too_early"
        })
        .expect("second foreground finish guard failure");
    assert_eq!(
        second_guard_failure.payload["errorCode"],
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
    let commit_recorded = events
        .iter()
        .find(|event| event.event_type == "chat_commit_recorded")
        .expect("chat commit recorded event");
    assert_eq!(commit_recorded.payload["commitCount"], 1);
    let loop_finished = events
        .iter()
        .find(|event| event.event_type == "agent_loop_finished")
        .expect("agent loop finished event");
    assert_eq!(loop_finished.payload["commitCount"], 1);
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

#[tokio::test]
async fn agent_runtime_delegate_await_runs_return_mode_child() {
    let root = temp_root("agent-return-child");
    let fixture = agent_runtime_fixture_with_responses(
        &root,
        vec![
            model_tool_response(vec![
                model_tool_call(
                    "call_delegate",
                    "agent_delegate",
                    json!({
                        "agentId": "scene-critic",
                        "task": { "objective": "Return one concrete revision note." }
                    }),
                ),
                model_tool_call(
                    "call_await",
                    "agent_await",
                    json!({ "mode": "nextCompleted", "timeoutMs": 5_000 }),
                ),
            ]),
            model_tool_response(vec![
                model_tool_call(
                    "call_child_write",
                    "workspace_write_file",
                    json!({ "path": "summaries/note.md", "content": "Add rain." }),
                ),
                model_tool_call(
                    "call_child_return",
                    "task_return",
                    json!({ "summary": "Add a concrete sound.", "status": "completed" }),
                ),
            ]),
            model_tool_response(vec![model_tool_call(
                "call_parent_finish",
                "workspace_finish",
                json!({}),
            )]),
        ],
    );
    let profile = configure_return_mode_profiles(&fixture).await;
    let handle = start_contract_agent_run(
        &fixture,
        &profile,
        AgentRunPresentation::Background,
        "delegate-return-child",
    )
    .await;

    let run = wait_for_terminal_agent_run(&fixture.agent_repository, &handle.run_id).await;
    assert_eq!(run.status, AgentRunStatus::Completed);
    let tasks = fixture
        .agent_repository
        .list_tasks(&handle.run_id)
        .await
        .expect("list delegated tasks");
    assert_eq!(tasks.len(), 1);
    let task = &tasks[0];
    assert_eq!(
        task.continuation,
        AgentDelegationContinuation::ReturnToParent
    );
    assert_eq!(task.status, AgentTaskStatus::Completed);
    let child = fixture
        .agent_repository
        .load_invocation(&handle.run_id, &task.child_invocation_id)
        .await
        .expect("load child invocation");
    assert_eq!(child.kind, AgentInvocationKind::Subagent);
    assert_eq!(
        child.exit_policy,
        AgentInvocationExitPolicy::TaskReturnRequired
    );
    assert_eq!(child.status, AgentInvocationStatus::Completed);

    let requests = fixture.model_gateway.requests().await;
    assert_eq!(requests.len(), 3);
    assert_eq!(
        requests[0].provider_state["invocationId"],
        ROOT_AGENT_INVOCATION_ID
    );
    assert_eq!(
        requests[1].provider_state["invocationId"],
        task.child_invocation_id
    );
    assert_eq!(
        requests[2].provider_state["invocationId"],
        ROOT_AGENT_INVOCATION_ID
    );
    assert!(
        requests[1]
            .tools
            .iter()
            .any(|tool| tool.tool_id.native_name() == "task.return")
    );
    assert!(requests[1].tools.iter().all(|tool| {
        !matches!(
            tool.tool_id.native_name(),
            "workspace.commit"
                | "workspace.finish"
                | "agent.list"
                | "agent.delegate"
                | "agent.handoff"
                | "agent.await"
        )
    }));
    let child_snapshot = read_workspace_json(
        &fixture.agent_repository,
        &handle.run_id,
        &format!(
            "input/invocations/{}/tool_snapshot.json",
            task.child_invocation_id
        ),
    )
    .await;
    let child_tool_ids = child_snapshot["bindings"]
        .as_array()
        .expect("child snapshot bindings")
        .iter()
        .map(|binding| binding["descriptor"]["id"].as_str().expect("tool id"))
        .collect::<Vec<_>>();
    assert_eq!(child_tool_ids.last(), Some(&"builtin:task.return"));
    assert!(child_tool_ids.iter().all(|tool_id| {
        !matches!(
            *tool_id,
            "builtin:workspace.commit"
                | "builtin:workspace.finish"
                | "builtin:agent.list"
                | "builtin:agent.delegate"
                | "builtin:agent.handoff"
                | "builtin:agent.await"
        )
    }));
    assert!(message_text_for_role(&requests[1], AgentModelRole::User).contains("# Delegated Task"));
    wait_for_closed_sessions(
        &fixture.model_gateway,
        vec![
            format!("{}:{ROOT_AGENT_INVOCATION_ID}", handle.run_id),
            format!("{}:{}", handle.run_id, task.child_invocation_id),
        ],
    )
    .await;

    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn agent_runtime_handoff_preserves_prior_commit_and_switches_invocation() {
    let root = temp_root("agent-handoff-success");
    let fixture = agent_runtime_fixture_with_responses(
        &root,
        vec![
            model_tool_response(vec![
                model_tool_call(
                    "call_write",
                    "workspace_write_file",
                    json!({ "path": "output/main.md", "content": "Committed draft." }),
                ),
                model_tool_call("call_commit", "workspace_commit", json!({})),
                model_tool_call(
                    "call_handoff",
                    "agent_handoff",
                    json!({
                        "agentId": "final-editor",
                        "handoff": { "objective": "Review the committed draft and finish." }
                    }),
                ),
            ]),
            model_tool_response(vec![model_tool_call(
                "call_target_finish",
                "workspace_finish",
                json!({}),
            )]),
        ],
    );
    let profile = configure_handoff_profiles(&fixture).await;
    let handle = start_contract_agent_run(
        &fixture,
        &profile,
        AgentRunPresentation::Foreground,
        "handoff-after-commit",
    )
    .await;
    resolve_next_chat_commit_and_persistent_state_update(
        fixture.service.clone(),
        fixture.agent_repository.clone(),
        handle.run_id.clone(),
        "message_handoff",
    )
    .await
    .expect("resolve host commit");

    let run = wait_for_terminal_agent_run(&fixture.agent_repository, &handle.run_id).await;
    assert_eq!(run.status, AgentRunStatus::Completed);
    let tasks = fixture
        .agent_repository
        .list_tasks(&handle.run_id)
        .await
        .expect("list handoff tasks");
    assert_eq!(tasks.len(), 1);
    let task = &tasks[0];
    assert_eq!(
        task.continuation,
        AgentDelegationContinuation::TransferControl
    );
    assert_eq!(task.status, AgentTaskStatus::Completed);
    let root_invocation = fixture
        .agent_repository
        .load_invocation(&handle.run_id, ROOT_AGENT_INVOCATION_ID)
        .await
        .expect("load root invocation");
    assert_eq!(root_invocation.status, AgentInvocationStatus::Transferred);
    let target = fixture
        .agent_repository
        .load_invocation(&handle.run_id, &task.child_invocation_id)
        .await
        .expect("load handoff invocation");
    assert_eq!(target.kind, AgentInvocationKind::Handoff);
    assert_eq!(
        target.exit_policy,
        AgentInvocationExitPolicy::RunFinishAllowed
    );
    assert_eq!(target.status, AgentInvocationStatus::Completed);

    wait_for_event_type(&fixture.agent_repository, &handle.run_id, "run_completed").await;
    let events = read_agent_events(&fixture.agent_repository, &handle.run_id).await;
    let commit = events
        .iter()
        .find(|event| event.event_type == "chat_commit_recorded")
        .expect("chat commit recorded");
    assert_eq!(commit.payload["commitCount"], 1);
    let task_completed = events
        .iter()
        .position(|event| {
            event.event_type == "agent_task_completed" && event.payload["taskId"] == task.id
        })
        .expect("handoff task completed event");
    let invocation_completed = events
        .iter()
        .position(|event| {
            event.event_type == "agent_invocation_completed"
                && event.payload["invocationId"] == task.child_invocation_id
        })
        .expect("handoff invocation completed event");
    let run_completed = events
        .iter()
        .position(|event| event.event_type == "run_completed")
        .expect("run completed event");
    assert!(task_completed < invocation_completed && invocation_completed < run_completed);
    let requests = fixture.model_gateway.requests().await;
    assert_eq!(requests.len(), 2);
    assert_eq!(
        requests[1].provider_state["invocationId"],
        task.child_invocation_id
    );
    assert!(
        requests[1]
            .tools
            .iter()
            .any(|tool| tool.tool_id.native_name() == "workspace.finish")
    );
    assert!(
        requests[1]
            .tools
            .iter()
            .all(|tool| tool.tool_id.native_name() != "agent.handoff")
    );
    let handoff_snapshot = read_workspace_json(
        &fixture.agent_repository,
        &handle.run_id,
        &format!(
            "input/invocations/{}/tool_snapshot.json",
            task.child_invocation_id
        ),
    )
    .await;
    assert!(
        handoff_snapshot["bindings"]
            .as_array()
            .expect("handoff snapshot bindings")
            .iter()
            .any(|binding| binding["descriptor"]["id"] == "builtin:workspace.finish")
    );
    assert!(message_text_for_role(&requests[1], AgentModelRole::User).contains("# Handoff Brief"));
    wait_for_closed_sessions(
        &fixture.model_gateway,
        vec![
            format!("{}:{ROOT_AGENT_INVOCATION_ID}", handle.run_id),
            format!("{}:{}", handle.run_id, task.child_invocation_id),
        ],
    )
    .await;

    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn agent_runtime_handoff_target_failure_keeps_root_transferred() {
    let root = temp_root("agent-handoff-failure");
    let fixture = agent_runtime_fixture_with_results(
        &root,
        vec![
            Ok(model_tool_response(vec![model_tool_call(
                "call_handoff",
                "agent_handoff",
                json!({
                    "agentId": "final-editor",
                    "handoff": { "objective": "Take over and finish." }
                }),
            )])),
            Err(ApplicationError::ValidationError(
                "model.target_failed: invalid target response".to_string(),
            )),
        ],
    );
    let profile = configure_handoff_profiles(&fixture).await;
    let handle = start_contract_agent_run(
        &fixture,
        &profile,
        AgentRunPresentation::Background,
        "handoff-target-failure",
    )
    .await;

    let run = wait_for_terminal_agent_run(&fixture.agent_repository, &handle.run_id).await;
    assert_eq!(run.status, AgentRunStatus::Failed);
    let tasks = fixture
        .agent_repository
        .list_tasks(&handle.run_id)
        .await
        .expect("list failed handoff task");
    assert_eq!(tasks.len(), 1);
    let task = &tasks[0];
    assert_eq!(task.status, AgentTaskStatus::Failed);
    let root_invocation = fixture
        .agent_repository
        .load_invocation(&handle.run_id, ROOT_AGENT_INVOCATION_ID)
        .await
        .expect("load root invocation");
    assert_eq!(root_invocation.status, AgentInvocationStatus::Transferred);
    let target = fixture
        .agent_repository
        .load_invocation(&handle.run_id, &task.child_invocation_id)
        .await
        .expect("load failed target invocation");
    assert_eq!(target.status, AgentInvocationStatus::Failed);

    let events = read_agent_events(&fixture.agent_repository, &handle.run_id).await;
    assert!(events.iter().any(|event| {
        event.event_type == "agent_invocation_transferred"
            && event.payload["invocationId"] == ROOT_AGENT_INVOCATION_ID
    }));
    assert!(events.iter().all(|event| {
        !(matches!(
            event.event_type.as_str(),
            "agent_invocation_failed" | "agent_invocation_cancelled"
        ) && event.payload["invocationId"] == ROOT_AGENT_INVOCATION_ID)
    }));
    wait_for_closed_sessions(
        &fixture.model_gateway,
        vec![
            format!("{}:{ROOT_AGENT_INVOCATION_ID}", handle.run_id),
            format!("{}:{}", handle.run_id, task.child_invocation_id),
        ],
    )
    .await;

    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn agent_runtime_rejects_handoff_before_trailing_tool_without_successor() {
    let root = temp_root("agent-handoff-trailing-tool");
    let fixture = agent_runtime_fixture_with_responses(
        &root,
        vec![model_tool_response(vec![
            model_tool_call(
                "call_handoff",
                "agent_handoff",
                json!({
                    "agentId": "final-editor",
                    "handoff": { "objective": "Take over and finish." }
                }),
            ),
            model_tool_call(
                "call_after_handoff",
                "workspace_write_file",
                json!({ "path": "output/main.md", "content": "must not run" }),
            ),
        ])],
    );
    let profile = configure_handoff_profiles(&fixture).await;
    let handle = start_contract_agent_run(
        &fixture,
        &profile,
        AgentRunPresentation::Background,
        "handoff-trailing-tool",
    )
    .await;

    let run = wait_for_terminal_agent_run(&fixture.agent_repository, &handle.run_id).await;
    assert_eq!(run.status, AgentRunStatus::Failed);
    assert!(
        fixture
            .agent_repository
            .list_tasks(&handle.run_id)
            .await
            .expect("list handoff tasks")
            .is_empty()
    );
    let invocations = fixture
        .agent_repository
        .list_invocations(&handle.run_id)
        .await
        .expect("list invocations");
    assert_eq!(invocations.len(), 1);
    assert_eq!(invocations[0].id, ROOT_AGENT_INVOCATION_ID);
    assert_eq!(invocations[0].status, AgentInvocationStatus::Failed);
    let failure_code = wait_for_event_field(
        &fixture.agent_repository,
        &handle.run_id,
        "run_failed",
        "code",
    )
    .await
    .expect("run failed event");
    assert_eq!(failure_code, "agent.tool_after_finish");

    let _ = fs::remove_dir_all(root).await;
}

async fn configure_return_mode_profiles(
    fixture: &AgentRuntimeFixture,
) -> tt_domain::models::agent::profile::ResolvedAgentProfile {
    let mut root = fixture
        .profile_service
        .load_profile("default-writer")
        .await
        .expect("load root profile")
        .expect("root profile exists");
    let mut child = root.clone();
    child.id = AgentProfileId::parse("scene-critic").expect("child profile id");
    child.display_name = "Scene Critic".to_string();
    child.tools.max_rounds = 1;
    child.tools.allow.retain(|name| {
        !matches!(
            name.as_str(),
            "builtin:agent.list"
                | "builtin:agent.delegate"
                | "builtin:agent.handoff"
                | "builtin:agent.await"
        )
    });
    child.delegation = AgentDelegationPolicy {
        callable: true,
        allow_as_subagent: true,
        allowed_callers: vec![root.id.as_str().to_string()],
        ..Default::default()
    };
    root.tools.max_rounds = 2;
    root.delegation.can_delegate = true;
    allow_profile_tool(&mut root.tools.allow, "agent.delegate");
    allow_profile_tool(&mut root.tools.allow, "agent.await");
    fixture
        .profile_service
        .save_profile(child, fixture.service.tool_catalog())
        .await
        .expect("save child profile");
    fixture
        .profile_service
        .save_profile(root, fixture.service.tool_catalog())
        .await
        .expect("save root profile");
    resolve_contract_profile(fixture).await
}

async fn configure_handoff_profiles(
    fixture: &AgentRuntimeFixture,
) -> tt_domain::models::agent::profile::ResolvedAgentProfile {
    let mut root = fixture
        .profile_service
        .load_profile("default-writer")
        .await
        .expect("load root profile")
        .expect("root profile exists");
    let mut target = root.clone();
    target.id = AgentProfileId::parse("final-editor").expect("target profile id");
    target.display_name = "Final Editor".to_string();
    target.run.direct_runnable = false;
    target.tools.max_rounds = 1;
    target.tools.allow.retain(|name| {
        matches!(
            name.as_str(),
            "builtin:workspace.finish"
                | "builtin:workspace.read_file"
                | "builtin:workspace.write_file"
        )
    });
    target.delegation = AgentDelegationPolicy {
        callable: true,
        allow_as_handoff_target: true,
        allowed_callers: vec![root.id.as_str().to_string()],
        ..Default::default()
    };
    root.tools.max_rounds = 1;
    root.delegation.can_handoff = true;
    allow_profile_tool(&mut root.tools.allow, "agent.handoff");
    fixture
        .profile_service
        .save_profile(target, fixture.service.tool_catalog())
        .await
        .expect("save handoff target profile");
    fixture
        .profile_service
        .save_profile(root, fixture.service.tool_catalog())
        .await
        .expect("save root profile");
    resolve_contract_profile(fixture).await
}

fn allow_profile_tool(allow: &mut Vec<String>, name: &str) {
    let id = format!("builtin:{name}");
    if !allow.iter().any(|tool| tool == &id) {
        allow.push(id);
    }
}

fn model_tool_response(tool_calls: Vec<Value>) -> Value {
    json!({
        "choices": [{
            "message": {
                "role": "assistant",
                "content": null,
                "tool_calls": tool_calls,
            }
        }]
    })
}

fn model_tool_call(id: &str, name: &str, arguments: Value) -> Value {
    json!({
        "id": id,
        "type": "function",
        "function": {
            "name": name,
            "arguments": serde_json::to_string(&arguments).expect("serialize tool arguments"),
        }
    })
}

fn message_text_for_role(request: &AgentModelRequest, role: AgentModelRole) -> &str {
    request
        .messages
        .iter()
        .find(|message| message.role == role)
        .and_then(|message| {
            message.parts.iter().find_map(|part| match part {
                AgentModelContentPart::Text { text } => Some(text.as_str()),
                _ => None,
            })
        })
        .expect("message text for role")
}
