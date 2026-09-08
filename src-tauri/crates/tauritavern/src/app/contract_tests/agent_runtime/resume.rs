use super::*;
use tt_application::dto::agent_dto::{
    AgentCancelRunDto, AgentFinishRunPresentationDto, AgentOutputRevisionDto,
    AgentReadRunCheckpointDto, AgentReadRunCheckpointResultDto, AgentResumeRunDto,
};

#[tokio::test]
async fn agent_runtime_checkpoint_publishes_terminal_state_after_host_presentation() {
    let root = temp_root("agent-host-checkpoint");
    let fixture = agent_runtime_fixture(&root);
    let profile = resolve_contract_profile(&fixture).await;
    let handle = start_contract_agent_run_with_options(
        &fixture,
        &profile,
        "host-checkpoint",
        AgentStartRunOptionsDto {
            host_presentation: true,
            presentation: Some(AgentRunPresentation::Background),
            stream: Some(false),
            ..Default::default()
        },
        None,
    )
    .await;
    let terminal_seq = tokio::time::timeout(AGENT_CONTRACT_ASYNC_TIMEOUT, async {
        loop {
            let events = fixture
                .agent_repository
                .read_all_events(&handle.run_id)
                .await
                .unwrap();
            if let Some(event) = events
                .iter()
                .find(|event| event.event_type == "run_completed")
            {
                break event.seq;
            }
        }
    })
    .await
    .expect("runtime completion event");
    assert!(
        !fixture
            .agent_repository
            .load_run(&handle.run_id)
            .await
            .unwrap()
            .status
            .is_terminal()
    );
    assert!(
        fixture
            .agent_repository
            .load_run_checkpoint(&handle.run_id)
            .await
            .unwrap()
            .is_none()
    );

    // A late Stop must not supersede the already completed runtime while its
    // host presentation is still being saved.
    let stopped = fixture
        .service
        .cancel_run(AgentCancelRunDto {
            run_id: handle.run_id.clone(),
        })
        .await
        .unwrap();
    assert_eq!(stopped.status, AgentRunStatus::Completed);

    let presentation = json!({
        "messageId": "0",
        "rawCommittedText": "hello from real repo",
        "reasoningCursor": 2,
    });
    fixture
        .service
        .finish_run_presentation(AgentFinishRunPresentationDto {
            run_id: handle.run_id.clone(),
            terminal_seq,
            presentation: Some(presentation.clone()),
        })
        .await
        .expect("finish presentation and publish checkpoint");
    let completed = wait_for_checkpoint(&fixture, &handle.run_id).await;
    assert_eq!(completed.run.status, AgentRunStatus::Completed);
    assert_eq!(completed.terminal_seq, terminal_seq);
    assert_eq!(completed.presentation, Some(presentation));

    fs::remove_dir_all(root).await.unwrap();
}

