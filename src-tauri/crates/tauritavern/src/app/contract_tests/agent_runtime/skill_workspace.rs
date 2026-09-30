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
                model_tool_call("parent_read", "read", json!({ "file_path": path })),
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
                model_tool_call("child_read", "read", json!({ "file_path": path })),
                model_tool_call(
                    "child_return",
                    "task_return",
                    json!({ "summary": "Critique complete.", "status": "completed" }),
                ),
            ]),
            model_tool_response(vec![model_tool_call("finish", "finish", json!({}))]),
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

#[tokio::test]
async fn agent_runtime_places_the_workspace_index_only_where_instructions_ask() {
    let root = temp_root("agent-workspace-placeholder");
    let finish = || model_tool_response(vec![model_tool_call("finish", "finish", json!({}))]);
    let fixture = agent_runtime_fixture_with_responses(&root, vec![finish(), finish()]);
    let profile = resolve_contract_profile(&fixture).await;
    for (id, instructions) in [
        (
            "run_index_middle",
            "Before the index.\n\n{{workspace}}\n\nAfter the index.",
        ),
        ("run_index_absent", "Use the available Agent tools."),
    ] {
        let run = contract_run(id, AgentRunPresentation::Background, &profile);
        fixture.agent_repository.create_run(&run).await.unwrap();
        let mut request = chat_request("Write the reply.");
        request.payload["messages"][0]["content"] = json!(instructions);
        let snapshot = json!({ "chatCompletionPayload": request.payload.clone() });
        let (_cancel, mut receiver) = watch::channel(false);
        fixture
            .service
            .execute_agent_loop_run_inner(
                &run.id,
                snapshot,
                request,
                profile.clone(),
                &mut receiver,
            )
            .await
            .expect("run completes");
    }

    let requests = fixture.model_gateway.requests().await;
    let placed = message_text_for_role(&requests[0], AgentModelRole::System);
    let index = placed.find("# Workspace").expect("the index is placed");
    let after = placed.find("After the index.").unwrap();
    assert!(
        placed.starts_with("Before the index.") && index < after,
        "{placed}"
    );
    assert!(!placed.contains("{{workspace}}"), "{placed}");
    let absent = message_text_for_role(&requests[1], AgentModelRole::System);
    assert!(!absent.contains("# Workspace"), "{absent}");

    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn agent_runtime_mounts_the_current_chat_as_read_only_floors() {
    use tt_domain::models::chat::ChatMessage;

    let root = temp_root("agent-chat-mount");
    let character = "雪之下 雪乃（v2.1)";
    let floor = |index: usize, file: &str| format!("floors/{index:06}/{file}");
    let fixture = agent_runtime_fixture_with_responses(
        &root,
        vec![
            model_tool_response(vec![
                model_tool_call("list", "list", json!({ "depth": 2 })),
                model_tool_call(
                    "read_raw",
                    "read",
                    json!({ "file_path": floor(0, "message.md") }),
                ),
                model_tool_call(
                    "read_hidden",
                    "read",
                    json!({ "file_path": floor(1, "meta.json") }),
                ),
                model_tool_call(
                    "read_after_input",
                    "read",
                    json!({ "file_path": floor(2, "message.md") }),
                ),
                model_tool_call(
                    "write_mount",
                    "write",
                    json!({ "file_path": floor(0, "message.md"), "content": "edited" }),
                ),
                model_tool_call("shell_cat", "shell", json!({ "command": "cat /chat.json" })),
                model_tool_call(
                    "grep_regex",
                    "grep",
                    json!({ "pattern": "(?i)^hello|隐藏的?旁白" }),
                ),
                model_tool_call("grep_after_input", "grep", json!({ "pattern": "froze" })),
                model_tool_call(
                    "grep_literal",
                    "grep",
                    json!({ "path": "floors", "pattern": r"\{\{char\}\}" }),
                ),
                model_tool_call(
                    "grep_results_default",
                    "grep",
                    json!({ "pattern": "floorCount" }),
                ),
                model_tool_call(
                    "grep_results_named",
                    "grep",
                    json!({ "path": "tool-results", "pattern": "floorCount" }),
                ),
                model_tool_call("grep_invalid", "grep", json!({ "pattern": "(" })),
            ]),
            model_tool_response(vec![model_tool_call("finish", "finish", json!({}))]),
        ],
    );
    let mut profile = resolve_contract_profile(&fixture).await;
    profile.tools.max_calls_per_run = 20;
    let mut run = contract_run("run_chat_mount", AgentRunPresentation::Background, &profile);
    let target = run.chat_target_mut().unwrap();
    target.stable_chat_id = "3f9a1c2e-7b4d-4c1a-9e2f-0123456789ab".into();
    target.chat_ref = AgentChatRef::Character {
        character_id: character.into(),
        file_name: "Branch #2.jsonl".into(),
    };
    target.input_message_count = Some(2);
    let mut chat = Chat::new("User", character);
    chat.file_name = Some("Branch #2.jsonl".into());
    chat.add_message(ChatMessage::user("User", "Hello {{char}}, <b>raw</b>"));
    let mut hidden = ChatMessage::character(character, "一段隐藏的旁白。");
    hidden.is_system = true;
    chat.add_message(hidden);
    chat.add_message(ChatMessage::character(
        character,
        "Written after the input froze.",
    ));
    fixture.chat_repository.save(&chat).await.unwrap();
    fixture.agent_repository.create_run(&run).await.unwrap();
    let mut request = chat_request("Read the chat mount.");
    // The default instructions end with the index placeholder.
    request.payload["messages"][0]["content"] =
        json!("Use the available Agent tools.\n\n{{workspace}}");
    let snapshot = json!({
        "chatCompletionPayload": request.payload.clone(),
        "frozenRunInputSnapshot": {
            "macroContext": { "names": { "char": "Frozen Yukino" } }
        }
    });
    let (_cancel, mut receiver) = watch::channel(false);
    fixture
        .service
        .execute_agent_loop_run_inner(&run.id, snapshot, request, profile, &mut receiver)
        .await
        .expect("chat mount run completes");

    let requests = fixture.model_gateway.requests().await;
    let instructions = message_text_for_role(&requests[0], AgentModelRole::System);
    assert!(instructions.contains("chat.json"), "{instructions}");
    let results = requests[1]
        .messages
        .iter()
        .flat_map(|message| &message.parts)
        .filter_map(|part| match part {
            AgentModelContentPart::ToolResult { result } => Some((result.call_id.as_str(), result)),
            _ => None,
        })
        .collect::<HashMap<_, _>>();

    let listed = results["list"].structured["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["path"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    // Listing the root shows the chat mount beside the Run roots.
    for expected in ["chat.json", "floors/000000", "floors/000001"] {
        assert!(listed.iter().any(|path| path == expected), "{listed:?}");
    }
    assert!(
        !listed.iter().any(|path| path == "floors/000002"),
        "{listed:?}"
    );

    let raw = &results["read_raw"];
    assert!(!raw.is_error, "{}", raw.content);
    assert!(
        raw.content.contains("Hello {{char}}, <b>raw</b>"),
        "{}",
        raw.content
    );
    assert!(!raw.content.contains("Frozen Yukino"), "{}", raw.content);

    let hidden = &results["read_hidden"];
    assert!(!hidden.is_error, "{}", hidden.content);
    assert!(
        hidden.content.contains(r#""is_system": true"#),
        "{}",
        hidden.content
    );

    let after_input = &results["read_after_input"];
    assert!(after_input.is_error);
    assert_eq!(
        after_input.error_code.as_deref(),
        Some("workspace.file_not_found")
    );

    let write = &results["write_mount"];
    assert!(write.is_error);
    assert_eq!(
        write.error_code.as_deref(),
        Some("workspace.path_not_writable")
    );

    let shell = &results["shell_cat"];
    assert!(!shell.is_error, "{}", shell.content);
    assert!(
        shell.content.contains(r#""floorCount": 2"#),
        "{}",
        shell.content
    );

    let hit_paths = |id: &str| {
        results[id].structured["matches"]
            .as_array()
            .unwrap_or_else(|| panic!("{id}: {}", results[id].content))
            .iter()
            .map(|found| found["path"].as_str().unwrap().to_string())
            .collect::<Vec<_>>()
    };
    let hidden_hit = floor(1, "message.md");
    // Without a path, grep covers Run files and chat floors together.
    let regex_hits = hit_paths("grep_regex");
    assert!(
        regex_hits.contains(&floor(0, "message.md")),
        "{regex_hits:?}"
    );
    assert!(regex_hits.contains(&hidden_hit), "{regex_hits:?}");
    assert!(
        results["grep_regex"]
            .content
            .contains(&format!("{hidden_hit} [hidden]")),
        "{}",
        results["grep_regex"].content
    );
    assert!(
        !hit_paths("grep_after_input")
            .iter()
            .any(|path| path.starts_with("floors/"))
    );
    assert_eq!(hit_paths("grep_literal"), [floor(0, "message.md")]);
    // Tool results are searched only when the path names them.
    assert!(
        !hit_paths("grep_results_default")
            .iter()
            .any(|path| path.starts_with("tool-results/"))
    );
    let named = hit_paths("grep_results_named");
    assert!(
        !named.is_empty(),
        "{}",
        results["grep_results_named"].content
    );
    assert!(named.iter().all(|path| path.starts_with("tool-results/")));
    let invalid = &results["grep_invalid"];
    assert!(invalid.is_error);
    assert_eq!(
        invalid.error_code.as_deref(),
        Some("workspace.grep_pattern_invalid")
    );

    let _ = fs::remove_dir_all(root).await;
}
