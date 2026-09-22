use super::*;
use tt_application::dto::agent_dto::{
    AgentCancelRunDto, AgentCancelRunResultDto, AgentDeleteSessionDto, AgentPrepareSessionRunDto,
    AgentReadSessionDto, AgentSaveProfileDto, AgentStartSessionRunDto,
};
use tt_domain::models::agent::AgentModelMessage;
use tt_domain::models::agent::profile::{
    AgentModelBindingMode, AgentPresetBindingMode, AgentPresetRef, AgentProfileDefinition,
};
use tt_domain::models::llm_connection::LlmConnectionDefinition;
use tt_ports::repositories::llm_connection_repository::LlmConnectionRepository;

#[tokio::test]
async fn session_keeps_files_and_canonical_history_across_runs_and_restart() {
    let root = temp_root("session-continuity");
    let fixture = agent_runtime_fixture_with_responses(
        &root,
        vec![
            model_tool_response(vec![
                model_tool_call(
                    "work",
                    "workspace_write_file",
                    json!({"path":"work/note.md","content":"saved work"}),
                ),
                model_tool_call(
                    "tmp",
                    "workspace_shell",
                    json!({"command":"printf 'saved tmp' > /tmp/check.txt"}),
                ),
            ]),
            json!({"choices":[{"message":{"role":"assistant","content":"Saved both files.","native":{"example":{"signature":"opaque"}}}}]}),
        ],
    );
    let mut profile = configure_session_profile(&fixture, &root).await;
    fixture
        .service
        .save_session_profile(AgentSaveProfileDto {
            profile: profile.clone(),
        })
        .await
        .unwrap();
    let session = fixture.service.create_session().await.unwrap().session;
    let first_input = prepare_input(&fixture, &session.id, &profile, "Save the files.").await;
    let stale_input = first_input.clone();
    profile.model.model_id = Some("later-model".into());
    fixture
        .service
        .save_session_profile(AgentSaveProfileDto {
            profile: profile.clone(),
        })
        .await
        .unwrap();
    let first = fixture
        .service
        .start_session_run(first_input)
        .await
        .unwrap();
    wait_until_session_idle(&fixture, &first.run_id).await;
    assert_eq!(
        fixture
            .agent_repository
            .load_run(&first.run_id)
            .await
            .unwrap()
            .status,
        AgentRunStatus::Completed
    );
    let requests = fixture.model_gateway.requests().await;
    assert_eq!(requests[0].payload["model"], "test-model");
    assert!(requests[0].tools.iter().all(|tool| !matches!(
        tool.tool_id.native_name(),
        "chat.search" | "worldinfo.read_activated" | "workspace.commit" | "workspace.finish"
    )));
    let history = fixture
        .service
        .read_session(AgentReadSessionDto {
            session_id: session.id.clone(),
            before_seq: None,
            limit: Some(100),
        })
        .await
        .unwrap();
    assert_eq!(history.messages.len(), 5);
    let previous_messages = history
        .messages
        .iter()
        .map(|entry| entry.message.clone())
        .collect::<Vec<_>>();
    assert!(previous_messages.last().unwrap().parts.iter().any(|part| matches!(part,
        AgentModelContentPart::Native { provider, value } if provider == "example" && value["signature"] == "opaque")));
    assert!(
        fixture
            .service
            .start_session_run(stale_input)
            .await
            .unwrap_err()
            .to_string()
            .contains("agent.session_history_changed")
    );
    assert!(!root.join("default-user/chats").exists());
    drop(fixture);

    let fixture = agent_runtime_fixture_with_responses(
        &root,
        vec![
            model_tool_response(vec![
                model_tool_call(
                    "read-work",
                    "workspace_read_file",
                    json!({"path":"work/note.md"}),
                ),
                model_tool_call(
                    "read-tmp",
                    "workspace_read_file",
                    json!({"path":"tmp/check.txt"}),
                ),
            ]),
            json!({"choices":[{"message":{"role":"assistant","content":"Both files survived."}}]}),
        ],
    );
    configure_session_profile(&fixture, &root).await;
    let restored = fixture
        .service
        .read_session(AgentReadSessionDto {
            session_id: session.id.clone(),
            before_seq: None,
            limit: Some(100),
        })
        .await
        .unwrap();
    assert_eq!(restored.messages.len(), 5);
    assert!(restored.messages[0].origin.is_none());
    let tool_round = restored.messages[1].origin.as_ref().unwrap();
    assert_eq!(restored.messages[2].origin.as_ref(), Some(tool_round));
    assert_eq!(restored.messages[3].origin.as_ref(), Some(tool_round));
    let reply_round = restored.messages[4].origin.as_ref().unwrap();
    assert_eq!(tool_round.invocation_id, reply_round.invocation_id);
    assert_ne!(tool_round.round, reply_round.round);
    let latest = fixture
        .service
        .load_session_profile()
        .await
        .unwrap()
        .profile
        .unwrap();
    let second_input = prepare_input(&fixture, &session.id, &latest, "Read the saved files.").await;
    let second = fixture
        .service
        .start_session_run(second_input)
        .await
        .unwrap();
    wait_until_session_idle(&fixture, &second.run_id).await;
    assert_eq!(
        fixture
            .agent_repository
            .load_run(&second.run_id)
            .await
            .unwrap()
            .status,
        AgentRunStatus::Completed
    );
    let requests = fixture.model_gateway.requests().await;
    assert_eq!(requests[0].payload["model"], "later-model");
    assert_eq!(
        &requests[0].messages[1..1 + previous_messages.len()],
        previous_messages.as_slice()
    );
    let results = requests[1]
        .messages
        .iter()
        .flat_map(|message| &message.parts)
        .filter_map(|part| match part {
            AgentModelContentPart::ToolResult { result } if result.call_id.starts_with("read-") => {
                Some(result)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(results.len(), 2);
    assert!(results.iter().all(|result| !result.is_error));
    assert!(
        results
            .iter()
            .any(|result| result.content.contains("saved work"))
    );
    assert!(
        results
            .iter()
            .any(|result| result.content.contains("saved tmp"))
    );
    let files = fixture
        .agent_repository
        .open_filesystem(&second.run_id)
        .await
        .unwrap();
    let old_result_path = format!("tool-results/{}/inv_root", first.run_id);
    assert!(
        files
            .list_files(None, 8, 100)
            .await
            .unwrap()
            .entries
            .iter()
            .any(|file| file.path.as_str().starts_with(&old_result_path))
    );
    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn session_admission_is_exclusive_and_cancellation_keeps_the_user_message() {
    let root = temp_root("session-cancel");
    let fixture = agent_runtime_fixture_with_responses(&root, Vec::new());
    let profile = configure_session_profile(&fixture, &root).await;
    let session = fixture.service.create_session().await.unwrap().session;
    let input = prepare_input(&fixture, &session.id, &profile, "Wait for cancellation.").await;
    fixture
        .model_gateway
        .wait_for_cancel_on_request
        .store(1, Ordering::SeqCst);
    let mut requests = fixture.model_gateway.request_count.subscribe();
    let handle = fixture
        .service
        .start_session_run(input.clone())
        .await
        .unwrap();
    tokio::time::timeout(
        AGENT_CONTRACT_ASYNC_TIMEOUT,
        requests.wait_for(|count| *count == 1),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(
        fixture
            .service
            .start_session_run(input)
            .await
            .unwrap_err()
            .to_string()
            .contains("agent.session_busy")
    );
    let listed = fixture.service.list_sessions().await.unwrap();
    assert_eq!(listed.sessions.len(), 1);
    assert_eq!(listed.active_runs.len(), 1);
    assert_eq!(listed.active_runs[0].run_id, handle.run_id);
    assert!(
        fixture
            .service
            .delete_session(AgentDeleteSessionDto {
                session_id: session.id.clone(),
            })
            .await
            .unwrap_err()
            .to_string()
            .contains("agent.session_busy")
    );
    let cancelled = fixture
        .service
        .cancel_run(AgentCancelRunDto {
            run_id: handle.run_id.clone(),
        })
        .await
        .unwrap();
    assert!(
        matches!(cancelled, AgentCancelRunResultDto::Session(cancelled)
            if cancelled.session_id == session.id && cancelled.run_id == handle.run_id)
    );
    wait_until_session_idle(&fixture, &handle.run_id).await;
    assert_eq!(
        fixture
            .agent_repository
            .load_run(&handle.run_id)
            .await
            .unwrap()
            .status,
        AgentRunStatus::Cancelled
    );
    let history = fixture
        .service
        .read_session(AgentReadSessionDto {
            session_id: session.id.clone(),
            before_seq: None,
            limit: None,
        })
        .await
        .unwrap();
    assert_eq!(history.messages.len(), 1);
    assert_eq!(history.messages[0].message.role, AgentModelRole::User);
    assert!(history.active_run.is_none());
    fixture
        .service
        .delete_session(AgentDeleteSessionDto {
            session_id: session.id.clone(),
        })
        .await
        .unwrap();
    let listed = fixture.service.list_sessions().await.unwrap();
    assert!(listed.sessions.is_empty());
    assert!(listed.active_runs.is_empty());
    assert!(
        fixture
            .agent_repository
            .load_run(&handle.run_id)
            .await
            .is_err()
    );
    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn preset_rename_updates_shared_session_profile_without_listing_it_as_chat_profile() {
    let root = temp_root("session-preset-rename");
    let fixture = agent_runtime_fixture(&root);
    let profile = configure_session_profile(&fixture, &root).await;
    let from = profile.preset.ref_.clone().unwrap();
    let to = AgentPresetRef {
        api_id: from.api_id.clone(),
        name: "renamed-session".into(),
    };
    fixture
        .preset_repository
        .save_preset(&Preset::new(
            to.name.clone(),
            PresetType::OpenAI,
            json!({"openai_max_context":8192,"openai_max_tokens":512}),
        ))
        .await
        .unwrap();

    let unchanged = fixture
        .profile_service
        .retarget_preset_refs(from.clone(), to.clone())
        .await
        .unwrap();
    assert!(!unchanged.session_profile_updated);
    assert!(
        fixture
            .service
            .load_session_profile()
            .await
            .unwrap()
            .profile
            .is_none()
    );

    fixture
        .service
        .save_session_profile(AgentSaveProfileDto { profile })
        .await
        .unwrap();
    let mut writer = fixture
        .profile_service
        .load_profile("default-writer")
        .await
        .unwrap()
        .unwrap();
    writer.id = AgentProfileId::parse("writer").unwrap();
    writer.preset.mode = AgentPresetBindingMode::Ref;
    writer.preset.ref_ = Some(from.clone());
    fixture
        .profile_service
        .save_profile(writer, BuiltinAgentToolRegistry::all().catalog())
        .await
        .unwrap();

    let result = fixture
        .profile_service
        .retarget_preset_refs(from.clone(), to.clone())
        .await
        .unwrap();
    assert_eq!(
        result.profile_ids,
        [AgentProfileId::parse("writer").unwrap()]
    );
    assert!(result.session_profile_updated);
    fixture
        .preset_repository
        .delete_preset(&from.name, &PresetType::OpenAI)
        .await
        .unwrap();
    assert_eq!(
        fixture
            .profile_service
            .load_profile("writer")
            .await
            .unwrap()
            .unwrap()
            .preset
            .ref_,
        Some(to.clone())
    );
    let stored = fixture
        .service
        .load_session_profile()
        .await
        .unwrap()
        .profile
        .unwrap();
    assert_eq!(stored.preset.ref_, Some(to.clone()));
    assert!(
        fixture
            .profile_service
            .list_profiles()
            .await
            .unwrap()
            .profiles
            .iter()
            .all(|candidate| candidate.id != stored.id)
    );
    let session = fixture.service.create_session().await.unwrap().session;
    fixture
        .service
        .prepare_session_run(AgentPrepareSessionRunDto {
            session_id: session.id,
            text: "Use the renamed preset.".into(),
            profile: stored,
        })
        .await
        .unwrap();

    fs::remove_dir_all(root).await.unwrap();
}

pub(super) async fn configure_session_profile(
    fixture: &AgentRuntimeFixture,
    root: &Path,
) -> AgentProfileDefinition {
    fixture
        .preset_repository
        .save_preset(&Preset::new(
            "session".into(),
            PresetType::OpenAI,
            json!({"openai_max_context":8192,"openai_max_tokens":512}),
        ))
        .await
        .unwrap();
    let connection: LlmConnectionDefinition = serde_json::from_value(json!({
        "schemaVersion":1,"kind":"tauritavern.llmConnection","id":"session-model","displayName":"Session model",
        "provider":{"chatCompletionSource":"openai"},"auth":{"secretRef":{"key":"api_key_openai","id":"session-test-secret"}},
    })).unwrap();
    FileLlmConnectionRepository::new(root.join("_tauritavern/llm-connections"))
        .save_connection(&connection)
        .await
        .unwrap();
    let mut profile = fixture
        .profile_service
        .load_profile("default-writer")
        .await
        .unwrap()
        .unwrap();
    profile.id = AgentProfileId::parse("session-agent").unwrap();
    profile.preset.mode = AgentPresetBindingMode::Ref;
    profile.preset.ref_ = Some(AgentPresetRef {
        api_id: "openai".into(),
        name: "session".into(),
    });
    profile.model.mode = AgentModelBindingMode::ConnectionRef;
    profile.model.connection_ref = Some("session-model".into());
    profile.model.model_id = Some("test-model".into());
    profile.tools.allow = [
        "workspace.read_file",
        "workspace.write_file",
        "workspace.shell",
        "workspace.commit",
        "workspace.finish",
        "chat.search",
        "worldinfo.read_activated",
    ]
    .map(|name| format!("builtin:{name}"))
    .into();
    profile.tools.deny.clear();
    profile.tools.tool_descriptions.clear();
    profile.tools.max_calls_per_tool.clear();
    profile.tools.max_rounds = 3;
    profile.skills.visible.clear();
    profile.workspace.visible_roots = vec!["work".into(), "tmp".into()];
    profile.workspace.writable_roots = profile.workspace.visible_roots.clone();
    profile.output.artifacts.clear();
    profile.delegation = AgentDelegationPolicy::default();
    profile
}

pub(super) async fn prepare_input(
    fixture: &AgentRuntimeFixture,
    session_id: &str,
    profile: &AgentProfileDefinition,
    text: &str,
) -> AgentStartSessionRunDto {
    let prepared = fixture
        .service
        .prepare_session_run(AgentPrepareSessionRunDto {
            session_id: session_id.into(),
            text: text.into(),
            profile: profile.clone(),
        })
        .await
        .unwrap();
    let request = prepared.assembly.request.unwrap();
    let mut frozen_run_input_snapshot = request.frozen_run_input_snapshot;
    let mut messages = vec![AgentModelMessage {
        role: AgentModelRole::System,
        parts: vec![AgentModelContentPart::Text {
            text: request.agent_system_prompt,
        }],
        provider_metadata: json!({"promptComponent":"agentSystemPrompt"}),
    }];
    messages.extend(
        serde_json::from_value::<Vec<AgentModelMessage>>(
            frozen_run_input_snapshot["promptInputs"]
                .as_object_mut()
                .unwrap()
                .remove("agentMessages")
                .unwrap(),
        )
        .unwrap(),
    );
    AgentStartSessionRunDto {
        session_id: session_id.into(),
        text: text.into(),
        profile: profile.clone(),
        expected_history_seq: prepared.expected_history_seq,
        prompt_snapshot: json!({"contextPolicy":profile.context,"messages":messages,"generationParameters":{"chat_completion_source":"openai","model":"test-model","max_tokens":512}}),
        frozen_run_input_snapshot,
        generation_intent: None,
        stream: Some(false),
    }
}

pub(super) async fn wait_until_session_idle(fixture: &AgentRuntimeFixture, run_id: &str) {
    if let Some(mut projection) = fixture
        .service
        .subscribe_live_projection(run_id)
        .await
        .unwrap()
    {
        tokio::time::timeout(AGENT_CONTRACT_ASYNC_TIMEOUT, async {
            while projection.changed().await.is_ok() {}
        })
        .await
        .expect("Session did not finish");
    }
}