#[tokio::test]
async fn agent_runtime_resume_retries_metadata_without_republishing_or_calling_model() {
    let root = temp_root("agent-resume-metadata");
    let fixture = agent_runtime_fixture_with_responses(
        &root,
        vec![model_tool_response(vec![
            model_tool_call(
                "write",
                "workspace_write_file",
                json!({
                    "path": "output/main.md", "content": "keep this reply"
                }),
            ),
            model_tool_call(
                "commit",
                "workspace_commit",
                json!({ "path": "output/main.md" }),
            ),
            model_tool_call("finish", "workspace_finish", json!({})),
        ])],
    );
    let profile = resolve_contract_profile(&fixture).await;
    let handle = start_contract_agent_run(
        &fixture,
        &profile,
        AgentRunPresentation::Foreground,
        "metadata-retry",
        Some(false),
    )
    .await;
    let commit = wait_for_event(&fixture, &handle.run_id, "chat_commit_requested", 0).await;
    fixture
        .service
        .resolve_chat_commit(AgentResolveChatCommitDto {
            run_id: handle.run_id.clone(),
            commit_id: commit.payload["commitId"].as_str().unwrap().to_string(),
            message_id: Some("0".to_string()),
            error: None,
        })
        .await
        .unwrap();
    let update = wait_for_event(
        &fixture,
        &handle.run_id,
        "persistent_state_metadata_update_requested",
        0,
    )
    .await;
    fixture
        .service
        .resolve_persistent_state_metadata_update(AgentResolvePersistentStateMetadataUpdateDto {
            run_id: handle.run_id.clone(),
            update_id: update.payload["updateId"].as_str().unwrap().to_string(),
            error: Some("chat temporarily unavailable".to_string()),
        })
        .await
        .unwrap();
    let stopped = wait_for_checkpoint(&fixture, &handle.run_id).await;
    assert_eq!(stopped.run.status, AgentRunStatus::PartialSuccess);
    assert_eq!(stopped.next_step, "finalize");
    assert!(stopped.blocked_reason.is_none());
    drop(fixture);

    let fixture = agent_runtime_fixture_with_responses(&root, Vec::new());
    resume_checkpoint(&fixture, &stopped, 0).await;
    let retried = wait_for_event(
        &fixture,
        &handle.run_id,
        "persistent_state_metadata_update_requested",
        stopped.terminal_seq,
    )
    .await;
    assert_eq!(retried.payload["stateId"], update.payload["stateId"]);
    fixture
        .service
        .resolve_persistent_state_metadata_update(AgentResolvePersistentStateMetadataUpdateDto {
            run_id: handle.run_id.clone(),
            update_id: retried.payload["updateId"].as_str().unwrap().to_string(),
            error: None,
        })
        .await
        .unwrap();
    let completed = wait_for_checkpoint(&fixture, &handle.run_id).await;
    assert_eq!(completed.run.status, AgentRunStatus::Completed);
    assert!(fixture.model_gateway.requests().await.is_empty());
    let events = read_agent_events(&fixture.agent_repository, &handle.run_id).await;
    assert_eq!(
        events
            .iter()
            .filter(|event| event.event_type == "persistent_changes_committed")
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event.event_type == "chat_commit_requested")
            .count(),
        1
    );
    fs::remove_dir_all(root).await.unwrap();
}

#[tokio::test]
async fn agent_runtime_resumes_after_tool_bookkeeping_failure_without_replaying_the_call() {
    for blocked_directory in ["tool-args", "tool-results"] {
        let root = temp_root("agent-tool-bookkeeping");
        let fixture = agent_runtime_fixture_with_responses(
            &root,
            vec![model_tool_response(vec![
                model_tool_call(
                    "append_once",
                    "workspace_write_file",
                    json!({
                        "path": "output/main.md", "mode": "append", "content": "once"
                    }),
                ),
                model_tool_call("finish", "workspace_finish", json!({})),
            ])],
        );
        let profile = resolve_contract_profile(&fixture).await;
        // Hold the first model response until the real workspace is initialized.
        let response = fixture.model_gateway.responses.lock().await;
        let handle = start_contract_agent_run(
            &fixture,
            &profile,
            AgentRunPresentation::Background,
            blocked_directory,
            Some(false),
        )
        .await;
        let mut requests = fixture.model_gateway.request_count.subscribe();
        tokio::time::timeout(
            AGENT_CONTRACT_ASYNC_TIMEOUT,
            requests.wait_for(|count| *count == 1),
        )
        .await
        .unwrap()
        .unwrap();
        let run = fixture
            .agent_repository
            .load_run(&handle.run_id)
            .await
            .unwrap();
        let blocked_path = root
            .join("_tauritavern/agent-workspaces/chats")
            .join(&run.workspace_id)
            .join("runs")
            .join(&run.id)
            .join(blocked_directory);
        fixture
            .agent_repository
            .write_text(
                &run.id,
                &WorkspacePath::parse("output/main.md").unwrap(),
                "before:",
            )
            .await
            .unwrap();
        if fs::try_exists(&blocked_path).await.unwrap() {
            fs::remove_dir(&blocked_path).await.unwrap();
        }
        fs::write(&blocked_path, "temporarily unavailable directory")
            .await
            .unwrap();
        drop(response);
        let stopped = wait_for_checkpoint(&fixture, &run.id).await;
        assert_eq!(stopped.run.status, AgentRunStatus::Failed);
        assert!(stopped.blocked_reason.is_none());
        fs::remove_file(&blocked_path).await.unwrap();
        fs::create_dir(&blocked_path).await.unwrap();
        resume_checkpoint(&fixture, &stopped, 0).await;
        let completed = wait_for_checkpoint(&fixture, &run.id).await;
        assert_eq!(completed.run.status, AgentRunStatus::Completed);
        assert_eq!(fixture.model_gateway.requests().await.len(), 1);
        assert_eq!(
            read_output(&fixture, &run.id).await,
            if blocked_directory == "tool-results" {
                "before:once"
            } else {
                "before:"
            }
        );
        fs::remove_dir_all(root).await.unwrap();
    }
}

