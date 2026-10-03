use super::*;
use tt_domain::models::agent::AgentToolResult;
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

#[tokio::test]
async fn agent_runtime_places_the_workspace_index_only_where_instructions_ask() {
    let root = temp_root("agent-workspace-placeholder");
    let finish = || {
        model_tool_response(vec![model_tool_call(
            "finish",
            "workspace_finish",
            json!({}),
        )])
    };
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

/// Providers cache the prompt prefix, which starts with the expanded agent system prompt:
/// a new floor must not change it while the top-level `persist/` entries stay the same.
#[tokio::test]
async fn agent_runtime_keeps_the_default_writer_system_prompt_across_chat_floors() {
    use tt_domain::models::chat::ChatMessage;

    let root = temp_root("agent-prompt-cache");
    let reply = |content: &str| {
        vec![
            model_tool_call(
                "call_write",
                "write",
                json!({ "file_path": "output/main.md", "content": content }),
            ),
            model_tool_call(
                "call_commit",
                "commit",
                json!({ "reason": "Deliver the reply.", "finish": true }),
            ),
        ]
    };
    // The first floor leaves top-level persist/ entries; the two floors compared keep them.
    let mut seed = vec![
        model_tool_call(
            "call_plot",
            "write",
            json!({ "file_path": "persist/plot.md", "content": "Alice keeps the key." }),
        ),
        model_tool_call(
            "call_threads",
            "write",
            json!({ "file_path": "persist/threads.md", "content": "Who sent the letter?" }),
        ),
    ];
    seed.extend(reply("The door opens."));
    let fixture = agent_runtime_fixture_with_responses(
        &root,
        vec![
            model_tool_response(seed),
            model_tool_response(reply("Alice turns the key.")),
            model_tool_response(reply("The letter burns.")),
        ],
    );
    let profile = resolve_saved_default_profile(&fixture).await;
    // What the frontend places as the agentSystemPrompt component.
    let agent_system_prompt = fixture
        .service
        .resolve_agent_system_prompt(Some(DEFAULT_AGENT_PROFILE_ID))
        .await
        .unwrap();
    let mut chat = Chat::new("User", "Alice");
    chat.file_name = Some("cache.jsonl".into());
    chat.add_message(ChatMessage::user("User", "Open the door."));
    fixture.chat_repository.save(&chat).await.unwrap();

    let mut system_prompts = Vec::new();
    for message_id in ["message_1", "message_3", "message_5"] {
        let first_request = fixture.model_gateway.requests().await.len();
        let mut request = chat_request("Continue.");
        request.payload["messages"][0]["content"] = json!(agent_system_prompt);
        let handle = fixture
            .service
            .start_run(AgentStartRunDto {
                chat_ref: AgentChatRef::Character {
                    character_id: "Alice".into(),
                    file_name: "cache.jsonl".into(),
                },
                stable_chat_id: "stable-cache".into(),
                generation_type: "normal".into(),
                profile_id: Some(DEFAULT_AGENT_PROFILE_ID.into()),
                persist_base_state_id: None,
                prompt_snapshot: Some(json!({
                    "contextPolicy": &profile.context,
                    "chatCompletionPayload": request.payload,
                })),
                frozen_run_input_snapshot: None,
                generation_intent: None,
                skill_scope_refs: AgentSkillScopeRefsDto::default(),
                options: AgentStartRunOptionsDto {
                    presentation: Some(AgentRunPresentation::Foreground),
                    stream: Some(false),
                    ..Default::default()
                },
            })
            .await
            .unwrap();
        resolve_chat_commits_and_persistent_state_update(
            fixture.service.clone(),
            fixture.agent_repository.clone(),
            handle.run_id.clone(),
            message_id,
            &[],
        )
        .await
        .unwrap();
        let run = wait_for_terminal_agent_run(&fixture.agent_repository, &handle.run_id).await;
        assert_eq!(run.status, AgentRunStatus::Completed);
        let persist = fixture
            .agent_repository
            .open_filesystem(&handle.run_id)
            .await
            .unwrap()
            .list_files(Some(&WorkspacePath::parse("persist").unwrap()), 1, 10)
            .await
            .unwrap();
        assert_eq!(persist.entries.len(), 2);
        let requests = fixture.model_gateway.requests().await;
        system_prompts.push(
            message_text_for_role(&requests[first_request], AgentModelRole::System).to_owned(),
        );

        // The host saves the reply with the state it published, and the user answers.
        let state_id = read_agent_events(&fixture.agent_repository, &handle.run_id)
            .await
            .into_iter()
            .find(|event| event.event_type == "persistent_state_metadata_update_requested")
            .expect("persistent state update")
            .payload["stateId"]
            .clone();
        chat.messages.push(
            serde_json::from_value(json!({
                "name": "Alice",
                "mes": "Reply.",
                "extra": { "tauritavern": { "agent": {
                    "persistStateId": state_id,
                    "persistStateStatus": "committed"
                } } }
            }))
            .unwrap(),
        );
        chat.add_message(ChatMessage::user("User", "Go on."));
        fixture.chat_repository.save(&chat).await.unwrap();
    }
    assert_eq!(system_prompts[1], system_prompts[2]);

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
                model_tool_call(
                    "chat_search_hidden",
                    "chat_search",
                    json!({ "query": "隐藏的旁白" }),
                ),
                model_tool_call(
                    "chat_read_hidden",
                    "chat_read",
                    json!({ "floors": [{ "floor": 1 }] }),
                ),
            ]),
            model_tool_response(vec![model_tool_call(
                "finish",
                "workspace_finish",
                json!({}),
            )]),
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
        hidden.content.contains(r#""role": "assistant""#)
            && hidden.content.contains(r#""hidden": true"#),
        "{}",
        hidden.content
    );
    // The chat tools state a hidden floor's role as its meta.json does, marked like grep.
    for id in ["chat_search_hidden", "chat_read_hidden"] {
        assert!(
            results[id].content.contains("floor 1 assistant [hidden]"),
            "{id}: {}",
            results[id].content
        );
    }

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

#[derive(Clone, Copy)]
enum ChatKind {
    Character,
    Group,
}

/// Runs `calls` in one round of a `kind` chat holding `messages` as written in the chat
/// file, and returns the agent system prompt with each call's result.
async fn run_in_chat(
    label: &str,
    kind: ChatKind,
    messages: &[Value],
    edit_profile: impl FnOnce(&mut tt_domain::models::agent::profile::ResolvedAgentProfile),
    calls: Vec<Value>,
) -> (String, HashMap<String, AgentToolResult>) {
    let root = temp_root(label);
    let fixture = agent_runtime_fixture_with_responses(
        &root,
        vec![
            model_tool_response(calls),
            model_tool_response(vec![model_tool_call(
                "finish",
                "workspace_finish",
                json!({}),
            )]),
        ],
    );
    let mut profile = resolve_contract_profile(&fixture).await;
    profile.tools.max_calls_per_run = 20;
    edit_profile(&mut profile);
    let mut run = contract_run(label, AgentRunPresentation::Background, &profile);
    let target = run.chat_target_mut().unwrap();
    target.input_message_count = Some(messages.len());
    let (path, mut payload) = match kind {
        ChatKind::Character => {
            let file_name = format!("{label}.jsonl");
            let mut chat = Chat::new("User", "Alice");
            chat.file_name = Some(file_name.clone());
            fixture.chat_repository.save(&chat).await.unwrap();
            let path = fixture
                .chat_repository
                .get_chat_payload_path("Alice", &file_name)
                .await
                .unwrap();
            target.chat_ref = AgentChatRef::Character {
                character_id: "Alice".into(),
                file_name,
            };
            let header = fs::read_to_string(&path)
                .await
                .unwrap()
                .trim_end()
                .to_owned();
            (path, header)
        }
        ChatKind::Group => {
            let path = root
                .join("default-user/group chats")
                .join(format!("{label}.jsonl"));
            fs::create_dir_all(path.parent().unwrap()).await.unwrap();
            target.chat_ref = AgentChatRef::Group {
                chat_id: label.to_string(),
            };
            let header =
                json!({ "chat_metadata": {}, "user_name": "User", "character_name": "unused" });
            (path, header.to_string())
        }
    };
    for message in messages {
        payload.push('\n');
        payload.push_str(&message.to_string());
    }
    fs::write(&path, payload).await.unwrap();
    fixture.agent_repository.create_run(&run).await.unwrap();
    let mut request = chat_request("Read the chat.");
    request.payload["messages"][0]["content"] =
        json!("Use the available Agent tools.\n\n{{workspace}}");
    let snapshot = json!({ "chatCompletionPayload": request.payload.clone() });
    let (_cancel, mut receiver) = watch::channel(false);
    fixture
        .service
        .execute_agent_loop_run_inner(&run.id, snapshot, request, profile, &mut receiver)
        .await
        .expect("chat mount run completes");

    let requests = fixture.model_gateway.requests().await;
    let instructions = message_text_for_role(&requests[0], AgentModelRole::System).to_owned();
    let results = requests[1]
        .messages
        .iter()
        .flat_map(|message| &message.parts)
        .filter_map(|part| match part {
            AgentModelContentPart::ToolResult { result } => {
                Some((result.call_id.clone(), result.clone()))
            }
            _ => None,
        })
        .collect();
    let _ = fs::remove_dir_all(root).await;
    (instructions, results)
}

fn result_paths(result: &AgentToolResult, key: &str) -> Vec<String> {
    result.structured[key]
        .as_array()
        .unwrap_or_else(|| panic!("{}", result.content))
        .iter()
        .map(|entry| entry["path"].as_str().unwrap().to_string())
        .collect()
}

#[tokio::test]
async fn agent_runtime_chat_floor_without_text_fails_only_its_message() {
    let floor = |index: usize, file: &str| format!("floors/{index:06}/{file}");
    let (_, results) = run_in_chat(
        "agent-chat-mount-floor-without-text",
        ChatKind::Character,
        &[
            json!({ "name": "User", "is_user": true, "mes": "The lantern was lit." }),
            json!({ "name": "Alice", "is_user": false, "mes": null }),
            json!({ "name": "Alice", "is_user": false, "mes": "The lantern went out." }),
        ],
        |_| {},
        vec![
            model_tool_call("list_root", "list", json!({})),
            model_tool_call(
                "list_floors",
                "list",
                json!({ "path": "floors", "depth": 2 }),
            ),
            model_tool_call(
                "read_broken",
                "read",
                json!({ "file_path": floor(1, "message.md") }),
            ),
            model_tool_call(
                "read_meta",
                "read",
                json!({ "file_path": floor(1, "meta.json") }),
            ),
            model_tool_call(
                "read_next",
                "read",
                json!({ "file_path": floor(2, "message.md") }),
            ),
            model_tool_call("grep", "grep", json!({ "pattern": "lantern" })),
        ],
    )
    .await;

    assert!(
        !results["list_root"].is_error,
        "{}",
        results["list_root"].content
    );
    let listed = result_paths(&results["list_floors"], "entries");
    for expected in [
        floor(0, "message.md"),
        floor(1, "meta.json"),
        floor(2, "message.md"),
    ] {
        assert!(listed.contains(&expected), "{listed:?}");
    }
    assert!(!listed.contains(&floor(1, "message.md")), "{listed:?}");

    let broken = &results["read_broken"];
    assert!(broken.is_error);
    assert_eq!(
        broken.error_code.as_deref(),
        Some("workspace.file_not_found")
    );
    assert!(
        broken.content.contains(&floor(1, "message.md"))
            && broken.content.contains("floor 1 has no string `mes` field"),
        "{}",
        broken.content
    );
    assert!(
        !results["read_meta"].is_error,
        "{}",
        results["read_meta"].content
    );
    assert!(
        results["read_next"]
            .content
            .contains("The lantern went out."),
        "{}",
        results["read_next"].content
    );

    let grep = &results["grep"];
    assert_eq!(
        result_paths(grep, "matches"),
        [floor(0, "message.md"), floor(2, "message.md")]
    );
    assert!(grep.content.contains("Skipped 1 floor"), "{}", grep.content);
}

#[tokio::test]
async fn agent_runtime_profile_without_chat_tools_has_no_chat_mount() {
    let (instructions, results) = run_in_chat(
        "agent-chat-mount-without-chat-tools",
        ChatKind::Character,
        &[json!({ "name": "User", "is_user": true, "mes": "The lantern was lit." })],
        |profile| {
            profile
                .tools
                .allow
                .retain(|id| !matches!(id.native_name(), "chat.search" | "chat.read_messages"))
        },
        vec![
            model_tool_call("list", "list", json!({ "depth": 2 })),
            model_tool_call("grep", "grep", json!({ "pattern": "lantern" })),
            model_tool_call(
                "read",
                "read",
                json!({ "file_path": "floors/000000/message.md" }),
            ),
        ],
    )
    .await;

    assert!(!instructions.contains("chat.json"), "{instructions}");
    let listed = result_paths(&results["list"], "entries");
    assert!(
        !listed
            .iter()
            .any(|path| path == "chat.json" || path.starts_with("floors")),
        "{listed:?}"
    );
    assert!(
        result_paths(&results["grep"], "matches").is_empty(),
        "{}",
        results["grep"].content
    );
    assert_eq!(
        results["read"].error_code.as_deref(),
        Some("workspace.file_not_found")
    );
}

#[tokio::test]
async fn agent_runtime_group_chat_tools_read_floors_without_a_mount() {
    let (instructions, results) = run_in_chat(
        "agent-group-chat-floors",
        ChatKind::Group,
        &[
            json!({ "name": "User", "is_user": true, "mes": "The lantern was lit." }),
            json!({ "name": "Alice", "is_user": false, "is_system": true, "mes": "The lantern flickered." }),
            json!({ "name": "Bob", "is_user": false, "mes": "The lantern went out." }),
        ],
        |_| {},
        vec![
            model_tool_call("search", "chat_search", json!({ "query": "lantern" })),
            model_tool_call(
                "read",
                "chat_read",
                json!({ "floors": [{ "floor": 1 }] }),
            ),
            model_tool_call(
                "search_hidden",
                "chat_search",
                json!({ "query": "lantern", "hidden": true }),
            ),
            model_tool_call(
                "search_system",
                "chat_search",
                json!({ "query": "lantern", "role": "system" }),
            ),
            model_tool_call("list", "list", json!({})),
        ],
    )
    .await;

    // A group chat has no chat files.
    assert!(!instructions.contains("chat.json"), "{instructions}");
    assert!(
        !result_paths(&results["list"], "entries")
            .iter()
            .any(|path| path == "chat.json" || path.starts_with("floors")),
        "{}",
        results["list"].content
    );
    // Its floors read as a character chat's do: the same role and hidden mark, by index.
    let hits = results["search"].structured["hits"]
        .as_array()
        .unwrap_or_else(|| panic!("{}", results["search"].content));
    assert_eq!(hits.len(), 3, "{}", results["search"].content);
    let hidden = hits.iter().find(|hit| hit["index"] == 1).unwrap();
    assert_eq!(hidden["role"], "assistant");
    assert_eq!(hidden["hidden"], true);
    assert!(hits.iter().all(|hit| hit.get("path").is_none()));
    let read = &results["read"];
    assert!(
        read.content.contains("floor 1 assistant [hidden] Alice")
            && read.content.contains("The lantern flickered."),
        "{}",
        read.content
    );
    // Hidden floors are selected as they are shown; `system` is the chat API's name for
    // them, which an older run may still send.
    let hidden_hits = results["search_hidden"].structured["hits"]
        .as_array()
        .unwrap_or_else(|| panic!("{}", results["search_hidden"].content));
    assert_eq!(hidden_hits.len(), 1);
    assert_eq!(hidden_hits[0]["index"], 1);
    let system = &results["search_system"];
    assert_eq!(system.error_code.as_deref(), Some("tool.invalid_arguments"));
    assert!(
        system.content.contains("hidden: true"),
        "{}",
        system.content
    );
}
