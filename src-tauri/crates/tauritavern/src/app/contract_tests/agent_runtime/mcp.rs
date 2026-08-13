use super::*;

#[tokio::test]
async fn agent_runtime_executes_cached_mcp_tool_through_readable_alias() {
    let root = temp_root("agent-mcp-integration");
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
                    "path": "tool-results/call_fe79da5f09df9787.txt",
                    "start_line": 1,
                    "line_count": 100
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
        "mcp-tool-call",
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
    assert!(mcp_result.content.contains("too large to include here"));
    assert!(mcp_result.content.contains("workspace_read_file"));
    assert!(mcp_result.content.contains("## Prefix preview"));
    assert_eq!(mcp_result.structured["externalized"], true);
    assert_eq!(mcp_result.structured["charLimit"], 10_000);
    let readable_path = mcp_result.structured["path"].as_str().unwrap();
    let audit_path = mcp_result.structured["auditPath"].as_str().unwrap();
    assert_eq!(readable_path, "tool-results/call_fe79da5f09df9787.txt");
    assert_eq!(audit_path, "tool-results/call_fe79da5f09df9787.json");
    assert!(!mcp_result.content.contains(audit_path));
    assert_eq!(
        mcp_result.resource_refs,
        vec![readable_path.to_string(), audit_path.to_string()]
    );
    let stored = read_workspace_json(&fixture.agent_repository, &handle.run_id, audit_path).await;
    assert_eq!(stored["content"].as_str().unwrap().len(), 60_000);
    assert_eq!(stored["structured"]["structuredContent"]["issueId"], 42);
    let readable = fixture
        .agent_repository
        .read_text(
            &handle.run_id,
            &WorkspacePath::parse(readable_path).unwrap(),
        )
        .await
        .expect("read line-addressable MCP result");
    assert!(
        readable
            .text
            .lines()
            .all(|line| line.chars().count() <= 3_000)
    );
    assert!(readable.text.contains(&"x".repeat(3_000)));
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
    assert_eq!(read_result.structured["fullRead"], true);
    assert!(read_result.content.contains(&"x".repeat(3_000)));

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
            std::collections::BTreeMap::new(),
            tt_domain::models::mcp::McpProtocolVersionPreference::Auto,
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