#[tokio::test]
async fn agent_runtime_completed_checkpoint_retains_final_native_turn_and_publication() {
    let root = temp_root("agent-completed-checkpoint");
    let mut responses = default_agent_responses();
    let native = json!({
        "openai_responses": {
            "responseId": "response-finished",
            "output": [{
                "type": "reasoning",
                "id": "reasoning-finished",
                "encrypted_content": "opaque-final-reasoning"
            }]
        }
    });
    responses[1]["choices"][0]["message"]["native"] = native.clone();
    let fixture = agent_runtime_fixture_with_responses(&root, responses);
    let profile = resolve_contract_profile(&fixture).await;
    let handle = start_contract_agent_run(
        &fixture,
        &profile,
        AgentRunPresentation::Background,
        "completed-checkpoint",
        Some(false),
    )
    .await;
    let completed = wait_for_checkpoint(&fixture, &handle.run_id).await;
    assert_eq!(completed.run.status, AgentRunStatus::Completed);
    assert_eq!(completed.next_step, "finished");
    let bytes = fixture
        .agent_repository
        .load_run_checkpoint(&handle.run_id)
        .await
        .unwrap()
        .unwrap();
    let checkpoint: Value = serde_json::from_slice(&bytes).unwrap();
    let request: AgentModelRequest =
        serde_json::from_value(checkpoint["state"]["foreground"]["prepared"]["request"].clone())
            .unwrap();
    assert!(
        request
            .messages
            .iter()
            .flat_map(|message| &message.parts)
            .any(|part| {
                matches!(part, AgentModelContentPart::Native { provider, value }
            if provider == "openai_responses" && *value == native["openai_responses"])
            })
    );
    let final_message = request.messages.last().expect("final tool response");
    assert_eq!(final_message.role, AgentModelRole::Tool);
    assert!(
        matches!(&final_message.parts[0], AgentModelContentPart::ToolResult { result }
        if result.call_id == "call_finish" && !result.is_error)
    );
    let published_state_id = checkpoint["state"]["publishedState"]["stateId"]
        .as_str()
        .unwrap();
    fixture
        .agent_repository
        .validate_persistent_state(&handle.workspace_id, published_state_id)
        .await
        .unwrap();

    let run = fixture
        .agent_repository
        .load_run(&handle.run_id)
        .await
        .unwrap();
    let error = fixture
        .service
        .resume_run(AgentResumeRunDto {
            run_id: run.id,
            expected_terminal_seq: completed.terminal_seq,
            chat_ref: run.chat_ref,
            stable_chat_id: run.stable_chat_id,
            additional_rounds: 0,
            host_presentation: false,
            revision: None,
        })
        .await
        .expect_err("completed execution cannot resume");
    assert!(
        error
            .to_string()
            .contains("completed runs cannot be resumed")
    );
    assert_eq!(fixture.model_gateway.requests().await.len(), 2);

    fs::remove_dir_all(root).await.unwrap();
}

#[tokio::test]
async fn agent_runtime_resume_after_restart_preserves_history_and_successful_tools() {
    let root = temp_root("agent-resume-restart");
    let mut first_response = model_tool_response(vec![model_tool_call(
        "call_append_first",
        "workspace_write_file",
        json!({ "path": "output/main.md", "mode": "append", "content": "first" }),
    )]);
    first_response["choices"][0]["message"]["native"] = json!({
        "openai_responses": {
            "responseId": "response-first",
            "output": [{
                "type": "reasoning",
                "id": "reasoning-first",
                "encrypted_content": "opaque-native-reasoning"
            }]
        }
    });
    let fixture = agent_runtime_fixture_with_results(
        &root,
        vec![
            Ok(first_response),
            Err(ApplicationError::Transient(
                "model connection interrupted".to_string(),
            )),
        ],
    );
    let profile = configure_resume_profile(&fixture, 2, None).await;
    let handle = start_contract_agent_run(
        &fixture,
        &profile,
        AgentRunPresentation::Background,
        "resume-restart",
        Some(false),
    )
    .await;
    let stopped = wait_for_checkpoint(&fixture, &handle.run_id).await;
    assert_eq!(stopped.run.status, AgentRunStatus::Failed);
    let requests = fixture.model_gateway.requests().await;
    assert_eq!(requests.len(), 2);
    let previous_history = requests[1].messages.clone();
    drop(fixture);

    let fixture = agent_runtime_fixture_with_responses(
        &root,
        vec![model_tool_response(vec![
            model_tool_call(
                "call_append_second",
                "workspace_write_file",
                json!({ "path": "output/main.md", "mode": "append", "content": " second" }),
            ),
            model_tool_call("call_finish", "workspace_finish", json!({})),
        ])],
    );
    resume_checkpoint(&fixture, &stopped, 0).await;
    let completed = wait_for_checkpoint(&fixture, &handle.run_id).await;
    assert_eq!(completed.run.status, AgentRunStatus::Completed);
    assert!(completed.terminal_seq > stopped.terminal_seq);
    assert_eq!(
        fixture.model_gateway.requests().await[0].messages,
        previous_history
    );
    assert_eq!(read_output(&fixture, &handle.run_id).await, "first second");
    fs::remove_dir_all(root).await.unwrap();
}

