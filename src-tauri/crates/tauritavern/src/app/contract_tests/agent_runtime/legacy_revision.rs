use super::resume::{revise_checkpoint, wait_for_checkpoint};
use super::*;
use tt_application::dto::agent_dto::{
    AgentOutputRevisionDto, AgentReadRunCheckpointDto, AgentResumeRunDto,
};
use tt_domain::models::skill::{
    SkillImportInput, SkillInlineFile, SkillInstallRequest, SkillScope,
};
use tt_ports::repositories::skill_repository::SkillRepository;
use tt_ports::workspace_fs::WorkspaceWriteGuard;

#[tokio::test]
async fn completed_legacy_revision_preserves_work_and_translates_only_affected_turns() {
    let root = temp_root("agent-legacy-revision");
    let mut responses = default_agent_responses();
    let original_call = responses[0]["choices"][0]["message"]["tool_calls"][0].clone();
    responses[0]["choices"][0]["message"]["native"] = json!({
        "openai_responses": { "responseId": "response-unrelated", "output": [
            { "type": "reasoning", "encrypted_content": "unrelated-native" },
            { "type": "function_call", "call_id": original_call["id"],
              "name": original_call["function"]["name"], "arguments": original_call["function"]["arguments"] }
        ] }
    });
    let fixture = agent_runtime_fixture_with_responses(&root, responses);
    let profile = resolve_contract_profile(&fixture).await;
    let handle = start_contract_agent_run(
        &fixture,
        &profile,
        AgentRunPresentation::Background,
        "legacy-revision",
        Some(false),
    )
    .await;
    let completed = wait_for_checkpoint(&fixture, &handle.run_id).await;
    let mut checkpoint = saved_checkpoint(&fixture, &handle.run_id).await;
    let mut original: AgentModelRequest =
        serde_json::from_value(checkpoint["state"]["foreground"]["prepared"]["request"].clone())
            .unwrap();
    original.messages[0].parts = vec![AgentModelContentPart::Text {
        text: "Legacy Agent instructions: use agent.list to find collaborators.".into(),
    }];
    checkpoint["state"]["foreground"]["prepared"]["request"]["messages"] =
        serde_json::to_value(&original.messages).unwrap();
    let mut target = fixture
        .profile_service
        .load_profile(DEFAULT_AGENT_PROFILE_ID)
        .await
        .unwrap()
        .unwrap();
    target.id = AgentProfileId::parse("revision-editor").unwrap();
    target.tools.allow.retain(|name| {
        !matches!(
            name.as_str(),
            "builtin:agent.delegate" | "builtin:agent.handoff" | "builtin:agent.await"
        )
    });
    target.delegation = AgentDelegationPolicy {
        callable: true,
        allow_as_subagent: true,
        allowed_callers: vec![DEFAULT_AGENT_PROFILE_ID.into()],
        description_for_agents: Some("Current revision helper.".into()),
        ..Default::default()
    };
    fixture
        .profile_service
        .save_profile(target, fixture.service.tool_catalog())
        .await
        .unwrap();

    let repository = FileSkillRepository::new(root.join("_tauritavern/skills"));
    let mut bound_skills = Value::Null;
    for (scope, description, body) in [
        (
            SkillScope::Global,
            "Archived global style",
            "legacy-bound-global",
        ),
        (
            SkillScope::Profile {
                profile_id: profile.id.as_str().into(),
            },
            "Current profile style",
            "current-profile-shadow",
        ),
    ] {
        let installed = repository
            .install_import(SkillInstallRequest {
                target_scope: scope.clone(),
                conflict_strategy: None,
                input: SkillImportInput::InlineFiles {
                    source: json!({ "kind": "test" }),
                    files: vec![SkillInlineFile {
                        path: "SKILL.md".into(),
                        content: format!(
                            "---\nname: style\ndescription: {description}\n---\n{body}"
                        ),
                        encoding: "utf8".into(),
                        media_type: None,
                        size_bytes: None,
                        sha256: None,
                    }],
                },
            })
            .await
            .unwrap();
        if scope == SkillScope::Global {
            bound_skills = json!([installed.skill.unwrap()]);
        }
    }
    checkpoint["schemaVersion"] = json!(1);
    let prepared = &mut checkpoint["state"]["foreground"]["prepared"];
    prepared["effectiveSkills"] = bound_skills.clone();
    prepared["profile"]["schemaVersion"] = json!(3);
    prepared["profile"]["skills"]["maxReadCharsPerCall"] = json!(100_000);
    prepared["profile"]["skills"]["maxReadCharsPerRun"] = json!(200_000);
    prepared["profile"]["tools"]["allow"]
        .as_array_mut()
        .unwrap()
        .retain(|id| id != "builtin:workspace.shell");
    prepared["toolSnapshot"]["bindings"]
        .as_array_mut()
        .unwrap()
        .retain(|binding| binding["descriptor"]["id"] != "builtin:workspace.shell");
    prepared["toolTurn"]["tools"]
        .as_array_mut()
        .unwrap()
        .retain(|id| id != "builtin:workspace.shell");
    prepared["request"]["tools"]
        .as_array_mut()
        .unwrap()
        .retain(|tool| tool["toolId"] != "builtin:workspace.shell");
    let binding_template = prepared["toolSnapshot"]["bindings"][0].clone();
    let tool_template = prepared["request"]["tools"][0].clone();
    for name in [
        "skill.list",
        "skill.read",
        "skill.search",
        "skill.run_script",
        "agent.list",
    ] {
        let id = format!("builtin:{name}");
        prepared["profile"]["tools"]["allow"]
            .as_array_mut()
            .unwrap()
            .push(json!(id));
        if name == "skill.search" {
            prepared["profile"]["tools"]["deny"]
                .as_array_mut()
                .unwrap()
                .push(json!(id));
            continue;
        }
        let mut binding = binding_template.clone();
        binding["descriptor"]["id"] = json!(id);
        binding["modelAlias"] = json!(name.replace('.', "_"));
        prepared["toolSnapshot"]["bindings"]
            .as_array_mut()
            .unwrap()
            .push(binding);
        prepared["toolTurn"]["tools"]
            .as_array_mut()
            .unwrap()
            .push(json!(id));
        let mut tool = tool_template.clone();
        tool["toolId"] = json!(id);
        tool["modelAlias"] = json!(name.replace('.', "_"));
        prepared["request"]["tools"]
            .as_array_mut()
            .unwrap()
            .push(tool);
    }
    prepared["profile"]["tools"]["maxCallsPerTool"]["builtin:skill.read"] = json!(5);
    prepared["profile"]["tools"]["toolDescriptions"]["builtin:skill.read"] =
        json!({ "description": "Legacy read description" });
    prepared["profile"]["tools"]["maxCallsPerTool"]["builtin:agent.list"] = json!(2);
    prepared["profile"]["tools"]["toolDescriptions"]["builtin:agent.list"] =
        json!({ "description": "Legacy discovery description" });
    let legacy_turns = [
        (
            "I consulted the old skill and updated the draft.",
            vec![
                (
                    "legacy_read",
                    "builtin:skill.read",
                    json!({ "name": "style", "path": "references/legacy-read-argument.md" }),
                    "Archived guidance visible to the model.",
                    false,
                ),
                (
                    "legacy_append",
                    "builtin:workspace.write_file",
                    json!({ "path": "output/main.md", "mode": "append", "content": ":already-applied-marker" }),
                    "The old workspace mutation succeeded.",
                    false,
                ),
                (
                    "legacy_script",
                    "builtin:skill.run_script",
                    json!({ "skill": "style", "script": "review", "args": { "input": "script-argument-marker" } }),
                    "The legacy script failed after its earlier write.",
                    true,
                ),
            ],
        ),
        (
            "I found a collaborator and read the draft.",
            vec![
                (
                    "legacy_agents",
                    "builtin:agent.list",
                    json!({ "purpose": "delegate" }),
                    "Previously available: old-editor",
                    false,
                ),
                (
                    "legacy_workspace_read",
                    "builtin:workspace.read_file",
                    json!({ "path": "output/main.md" }),
                    "The previously committed draft.",
                    false,
                ),
            ],
        ),
    ];
    for (text, calls) in legacy_turns {
        let native_output = calls
            .iter()
            .map(|(id, tool, args, _, _)| {
                json!({
                    "type": "function_call", "call_id": id,
                    "name": tool.strip_prefix("builtin:").unwrap().replace('.', "_"),
                    "arguments": serde_json::to_string(args).unwrap(),
                })
            })
            .collect::<Vec<_>>();
        let legacy_native =
            json!({ "responseId": "legacy-native-must-not-replay", "output": native_output });
        let mut parts = vec![
            json!({ "type": "text", "text": text }),
            json!({ "type": "native", "provider": "openai_responses", "value": legacy_native }),
        ];
        parts.extend(calls.iter().map(|(id, tool, args, _, _)| {
            json!({
                "type": "toolCall", "call": { "callId": id, "toolId": tool, "arguments": args }
            })
        }));
        let mut mixed_turn = vec![json!({
            "role": "assistant", "parts": parts,
            "providerMetadata": { "message": { "native": { "openai_responses": legacy_native } } }
        })];
        mixed_turn.extend(calls.iter().map(|(id, tool, _, content, is_error)| {
            json!({
                "role": "tool", "parts": [{ "type": "toolResult", "result": {
                    "callId": id, "toolId": tool, "content": content, "isError": is_error,
                    "errorCode": if *is_error { Some("skill.script_failed") } else { None }
                }}]
            })
        }));
        let messages = prepared["request"]["messages"].as_array_mut().unwrap();
        let insertion = messages.len() - 2;
        messages.splice(insertion..insertion, mixed_turn);
    }
    let legacy_profile = prepared["profile"].clone();
    let legacy_snapshot = prepared["toolSnapshot"].clone();
    checkpoint["state"]["foreground"]["progress"]["session"]["skillReadChars"] = json!(42);

    let events_before = read_agent_events(&fixture.agent_repository, &handle.run_id).await;
    let snapshot_path = events_before
        .iter()
        .find(|event| event.event_type == "context_assembled")
        .unwrap()
        .payload["toolSnapshotPath"]
        .as_str()
        .unwrap();
    let files = fixture
        .agent_repository
        .open_filesystem(&handle.run_id)
        .await
        .unwrap();
    for (path, text) in [
        (
            "output/main.md",
            "hello from real repo:already-applied-marker".to_string(),
        ),
        ("input/resolved_skills.json", bound_skills.to_string()),
        ("input/resolved_profile.json", legacy_profile.to_string()),
        (snapshot_path, legacy_snapshot.to_string()),
    ] {
        files
            .write_text(
                &WorkspacePath::parse(path).unwrap(),
                &text,
                WorkspaceWriteGuard::Unchecked,
            )
            .await
            .unwrap();
    }
    let mut originals = Vec::new();
    for path in [
        "output/main.md",
        "input/prompt_snapshot.json",
        "input/resolved_skills.json",
        "input/resolved_profile.json",
        snapshot_path,
    ] {
        originals.push(
            files
                .read_text(&WorkspacePath::parse(path).unwrap())
                .await
                .unwrap(),
        );
    }
    let legacy_bytes = serde_json::to_vec(&checkpoint).unwrap();
    fixture
        .agent_repository
        .save_run_checkpoint(&handle.run_id, &legacy_bytes)
        .await
        .unwrap();
    let legacy_status = fixture
        .service
        .read_run_checkpoint(AgentReadRunCheckpointDto {
            run_id: handle.run_id.clone(),
        })
        .await
        .unwrap();
    assert_eq!(legacy_status.run.status, AgentRunStatus::Completed);
    assert_eq!(
        fixture
            .agent_repository
            .load_run_checkpoint(&handle.run_id)
            .await
            .unwrap()
            .unwrap(),
        legacy_bytes
    );

    fixture
        .model_gateway
        .responses
        .lock()
        .await
        .push_back(Ok(model_tool_response(vec![
            model_tool_call(
                "revision_read",
                "workspace_read_file",
                json!({ "path": "skills/style/SKILL.md" }),
            ),
            model_tool_call("revision_finish", "workspace_finish", json!({})),
        ])));
    revise_checkpoint(
        &fixture,
        &completed,
        "Review the old guidance.",
        "User-edited draft.",
    )
    .await;
    let revised = wait_for_checkpoint(&fixture, &handle.run_id).await;
    assert_eq!(revised.run.status, AgentRunStatus::Completed);
    let requests = fixture.model_gateway.requests().await;
    let migrated = &requests[2];
    let revision_id = migrated.provider_state["invocationId"].as_str().unwrap();
    assert_ne!(revision_id, ROOT_AGENT_INVOCATION_ID);
    let invocation = fixture
        .agent_repository
        .load_invocation(&handle.run_id, revision_id)
        .await
        .unwrap();
    assert_eq!(invocation.kind, AgentInvocationKind::Revision);
    assert!(
        migrated
            .tools
            .iter()
            .all(|tool| !tool.tool_id.native_name().starts_with("skill.")
                && tool.tool_id.native_name() != "agent.list"
                && tool.tool_id.native_name() != "workspace.shell")
    );
    for message in &original.messages {
        assert!(
            migrated.messages.contains(message),
            "unaffected history changed: {message:?}"
        );
    }
    assert!(
        !serde_json::to_string(migrated)
            .unwrap()
            .contains("legacy-native-must-not-replay")
    );
    assert!(
        migrated
            .messages
            .iter()
            .flat_map(|message| &message.parts)
            .all(|part| match part {
                AgentModelContentPart::ToolCall { call } => !call.call_id.starts_with("legacy_"),
                AgentModelContentPart::ToolResult { result } =>
                    !result.call_id.starts_with("legacy_"),
                _ => true,
            })
    );
    let texts = user_texts(migrated);
    let record = texts
        .iter()
        .find(|text| text.contains("legacy_read"))
        .expect("legacy mixed turn is a user execution record");
    assert!(migrated.messages.iter().any(|message| {
        message.role == AgentModelRole::Assistant && message.parts.iter().any(|part| {
            matches!(part, AgentModelContentPart::Text { text } if text == "I consulted the old skill and updated the draft.")
        })
    }));
    for preserved in [
        "legacy_read",
        "skill.read",
        "legacy-read-argument.md",
        "Archived guidance visible to the model.",
        "legacy_append",
        "workspace.write_file",
        ":already-applied-marker",
        "The old workspace mutation succeeded.",
        "legacy_script",
        "skill.run_script",
        "script-argument-marker",
        "The legacy script failed after its earlier write.",
        "success",
        "error",
    ] {
        assert!(
            record.contains(preserved),
            "execution record lost {preserved}: {record}"
        );
    }
    let notice = texts
        .iter()
        .find(|text| text.contains("skills/style/SKILL.md"))
        .expect("migration explains bound skills");
    assert!(notice.contains("Archived global style"));
    assert!(!notice.contains("Current profile style"));
    assert!(notice.contains("Available agents:"));
    assert!(notice.contains("revision-editor [delegate]: Current revision helper."));
    let agents_record = texts
        .iter()
        .find(|text| text.contains("legacy_agents"))
        .expect("legacy discovery turn is a user execution record");
    for preserved in [
        "agent.list",
        "purpose",
        "delegate",
        "Previously available: old-editor",
        "legacy_workspace_read",
        "workspace.read_file",
        "output/main.md",
        "The previously committed draft.",
    ] {
        assert!(
            agents_record.contains(preserved),
            "discovery record lost {preserved}: {agents_record}"
        );
    }
    let saved = saved_checkpoint(&fixture, &handle.run_id).await;
    assert_eq!(saved["schemaVersion"], 2);
    let prepared = &saved["state"]["foreground"]["prepared"];
    assert_eq!(prepared["effectiveSkills"], bound_skills);
    assert_eq!(
        prepared["profile"]["schemaVersion"],
        tt_domain::models::agent::profile::AGENT_PROFILE_SCHEMA_VERSION
    );
    assert!(
        prepared["profile"]["skills"]
            .get("maxReadCharsPerCall")
            .is_none()
    );
    assert!(
        prepared["profile"]["skills"]
            .get("maxReadCharsPerRun")
            .is_none()
    );
    for value in [
        &prepared["profile"]["tools"],
        &prepared["toolSnapshot"],
        &prepared["toolTurn"],
        &prepared["request"]["tools"],
    ] {
        assert!(
            !value.to_string().contains("builtin:skill.")
                && !value.to_string().contains("builtin:agent.list"),
            "old tool definition survived: {value}"
        );
    }
    let final_request: AgentModelRequest =
        serde_json::from_value(prepared["request"].clone()).unwrap();
    let read = final_request
        .messages
        .iter()
        .flat_map(|message| &message.parts)
        .find_map(|part| match part {
            AgentModelContentPart::ToolResult { result } if result.call_id == "revision_read" => {
                Some(result)
            }
            _ => None,
        })
        .unwrap();
    assert!(!read.is_error, "{}", read.content);
    assert!(read.content.contains("legacy-bound-global"));
    assert!(!read.content.contains("current-profile-shadow"));

    fixture
        .model_gateway
        .responses
        .lock()
        .await
        .push_back(Ok(model_tool_response(vec![model_tool_call(
            "second_finish",
            "workspace_finish",
            json!({}),
        )])));
    revise_checkpoint(
        &fixture,
        &revised,
        "Keep the reviewed text.",
        "User-edited draft.",
    )
    .await;
    assert_eq!(
        wait_for_checkpoint(&fixture, &handle.run_id)
            .await
            .run
            .status,
        AgentRunStatus::Completed
    );
    let requests = fixture.model_gateway.requests().await;
    let second_texts = user_texts(&requests[3]);
    assert_eq!(
        second_texts
            .iter()
            .filter(|text| text.contains("legacy_read"))
            .count(),
        1
    );
    assert_eq!(
        second_texts
            .iter()
            .filter(|text| text.contains("skills/style/SKILL.md"))
            .count(),
        1
    );
    assert!(second_texts.contains(record));
    assert_eq!(
        second_texts
            .iter()
            .filter(|text| text.contains("legacy_agents"))
            .count(),
        1
    );
    assert!(second_texts.contains(agents_record));
    assert!(second_texts.contains(notice));
    for file in originals {
        assert_eq!(
            files.read_text(&file.path).await.unwrap().sha256,
            file.sha256,
            "old workspace file changed: {}",
            file.path.as_str()
        );
    }
    let events_after = read_agent_events(&fixture.agent_repository, &handle.run_id).await;
    assert_eq!(
        serde_json::to_value(&events_after[..events_before.len()]).unwrap(),
        serde_json::to_value(&events_before).unwrap()
    );
    assert!(events_after.iter().all(|event| {
        event.payload["callId"]
            .as_str()
            .is_none_or(|id| !id.starts_with("legacy_"))
    }));
    fs::remove_dir_all(root).await.unwrap();
}

