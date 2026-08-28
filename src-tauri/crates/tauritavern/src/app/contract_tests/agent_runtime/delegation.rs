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
                    "workspace_write_file",
                    json!({
                        "path": "output/main.md",
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
        .read_text(
            &handle.run_id,
            &WorkspacePath::parse("output/main.md").unwrap(),
        )
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
    resolve_contract_profile(fixture).await
}