#[tokio::test]
async fn agent_runtime_resume_keeps_partial_turn_cursor_after_confirmed_tool() {
    let root = temp_root("agent-resume-tool-cursor");
    let fixture = agent_runtime_fixture_with_responses(
        &root,
        vec![model_tool_response(vec![
            model_tool_call(
                "call_mcp",
                "mcp__my_server__issue_create",
                json!({ "title": "Create exactly once" }),
            ),
            model_tool_call(
                "call_write_after_mcp",
                "workspace_write_file",
                json!({ "path": "output/main.md", "content": "continued original turn" }),
            ),
            model_tool_call("call_finish", "workspace_finish", json!({})),
        ])],
    );
    fixture
        .mcp_gateway
        .wait_for_cancel
        .store(true, Ordering::SeqCst);
    let (profile, _) = super::mcp::configure_mcp_profile(&fixture, "resume-mcp", 1, 10_000).await;
    let handle = start_contract_agent_run(
        &fixture,
        &profile,
        AgentRunPresentation::Background,
        "resume-tool-cursor",
        Some(false),
    )
    .await;
    tokio::time::timeout(
        AGENT_CONTRACT_ASYNC_TIMEOUT,
        fixture.mcp_gateway.call_started.notified(),
    )
    .await
    .expect("MCP tool started");
    fixture
        .service
        .cancel_run(AgentCancelRunDto {
            run_id: handle.run_id.clone(),
        })
        .await
        .unwrap();
    let stopped = wait_for_checkpoint(&fixture, &handle.run_id).await;
    assert_eq!(stopped.run.status, AgentRunStatus::Cancelled);
    assert_eq!(fixture.mcp_gateway.calls.lock().await.len(), 1);
    drop(fixture);

    let fixture = agent_runtime_fixture_with_responses(&root, Vec::new());
    resume_checkpoint(&fixture, &stopped, 0).await;
    let completed = wait_for_checkpoint(&fixture, &handle.run_id).await;
    assert_eq!(completed.run.status, AgentRunStatus::Completed);
    assert!(fixture.model_gateway.requests().await.is_empty());
    assert!(fixture.mcp_gateway.calls.lock().await.is_empty());
    assert_eq!(
        read_output(&fixture, &handle.run_id).await,
        "continued original turn"
    );

    fs::remove_dir_all(root).await.unwrap();
}

#[tokio::test]
async fn agent_runtime_resume_adds_rounds_without_resetting_tool_budget() {
    let root = temp_root("agent-resume-budget");
    let fixture = agent_runtime_fixture_with_responses(
        &root,
        vec![model_tool_response(vec![model_tool_call(
            "call_first_write",
            "workspace_write_file",
            json!({ "path": "output/main.md", "mode": "append", "content": "first" }),
        )])],
    );
    let profile = configure_resume_profile(&fixture, 1, Some(1)).await;
    let handle = start_contract_agent_run(
        &fixture,
        &profile,
        AgentRunPresentation::Background,
        "resume-budget",
        Some(false),
    )
    .await;
    let stopped = wait_for_checkpoint(&fixture, &handle.run_id).await;
    assert_eq!(stopped.run.status, AgentRunStatus::Failed);
    assert_eq!(stopped.max_rounds, 1);
    drop(fixture);

    let fixture = agent_runtime_fixture_with_responses(
        &root,
        vec![model_tool_response(vec![
            model_tool_call(
                "call_disallowed_second_write",
                "workspace_write_file",
                json!({ "path": "output/main.md", "mode": "append", "content": " duplicated" }),
            ),
            model_tool_call("call_finish", "workspace_finish", json!({})),
        ])],
    );
    resume_checkpoint(&fixture, &stopped, 1).await;
    let completed = wait_for_checkpoint(&fixture, &handle.run_id).await;
    assert_eq!(completed.run.status, AgentRunStatus::Completed);
    assert_eq!(completed.max_rounds, 2);
    assert_eq!(read_output(&fixture, &handle.run_id).await, "first");
    let events = read_agent_events(&fixture.agent_repository, &handle.run_id).await;
    assert!(events.iter().any(|event| {
        event.event_type == "tool_call_failed"
            && event.payload["callId"] == "call_disallowed_second_write"
            && event.payload["errorCode"] == "agent.tool_budget_exhausted"
    }));

    fs::remove_dir_all(root).await.unwrap();
}

