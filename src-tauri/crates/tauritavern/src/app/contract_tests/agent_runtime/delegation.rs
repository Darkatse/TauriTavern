use super::*;

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
                    "write",
                    json!({ "file_path": "summaries/note.md", "content": "Add rain." }),
                ),
                model_tool_call(
                    "call_child_return",
                    "task_return",
                    json!({ "summary": "Add a concrete sound.", "status": "completed" }),
                ),
            ]),
            model_text_response("Done."),
        ],
    );
    let profile = configure_return_mode_profiles(&fixture).await;
    let handle = start_contract_agent_run(
        &fixture,
        &profile,
        AgentRunPresentation::Background,
        "delegate-return-child",
        Some(false),
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
    let detail = fixture
        .service
        .read_task_detail(tt_application::dto::agent_dto::AgentReadTaskDetailDto {
            run_id: handle.run_id.clone(),
            task_id: task.id.clone(),
            include_result: true,
        })
        .await
        .expect("read persisted task detail after completion");
    assert_eq!(detail.task.objective, "Return one concrete revision note.");
    assert_eq!(detail.child_invocation_id, task.child_invocation_id);
    assert_eq!(
        detail.result.expect("returned result").summary,
        "Add a concrete sound."
    );
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
    assert!(
        message_text_for_role(&requests[0], AgentModelRole::System)
            .contains("scene-critic [delegate]")
    );
    assert!(
        !message_text_for_role(&requests[1], AgentModelRole::System).contains("Available agents:")
    );
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
            "workspace.commit" | "agent.delegate" | "agent.handoff" | "agent.await"
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
                    "write",
                    json!({ "file_path": "output/main.md", "content": "Committed draft." }),
                ),
                model_tool_call(
                    "call_commit",
                    "commit",
                    json!({ "reason": "Deliver the reply." }),
                ),
                model_tool_call(
                    "call_private_note",
                    "write",
                    json!({ "file_path": "scratch/private.md", "content": "Root-only note." }),
                ),
                model_tool_call(
                    "call_handoff",
                    "agent_handoff",
                    json!({
                        "agentId": "final-editor",
                        "handoff": { "objective": "Review the committed draft and finish." }
                    }),
                ),
            ]),
            model_tool_response(vec![
                model_tool_call(
                    "call_target_denied",
                    "write",
                    json!({
                        "file_path": "scratch/forbidden.md", "content": "must not be written"
                    }),
                ),
                model_tool_call(
                    "call_target_write",
                    "write",
                    json!({
                        "file_path": "summaries/review.md", "content": "Reviewed."
                    }),
                ),
            ]),
            // The denied write reaches the target before it can finish.
            model_text_response("Done."),
        ],
    );
    let profile = configure_handoff_profiles(&fixture).await;
    let mut editor = fixture
        .profile_service
        .load_profile("final-editor")
        .await
        .unwrap()
        .unwrap();
    editor.workspace.visible_roots = vec!["output".into(), "summaries".into()];
    editor.workspace.writable_roots = vec!["output".into(), "summaries".into()];
    editor.tools.max_rounds = 2;
    fixture
        .profile_service
        .save_profile(editor, fixture.service.tool_catalog())
        .await
        .unwrap();
    let handle = start_contract_agent_run(
        &fixture,
        &profile,
        AgentRunPresentation::Foreground,
        "handoff-after-commit",
        Some(false),
    )
    .await;
    resolve_chat_commits_and_persistent_state_update(
        fixture.service.clone(),
        fixture.agent_repository.clone(),
        handle.run_id.clone(),
        "message_handoff",
        &[],
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
    let files = fixture
        .agent_repository
        .open_filesystem(&handle.run_id)
        .await
        .unwrap();
    assert!(
        files
            .metadata(Some(&WorkspacePath::parse("scratch/forbidden.md").unwrap()))
            .await
            .is_err()
    );
    assert_eq!(
        files
            .read_text(&WorkspacePath::parse("summaries/review.md").unwrap())
            .await
            .unwrap()
            .text,
        "Reviewed."
    );
    assert!(
        events
            .iter()
            .any(|event| event.event_type == "tool_call_failed"
                && event.payload["callId"] == "call_target_denied"
                && event.payload["isError"] == true)
    );
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
    assert_eq!(requests.len(), 3);
    assert!(
        message_text_for_role(&requests[0], AgentModelRole::System)
            .contains("final-editor [handoff]")
    );
    let target_system = message_text_for_role(&requests[1], AgentModelRole::System);
    assert!(!target_system.contains("Available agents:"));
    // The target starts with the files its own roots can see, written earlier in this Run.
    assert!(target_system.contains("output/main.md"));
    assert!(!target_system.contains("scratch/private.md"));
    assert_eq!(
        requests[1].provider_state["invocationId"],
        task.child_invocation_id
    );
    assert!(
        requests[1]
            .tools
            .iter()
            .any(|tool| tool.tool_id.native_name() == "workspace.write_file")
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
            .any(|binding| binding["descriptor"]["id"] == "builtin:workspace.write_file")
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
        Some(false),
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
async fn agent_runtime_recovers_handoff_before_trailing_tool() {
    let root = temp_root("agent-handoff-trailing-tool");
    let fixture = agent_runtime_fixture_with_responses(
        &root,
        vec![
            model_tool_response(vec![
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
                    "write",
                    json!({
                        "file_path": "output/main.md",
                        "content": "Complete this work before handing off."
                    }),
                ),
            ]),
            model_tool_response(vec![model_tool_call(
                "call_handoff_retry",
                "agent_handoff",
                json!({
                    "agentId": "final-editor",
                    "handoff": { "objective": "Take over and finish." }
                }),
            )]),
            model_text_response("Done."),
        ],
    );
    let profile = configure_handoff_profiles(&fixture).await;
    let handle = start_contract_agent_run(
        &fixture,
        &profile,
        AgentRunPresentation::Background,
        "handoff-trailing-tool",
        Some(false),
    )
    .await;

    let run = wait_for_terminal_agent_run(&fixture.agent_repository, &handle.run_id).await;
    assert_eq!(run.status, AgentRunStatus::Completed);
    let tasks = fixture
        .agent_repository
        .list_tasks(&handle.run_id)
        .await
        .expect("list handoff tasks");
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].status, AgentTaskStatus::Completed);
    let root_invocation = fixture
        .agent_repository
        .load_invocation(&handle.run_id, ROOT_AGENT_INVOCATION_ID)
        .await
        .expect("load root invocation");
    assert_eq!(root_invocation.status, AgentInvocationStatus::Transferred);
    let artifact = fixture
        .agent_repository
        .open_filesystem(&handle.run_id)
        .await
        .expect("open workspace")
        .read_text(&WorkspacePath::parse("output/main.md").unwrap())
        .await
        .expect("read artifact");
    assert_eq!(artifact.text, "Complete this work before handing off.");
    let events = read_agent_events(&fixture.agent_repository, &handle.run_id).await;
    assert!(events.iter().any(|event| {
        event.event_type == "tool_call_failed"
            && event.payload["callId"] == "call_handoff"
            && event.payload["errorCode"] == "agent.tool_after_finish"
    }));
    assert!(!events.iter().any(|event| event.event_type == "run_failed"));

    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn agent_runtime_foreground_handoff_requires_a_target_that_can_publish() {
    use tt_application::dto::agent_dto::AgentCancelRunDto;

    let root = temp_root("agent-handoff-unpublishable-target");
    let fixture = agent_runtime_fixture_with_responses(
        &root,
        vec![model_tool_response(vec![model_tool_call(
            "call_handoff",
            "agent_handoff",
            json!({
                "agentId": "final-editor",
                "handoff": { "objective": "Write the reply." }
            }),
        )])],
    );
    let profile = configure_handoff_profiles(&fixture).await;
    // Saved for background runs, where it could end without publishing; in a foreground
    // Run every stage that can end the run must be able to publish.
    let mut editor = fixture
        .profile_service
        .load_profile("final-editor")
        .await
        .unwrap()
        .unwrap();
    editor.run.presentation = AgentRunPresentation::Background;
    editor
        .tools
        .allow
        .retain(|name| name != "builtin:workspace.commit");
    fixture
        .profile_service
        .save_profile(editor, fixture.service.tool_catalog())
        .await
        .unwrap();
    fixture
        .model_gateway
        .wait_for_cancel_on_request
        .store(2, Ordering::SeqCst);
    let handle = start_contract_agent_run(
        &fixture,
        &profile,
        AgentRunPresentation::Foreground,
        "handoff-unpublishable-target",
        Some(false),
    )
    .await;
    let mut request_count = fixture.model_gateway.request_count.subscribe();
    tokio::time::timeout(
        AGENT_CONTRACT_ASYNC_TIMEOUT,
        request_count.wait_for(|count| *count >= 2),
    )
    .await
    .expect("the next model request started")
    .unwrap();

    let events = read_agent_events(&fixture.agent_repository, &handle.run_id).await;
    assert!(events.iter().any(|event| {
        event.event_type == "tool_call_failed"
            && event.payload["callId"] == "call_handoff"
            && event.payload["errorCode"] == "agent.handoff_policy_denied"
    }));
    assert!(
        fixture
            .agent_repository
            .list_tasks(&handle.run_id)
            .await
            .unwrap()
            .is_empty()
    );
    let requests = fixture.model_gateway.requests().await;
    assert_eq!(
        requests[1].provider_state["invocationId"],
        ROOT_AGENT_INVOCATION_ID
    );
    assert!(
        !message_text_for_role(&requests[0], AgentModelRole::System)
            .contains("final-editor [handoff]")
    );

    fixture
        .service
        .cancel_run(AgentCancelRunDto {
            run_id: handle.run_id.clone(),
        })
        .await
        .unwrap();
    let run = wait_for_terminal_agent_run(&fixture.agent_repository, &handle.run_id).await;
    assert_eq!(run.status, AgentRunStatus::Cancelled);

    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn agent_runtime_preset_handoff_target_prompt_ends_the_stage_as_the_run_does() {
    use tt_application::dto::agent_dto::{AgentCancelRunDto, AgentReadPromptAssemblyRequestDto};
    use tt_domain::models::agent::profile::{AgentPresetBindingMode, AgentPresetRef};

    let root = temp_root("agent-handoff-preset-prompt");
    let fixture = agent_runtime_fixture_with_responses(
        &root,
        vec![model_tool_response(vec![model_tool_call(
            "call_handoff",
            "agent_handoff",
            json!({
                "agentId": "final-editor",
                "handoff": { "objective": "Write the reply." }
            }),
        )])],
    );
    let profile = configure_handoff_profiles(&fixture).await;
    fixture
        .preset_repository
        .save_preset(&Preset::new(
            "editor-prompt".to_string(),
            PresetType::OpenAI,
            json!({}),
        ))
        .await
        .unwrap();
    // The target cannot hand off, and is saved for background runs.
    let mut editor = fixture
        .profile_service
        .load_profile("final-editor")
        .await
        .unwrap()
        .unwrap();
    editor.run.presentation = AgentRunPresentation::Background;
    editor.preset.mode = AgentPresetBindingMode::Ref;
    editor.preset.ref_ = Some(AgentPresetRef {
        api_id: "openai".to_string(),
        name: "editor-prompt".to_string(),
    });
    fixture
        .profile_service
        .save_profile(editor, fixture.service.tool_catalog())
        .await
        .unwrap();
    let handle = start_contract_agent_run_with_options(
        &fixture,
        &profile,
        "handoff-preset-prompt",
        AgentStartRunOptionsDto {
            presentation: Some(AgentRunPresentation::Foreground),
            stream: Some(false),
            ..Default::default()
        },
        Some(json!({
            "schemaVersion": 1, "kind": "tauritavern.agentFrozenRunInputSnapshot", "generationType": "normal",
            "promptInputs": {}, "worldInfoActivation": { "entries": [] }, "macroContext": {},
            "currentModelConnection": {
                "schemaVersion": 1, "kind": "tauritavern.currentModelConnectionSnapshot",
                "settings": { "chat_completion_source": "custom", "model": "contract-model", "custom_model": "contract-model" }
            }
        })),
    )
    .await;

    wait_for_event_type(
        &fixture.agent_repository,
        &handle.run_id,
        "prompt_assembly_requested",
    )
    .await;
    let assembly_id = read_agent_events(&fixture.agent_repository, &handle.run_id)
        .await
        .into_iter()
        .find(|event| event.event_type == "prompt_assembly_requested")
        .and_then(|event| event.payload["assemblyId"].as_str().map(str::to_owned))
        .expect("assembly id");
    let request = fixture
        .service
        .read_prompt_assembly_request(AgentReadPromptAssemblyRequestDto {
            run_id: handle.run_id.clone(),
            assembly_id,
        })
        .await
        .unwrap();
    // The target joins a foreground Run: it finishes with a final commit, as the runtime
    // will require, instead of by a plain-text reply.
    assert!(
        request
            .agent_system_prompt
            .contains("commit with finish: true")
    );
    assert!(
        !request
            .agent_system_prompt
            .contains("reply without calling a tool")
    );

    fixture
        .service
        .cancel_run(AgentCancelRunDto {
            run_id: handle.run_id.clone(),
        })
        .await
        .unwrap();
    let run = wait_for_terminal_agent_run(&fixture.agent_repository, &handle.run_id).await;
    assert_eq!(run.status, AgentRunStatus::Cancelled);

    let _ = fs::remove_dir_all(root).await;
}

pub(super) async fn configure_return_mode_profiles(
    fixture: &AgentRuntimeFixture,
) -> tt_domain::models::agent::profile::ResolvedAgentProfile {
    let mut root =
        crate::app::contract_tests::contract_writer_definition(&fixture.profile_service).await;
    let mut child = root.clone();
    child.id = AgentProfileId::parse("scene-critic").expect("child profile id");
    child.display_name = "Scene Critic".to_string();
    child.tools.max_rounds = 1;
    // The return-mode invocation must prune these capabilities from the profile.
    child.delegation = AgentDelegationPolicy {
        can_delegate: true,
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
    resolve_saved_default_profile(fixture).await
}

async fn configure_handoff_profiles(
    fixture: &AgentRuntimeFixture,
) -> tt_domain::models::agent::profile::ResolvedAgentProfile {
    let mut root =
        crate::app::contract_tests::contract_writer_definition(&fixture.profile_service).await;
    let mut target = root.clone();
    target.id = AgentProfileId::parse("final-editor").expect("target profile id");
    target.display_name = "Final Editor".to_string();
    target.run.direct_runnable = false;
    target.tools.max_rounds = 1;
    target.tools.allow.retain(|name| {
        matches!(
            name.as_str(),
            "builtin:workspace.commit"
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
    root.tools.max_rounds = 2;
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
    resolve_saved_default_profile(fixture).await
}
