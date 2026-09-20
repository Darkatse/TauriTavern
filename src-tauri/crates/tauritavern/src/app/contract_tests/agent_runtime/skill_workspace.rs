use super::*;
use tt_domain::models::skill::{
    SkillImportInput, SkillInlineFile, SkillInstallRequest, SkillScope,
};
use tt_ports::repositories::skill_repository::SkillRepository;

#[tokio::test]
async fn agent_runtime_parent_and_child_read_their_own_skill_binding() {
    let root = temp_root("agent-skill-scopes");
    let path = "skills/style/SKILL.md";
    let fixture = agent_runtime_fixture_with_responses(
        &root,
        vec![
            model_tool_response(vec![
                model_tool_call(
                    "parent_read",
                    "workspace_read_file",
                    json!({ "path": path }),
                ),
                model_tool_call(
                    "delegate",
                    "agent_delegate",
                    json!({
                        "agentId": "scene-critic",
                        "task": { "objective": "Read your style skill and return a critique." }
                    }),
                ),
                model_tool_call(
                    "await_child",
                    "agent_await",
                    json!({ "mode": "allCompleted", "timeoutMs": 5_000 }),
                ),
            ]),
            model_tool_response(vec![
                model_tool_call("child_read", "workspace_read_file", json!({ "path": path })),
                model_tool_call(
                    "child_return",
                    "task_return",
                    json!({ "summary": "Critique complete.", "status": "completed" }),
                ),
            ]),
            model_tool_response(vec![model_tool_call(
                "finish",
                "workspace_finish",
                json!({}),
            )]),
        ],
    );
    let profile = super::delegation::configure_return_mode_profiles(&fixture).await;
    let repository = FileSkillRepository::new(root.join("_tauritavern/skills"));
    for (profile_id, description, body) in [
        (
            profile.id.as_str(),
            "Writer style guidance",
            "Write for {{char}}.",
        ),
        (
            "scene-critic",
            "Critic style guidance",
            "Critique for {{char}}.",
        ),
    ] {
        repository
            .install_import(SkillInstallRequest {
                target_scope: SkillScope::Profile {
                    profile_id: profile_id.to_string(),
                },
                conflict_strategy: None,
                input: SkillImportInput::InlineFiles {
                    source: json!({ "kind": "test" }),
                    files: vec![SkillInlineFile {
                        path: "SKILL.md".to_string(),
                        content: format!(
                            "---\nname: style\ndescription: {description}\n---\n{body}"
                        ),
                        encoding: "utf8".to_string(),
                        media_type: None,
                        size_bytes: None,
                        sha256: None,
                    }],
                },
            })
            .await
            .expect("install scoped skill");
    }

    let run = contract_run(
        "run_skill_scopes",
        AgentRunPresentation::Background,
        &profile,
    );
    fixture.agent_repository.create_run(&run).await.unwrap();
    let mut request = chat_request("Use the style skill and ask for a critique.");
    request.payload["messages"] = json!([
        {
            "role": "system",
            "content": "Follow the writer's rules.",
            "_tauritavern_prompt_component": "agentSystemPrompt"
        },
        { "role": "user", "content": "Use the style skill and ask for a critique." }
    ]);
    let snapshot = json!({
        "chatCompletionPayload": request.payload,
        "frozenRunInputSnapshot": {
            "macroContext": { "names": { "char": "Frozen Alice" } }
        }
    });
    let (_cancel, mut receiver) = watch::channel(false);
    tokio::time::timeout(
        AGENT_CONTRACT_ASYNC_TIMEOUT,
        fixture.service.execute_agent_loop_run_inner(
            &run.id,
            snapshot,
            request,
            profile,
            &mut receiver,
        ),
    )
    .await
    .expect("scoped skill run completes")
    .expect("run scoped skill task");

    let requests = fixture.model_gateway.requests().await;
    assert_eq!(requests.len(), 3);
    for (index, expected, other) in [
        (0, "Writer style guidance", "Critic style guidance"),
        (1, "Critic style guidance", "Writer style guidance"),
        (2, "Writer style guidance", "Critic style guidance"),
    ] {
        let instructions = message_text_for_role(&requests[index], AgentModelRole::System);
        assert!(instructions.contains(expected));
        assert!(!instructions.contains(other));
        assert_eq!(instructions.matches(path).count(), 1);
    }
    let events = read_agent_events(&fixture.agent_repository, &run.id).await;
    for (call_id, request_index, expected, other) in [
        (
            "parent_read",
            0,
            "Write for Frozen Alice.",
            "Critique for Frozen Alice.",
        ),
        (
            "child_read",
            1,
            "Critique for Frozen Alice.",
            "Write for Frozen Alice.",
        ),
    ] {
        let event = events
            .iter()
            .find(|event| {
                event.event_type == "tool_result_stored" && event.payload["callId"] == call_id
            })
            .expect("workspace read result was stored");
        assert_eq!(
            event.payload["invocationId"],
            requests[request_index].provider_state["invocationId"]
        );
        let result = read_workspace_json(
            &fixture.agent_repository,
            &run.id,
            event.payload["path"].as_str().unwrap(),
        )
        .await;
        assert_eq!(result["isError"], false, "{result}");
        assert_eq!(result["structured"]["path"], path);
        let content = result["content"].as_str().unwrap();
        assert!(content.contains(expected), "{content}");
        assert!(!content.contains(other), "{content}");
    }

    let original = read_workspace_json(
        &fixture.agent_repository,
        &run.id,
        "input/prompt_snapshot.json",
    )
    .await;
    assert_eq!(
        original["chatCompletionPayload"]["messages"][0]["content"],
        "Follow the writer's rules."
    );
    fs::remove_dir_all(root).await.unwrap();
}