#[tokio::test]
async fn agent_runtime_resume_preserves_cancelled_child_progress() {
    for during_preparation in [false, true] {
        let root = temp_root("agent-resume-child");
        let fixture = agent_runtime_fixture_with_responses(
            &root,
            vec![model_tool_response(vec![
                model_tool_call(
                    "call_delegate",
                    "agent_delegate",
                    json!({
                        "agentId": "scene-critic",
                        "task": { "objective": "Write a revision note." }
                    }),
                ),
                model_tool_call(
                    "call_first_await",
                    "agent_await",
                    json!({ "mode": "allCompleted" }),
                ),
            ])],
        );
        let profile = super::delegation::configure_return_mode_profiles(&fixture).await;
        let frozen_input = if during_preparation {
            use tt_domain::models::agent::profile::{AgentPresetBindingMode, AgentPresetRef};
            fixture
                .preset_repository
                .save_preset(&Preset::new(
                    "child-prompt".to_string(),
                    PresetType::OpenAI,
                    json!({}),
                ))
                .await
                .unwrap();
            let mut child = fixture
                .profile_service
                .load_profile("scene-critic")
                .await
                .unwrap()
                .unwrap();
            child.preset.mode = AgentPresetBindingMode::Ref;
            child.preset.ref_ = Some(AgentPresetRef {
                api_id: "openai".to_string(),
                name: "child-prompt".to_string(),
            });
            fixture
                .profile_service
                .save_profile(child, fixture.service.tool_catalog())
                .await
                .unwrap();
            Some(json!({
                "schemaVersion": 1, "kind": "tauritavern.agentFrozenRunInputSnapshot", "generationType": "normal",
                "promptInputs": {}, "worldInfoActivation": { "entries": [] }, "macroContext": {},
                "currentModelConnection": {
                    "schemaVersion": 1, "kind": "tauritavern.currentModelConnectionSnapshot",
                    "settings": { "chat_completion_source": "custom", "model": "contract-model", "custom_model": "contract-model" }
                }
            }))
        } else {
            fixture
                .model_gateway
                .wait_for_cancel_on_request
                .store(2, Ordering::SeqCst);
            None
        };
        let handle = start_contract_agent_run_with_options(
            &fixture,
            &profile,
            "resume-child",
            AgentStartRunOptionsDto {
                presentation: Some(AgentRunPresentation::Background),
                stream: Some(false),
                ..Default::default()
            },
            frozen_input,
        )
        .await;
        if during_preparation {
            wait_for_event(&fixture, &handle.run_id, "prompt_assembly_requested", 0).await;
            assert_eq!(fixture.model_gateway.requests().await.len(), 1);
        } else {
            let mut request_count = fixture.model_gateway.request_count.subscribe();
            tokio::time::timeout(
                AGENT_CONTRACT_ASYNC_TIMEOUT,
                request_count.wait_for(|count| *count >= 2),
            )
            .await
            .expect("child model request started")
            .unwrap();
        }
        fixture
            .service
            .cancel_run(AgentCancelRunDto {
                run_id: handle.run_id.clone(),
            })
            .await
            .unwrap();
        let stopped = wait_for_checkpoint(&fixture, &handle.run_id).await;
        assert!(stopped.blocked_reason.is_none());
        let tasks = fixture
            .agent_repository
            .list_tasks(&handle.run_id)
            .await
            .unwrap();
        assert_eq!(tasks.len(), 1);
        let child_invocation_id = tasks[0].child_invocation_id.clone();
        drop(fixture);

        let fixture = agent_runtime_fixture_with_responses(
            &root,
            if during_preparation {
                vec![model_tool_response(vec![model_tool_call(
                    "call_finish",
                    "workspace_finish",
                    json!({}),
                )])]
            } else {
                vec![
                    model_tool_response(vec![model_tool_call(
                        "call_resumed_await",
                        "agent_await",
                        json!({ "mode": "allCompleted" }),
                    )]),
                    model_tool_response(vec![model_tool_call(
                        "call_finish",
                        "workspace_finish",
                        json!({}),
                    )]),
                ]
            },
        );
        if !during_preparation {
            fixture
                .model_gateway
                .invocation_responses
                .lock()
                .await
                .insert(
                    child_invocation_id.clone(),
                    VecDeque::from([Ok(model_tool_response(vec![
                        model_tool_call(
                            "call_child_write",
                            "workspace_write_file",
                            json!({
                                "path": "summaries/note.md", "content": "Add rain."
                            }),
                        ),
                        model_tool_call(
                            "call_child_return",
                            "task_return",
                            json!({ "summary": "Add rain." }),
                        ),
                    ]))]),
                );
        }
        resume_checkpoint(&fixture, &stopped, 1).await;
        let completed = wait_for_checkpoint(&fixture, &handle.run_id).await;
        assert_eq!(completed.run.status, AgentRunStatus::Completed);
        let resumed_tasks = fixture
            .agent_repository
            .list_tasks(&handle.run_id)
            .await
            .unwrap();
        assert_eq!(resumed_tasks.len(), 1);
        assert_eq!(resumed_tasks[0].id, tasks[0].id);
        assert_eq!(
            resumed_tasks[0].status,
            if during_preparation {
                AgentTaskStatus::Cancelled
            } else {
                AgentTaskStatus::Completed
            }
        );
        assert_eq!(
            fixture
                .model_gateway
                .requests()
                .await
                .iter()
                .filter(|request| { request.provider_state["invocationId"] == child_invocation_id })
                .count(),
            usize::from(!during_preparation)
        );
        if !during_preparation {
            let note = fixture
                .agent_repository
                .read_text(
                    &handle.run_id,
                    &WorkspacePath::parse("summaries/note.md").unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(note.text, "Add rain.");
        }

        fs::remove_dir_all(root).await.unwrap();
    }
}

#[tokio::test]
async fn agent_runtime_revises_completed_output_and_resumes_without_replaying_work() {
    let root = temp_root("agent-output-revision");
    let fixture = agent_runtime_fixture_with_responses(
        &root,
        vec![model_tool_response(vec![
            model_tool_call(
                "write",
                "workspace_write_file",
                json!({ "path": "output/main.md", "content": "Original ending." }),
            ),
            model_tool_call("commit", "workspace_commit", json!({})),
            model_tool_call("finish", "workspace_finish", json!({})),
        ])],
    );
    let profile = configure_resume_profile(&fixture, 2, Some(1)).await;
    let handle = start_contract_agent_run(
        &fixture,
        &profile,
        AgentRunPresentation::Foreground,
        "revision",
        Some(false),
    )
    .await;
    let state_id =
        acknowledge_revision_output(&fixture, &handle.run_id, 0, Some("Original ending.")).await;
    let completed = wait_for_checkpoint(&fixture, &handle.run_id).await;

    fixture
        .model_gateway
        .wait_for_cancel_on_request
        .store(2, Ordering::SeqCst);
    revise_checkpoint(
        &fixture,
        &completed,
        "Make the ending quieter.",
        "An ending edited by hand.",
    )
    .await;
    let mut requests = fixture.model_gateway.request_count.subscribe();
    tokio::time::timeout(
        AGENT_CONTRACT_ASYNC_TIMEOUT,
        requests.wait_for(|count| *count == 2),
    )
    .await
    .unwrap()
    .unwrap();
    fixture
        .service
        .cancel_run(AgentCancelRunDto {
            run_id: handle.run_id.clone(),
        })
        .await
        .unwrap();
    let stopped = wait_for_checkpoint(&fixture, &handle.run_id).await;
    assert_eq!(stopped.run.status, AgentRunStatus::Cancelled);
    let request = fixture.model_gateway.requests().await.pop().unwrap();
    assert!(request.messages.iter().flat_map(|message| &message.parts).any(|part| {
        matches!(part, AgentModelContentPart::ToolResult { result } if result.call_id == "finish" && !result.is_error)
    }), "the original completed tool turn remains in context");
    assert!(request.messages.iter().flat_map(|message| &message.parts).any(|part| {
        matches!(part, AgentModelContentPart::Text { text } if text.contains("Make the ending quieter."))
    }));
    let revision_id = request.provider_state["invocationId"]
        .as_str()
        .unwrap()
        .to_string();
    assert_ne!(revision_id, ROOT_AGENT_INVOCATION_ID);
    assert_eq!(
        fixture
            .agent_repository
            .read_text(
                &handle.run_id,
                &WorkspacePath::parse("output/previous_output.md").unwrap()
            )
            .await
            .unwrap()
            .text,
        "An ending edited by hand."
    );
    drop(fixture);

    let fixture = agent_runtime_fixture_with_responses(
        &root,
        vec![model_tool_response(vec![
            model_tool_call(
                "read",
                "workspace_read_file",
                json!({ "path": "output/previous_output.md" }),
            ),
            model_tool_call(
                "write",
                "workspace_write_file",
                json!({ "path": "output/revised.md", "content": "A quieter ending edited by hand." }),
            ),
            model_tool_call(
                "commit",
                "workspace_commit",
                json!({ "path": "output/revised.md" }),
            ),
            model_tool_call("finish", "workspace_finish", json!({})),
        ])],
    );
    resume_checkpoint(&fixture, &stopped, 0).await;
    let revised_state = acknowledge_revision_output(
        &fixture,
        &handle.run_id,
        stopped.terminal_seq,
        Some("A quieter ending edited by hand."),
    )
    .await;
    let revised = wait_for_checkpoint(&fixture, &handle.run_id).await;
    assert_eq!(revised.run.status, AgentRunStatus::Completed);
    assert_eq!(
        revised_state, state_id,
        "body-only revisions reuse the persistent version"
    );
    assert_eq!(
        fixture.model_gateway.requests().await[0].provider_state["invocationId"],
        revision_id
    );

    fixture
        .model_gateway
        .responses
        .lock()
        .await
        .push_back(Ok(model_tool_response(vec![model_tool_call(
            "finish",
            "workspace_finish",
            json!({}),
        )])));
    revise_checkpoint(
        &fixture,
        &revised,
        "Keep this version.",
        "A quieter ending edited by hand.",
    )
    .await;
    assert_eq!(
        acknowledge_revision_output(&fixture, &handle.run_id, revised.terminal_seq, None).await,
        state_id
    );
    assert_eq!(
        wait_for_checkpoint(&fixture, &handle.run_id)
            .await
            .run
            .status,
        AgentRunStatus::Completed
    );
    let events = read_agent_events(&fixture.agent_repository, &handle.run_id).await;
    assert_eq!(
        events
            .iter()
            .filter(|event| event.event_type == "chat_commit_requested")
            .count(),
        2,
        "a revision does not require a redundant chat commit"
    );
    let invocations = fixture
        .agent_repository
        .list_invocations(&handle.run_id)
        .await
        .unwrap();
    assert_eq!(invocations.len(), 3);
    assert!(
        invocations
            .iter()
            .all(|invocation| invocation.status == AgentInvocationStatus::Completed)
    );
    fs::remove_dir_all(root).await.unwrap();
}

async fn revise_checkpoint(
    fixture: &AgentRuntimeFixture,
    checkpoint: &AgentReadRunCheckpointResultDto,
    guidance: &str,
    previous_output: &str,
) {
    let run = fixture
        .agent_repository
        .load_run(&checkpoint.run.run_id)
        .await
        .unwrap();
    let handle = fixture
        .service
        .resume_run(AgentResumeRunDto {
            run_id: run.id,
            expected_terminal_seq: checkpoint.terminal_seq,
            chat_ref: run.chat_ref,
            stable_chat_id: run.stable_chat_id,
            additional_rounds: 0,
            host_presentation: false,
            revision: Some(AgentOutputRevisionDto {
                guidance: guidance.to_string(),
                previous_output: previous_output.to_string(),
            }),
        })
        .await
        .unwrap();
    assert_eq!(handle.after_seq, Some(checkpoint.terminal_seq));
}

async fn acknowledge_revision_output(
    fixture: &AgentRuntimeFixture,
    run_id: &str,
    after_seq: u64,
    output: Option<&str>,
) -> String {
    if let Some(output) = output {
        let commit = wait_for_event(fixture, run_id, "chat_commit_requested", after_seq).await;
        let path = WorkspacePath::parse(commit.payload["path"].as_str().unwrap()).unwrap();
        assert_eq!(
            fixture
                .agent_repository
                .read_text(run_id, &path)
                .await
                .unwrap()
                .text,
            output
        );
        fixture
            .service
            .resolve_chat_commit(AgentResolveChatCommitDto {
                run_id: run_id.to_string(),
                commit_id: commit.payload["commitId"].as_str().unwrap().to_string(),
                message_id: Some("0".to_string()),
                error: None,
            })
            .await
            .unwrap();
    }
    let update = wait_for_event(
        fixture,
        run_id,
        "persistent_state_metadata_update_requested",
        after_seq,
    )
    .await;
    fixture
        .service
        .resolve_persistent_state_metadata_update(AgentResolvePersistentStateMetadataUpdateDto {
            run_id: run_id.to_string(),
            update_id: update.payload["updateId"].as_str().unwrap().to_string(),
            error: None,
        })
        .await
        .unwrap();
    update.payload["stateId"].as_str().unwrap().to_string()
}

async fn configure_resume_profile(
    fixture: &AgentRuntimeFixture,
    max_rounds: usize,
    write_limit: Option<usize>,
) -> tt_domain::models::agent::profile::ResolvedAgentProfile {
    let mut profile = fixture
        .profile_service
        .load_profile(DEFAULT_AGENT_PROFILE_ID)
        .await
        .unwrap()
        .unwrap();
    profile.tools.max_rounds = max_rounds;
    profile.run.model_retry.max_retries = 0;
    if let Some(limit) = write_limit {
        profile
            .tools
            .max_calls_per_tool
            .insert("builtin:workspace.write_file".to_string(), limit);
    }
    fixture
        .profile_service
        .save_profile(profile, fixture.service.tool_catalog())
        .await
        .unwrap();
    resolve_contract_profile(fixture).await
}

pub(super) async fn wait_for_checkpoint(
    fixture: &AgentRuntimeFixture,
    run_id: &str,
) -> AgentReadRunCheckpointResultDto {
    if let Some(mut live) = fixture
        .service
        .subscribe_live_projection(run_id)
        .await
        .unwrap()
    {
        tokio::time::timeout(AGENT_CONTRACT_ASYNC_TIMEOUT, async {
            while live.changed().await.is_ok() {}
        })
        .await
        .expect("Agent released its completed execution");
    }
    fixture
        .service
        .read_run_checkpoint(AgentReadRunCheckpointDto {
            run_id: run_id.to_string(),
        })
        .await
        .expect("read saved checkpoint")
}

async fn resume_checkpoint(
    fixture: &AgentRuntimeFixture,
    checkpoint: &AgentReadRunCheckpointResultDto,
    additional_rounds: usize,
) {
    let run = fixture
        .agent_repository
        .load_run(&checkpoint.run.run_id)
        .await
        .unwrap();
    let handle = fixture
        .service
        .resume_run(AgentResumeRunDto {
            run_id: run.id.clone(),
            expected_terminal_seq: checkpoint.terminal_seq,
            chat_ref: run.chat_ref,
            stable_chat_id: run.stable_chat_id,
            additional_rounds,
            host_presentation: false,
            revision: None,
        })
        .await
        .expect("resume saved execution");
    assert_eq!(handle.run_id, run.id);
    assert_eq!(handle.after_seq, Some(checkpoint.terminal_seq));
}

async fn read_output(fixture: &AgentRuntimeFixture, run_id: &str) -> String {
    fixture
        .agent_repository
        .read_text(run_id, &WorkspacePath::parse("output/main.md").unwrap())
        .await
        .unwrap()
        .text
}

async fn wait_for_event(
    fixture: &AgentRuntimeFixture,
    run_id: &str,
    event_type: &str,
    after_seq: u64,
) -> tt_domain::models::agent::AgentRunEvent {
    tokio::time::timeout(AGENT_CONTRACT_ASYNC_TIMEOUT, async {
        loop {
            if let Some(event) = read_agent_events(&fixture.agent_repository, run_id)
                .await
                .into_iter()
                .find(|event| event.seq > after_seq && event.event_type == event_type)
            {
                return event;
            }
        }
    })
    .await
    .expect("expected runtime event")
}