#[tokio::test]
async fn unfinished_legacy_checkpoint_cannot_resume_or_start_a_revision() {
    let root = temp_root("agent-legacy-unfinished");
    let fixture = agent_runtime_fixture_with_results(
        &root,
        vec![Err(ApplicationError::ValidationError(
            "model unavailable".into(),
        ))],
    );
    let profile = resolve_contract_profile(&fixture).await;
    let handle = start_contract_agent_run(
        &fixture,
        &profile,
        AgentRunPresentation::Background,
        "legacy-unfinished",
        Some(false),
    )
    .await;
    let stopped = wait_for_checkpoint(&fixture, &handle.run_id).await;
    assert_eq!(stopped.run.status, AgentRunStatus::Failed);
    let mut checkpoint = saved_checkpoint(&fixture, &handle.run_id).await;
    checkpoint["schemaVersion"] = json!(1);
    let legacy_bytes = serde_json::to_vec(&checkpoint).unwrap();
    fixture
        .agent_repository
        .save_run_checkpoint(&handle.run_id, &legacy_bytes)
        .await
        .unwrap();
    let run = fixture
        .agent_repository
        .load_run(&handle.run_id)
        .await
        .unwrap();
    for revision in [
        None,
        Some(AgentOutputRevisionDto {
            guidance: "Try revision".into(),
            previous_output: "Old body".into(),
        }),
    ] {
        let error = fixture
            .service
            .resume_run(AgentResumeRunDto {
                run_id: run.id.clone(),
                expected_terminal_seq: stopped.terminal_seq,
                chat_ref: run.chat_target().unwrap().chat_ref.clone(),
                stable_chat_id: run.chat_target().unwrap().stable_chat_id.clone(),
                additional_rounds: 0,
                host_presentation: false,
                revision,
            })
            .await
            .expect_err("unfinished legacy execution cannot continue");
        assert!(
            error.to_string().contains("agent.resume_unavailable"),
            "{error}"
        );
    }
    assert_eq!(
        fixture
            .agent_repository
            .load_run_checkpoint(&run.id)
            .await
            .unwrap()
            .unwrap(),
        legacy_bytes
    );
    assert_eq!(fixture.model_gateway.requests().await.len(), 1);
    fs::remove_dir_all(root).await.unwrap();
}

async fn saved_checkpoint(fixture: &AgentRuntimeFixture, run_id: &str) -> Value {
    serde_json::from_slice(
        &fixture
            .agent_repository
            .load_run_checkpoint(run_id)
            .await
            .unwrap()
            .unwrap(),
    )
    .unwrap()
}

fn user_texts(request: &AgentModelRequest) -> Vec<String> {
    request
        .messages
        .iter()
        .filter(|message| message.role == AgentModelRole::User)
        .map(|message| {
            message
                .parts
                .iter()
                .filter_map(|part| match part {
                    AgentModelContentPart::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<String>()
        })
        .collect()
}
