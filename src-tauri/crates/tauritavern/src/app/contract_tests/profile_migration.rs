use super::*;
use tt_application::dto::agent_dto::AgentSaveProfileDto;
use tt_domain::models::agent::profile::{
    AgentModelBindingMode, AgentPresetBindingMode, AgentPresetRef,
};

#[tokio::test]
async fn legacy_profiles_load_import_and_persist_without_changing_remaining_choices() {
    let root = temp_root("profile-migration");
    let repository = Arc::new(FileAgentProfileRepository::new(root.clone()));
    let session_repository = Arc::new(FileAgentRepository::new(root.join("session-workspace")));
    let preset_repository = Arc::new(TestPresetRepository::default());
    preset_repository
        .save_preset(&Preset::new(
            "Session".into(),
            PresetType::OpenAI,
            json!({}),
        ))
        .await
        .unwrap();
    let service = AgentProfileService::new(
        repository.clone(),
        repository.clone(),
        preset_repository,
        session_repository,
    );
    let registry = BuiltinAgentToolRegistry::all();
    fs::create_dir_all(root.join("profiles")).await.unwrap();

    for version in [1, 2, 3, 4] {
        let id = format!("legacy-{version}");
        let mut profile = service
            .load_profile(DEFAULT_AGENT_PROFILE_ID)
            .await
            .unwrap()
            .unwrap();
        profile.id = AgentProfileId::parse(&id).unwrap();
        profile.instructions.agent_system_prompt = Some("  My exact prompt.\nKeep it.  ".into());
        profile.delegation.can_delegate = false;
        profile.tools.allow = [
            "workspace.read_file",
            "workspace.write_file",
            "workspace.commit",
            "workspace.finish",
        ]
        .map(|name| format!("builtin:{name}"))
        .to_vec();
        profile.tools.deny = vec!["builtin:workspace.shell".into()];
        profile.tools.external_result_inline_char_limit = 12_345;
        profile.tools.tool_descriptions.insert(
            "builtin:workspace.read_file".into(),
            serde_json::from_value(json!({
                "description": "  Read the supplied file.  ",
                "properties": {"path": "  Keep this parameter guidance.  "}
            }))
            .unwrap(),
        );
        profile.skills.visible = vec!["lore".into()];
        profile.skills.deny = vec!["private".into()];
        let expected = serde_json::to_value(&profile).unwrap();
        let mut legacy = expected.clone();
        legacy["preset"]["mode"] = json!("none");
        legacy["tools"]["mcpResultInlineCharLimit"] = legacy["tools"]
            .as_object_mut()
            .unwrap()
            .remove("externalResultInlineCharLimit")
            .unwrap();
        legacy["schemaVersion"] = json!(version);
        legacy["tools"]["maxRounds"] = json!(80);
        legacy["tools"]["maxCallsPerRun"] = json!(80);
        legacy["tools"]["maxCallsPerTool"] = json!({"builtin:workspace.read_file": 7});
        if version < 4 {
            legacy["skills"]["maxReadCharsPerCall"] = json!(20_000);
            legacy["skills"]["maxReadCharsPerRun"] = json!(80_000);
            legacy["tools"]["allow"].as_array_mut().unwrap().extend([
                json!("builtin:skill.read"),
                json!("builtin:skill.run_script"),
                json!("builtin:agent.list"),
            ]);
            legacy["tools"]["deny"]
                .as_array_mut()
                .unwrap()
                .extend([json!("builtin:skill.search"), json!("builtin:agent.list")]);
            legacy["tools"]["maxCallsPerTool"]["builtin:skill.list"] = json!(1);
            legacy["tools"]["maxCallsPerTool"]["builtin:agent.list"] = json!(2);
            legacy["tools"]["toolDescriptions"]["builtin:skill.read"] =
                json!({"description": "Legacy read"});
            legacy["tools"]["toolDescriptions"]["builtin:agent.list"] =
                json!({"description": "Legacy discovery"});
        }
        if version < 3 {
            for key in ["allow", "deny"] {
                for value in legacy["tools"][key].as_array_mut().unwrap() {
                    *value = json!(value.as_str().unwrap().strip_prefix("builtin:").unwrap());
                }
            }
            let descriptions = legacy["tools"]["toolDescriptions"].as_object_mut().unwrap();
            *descriptions = std::mem::take(descriptions)
                .into_iter()
                .map(|(id, value)| (id.strip_prefix("builtin:").unwrap().to_string(), value))
                .collect();
        }
        let path = root.join("profiles").join(format!("{id}.json"));
        fs::write(&path, serde_json::to_vec(&legacy).unwrap())
            .await
            .unwrap();
        let scan = repository.scan_profiles().await.unwrap();
        assert!(scan.issues.is_empty());
        assert!(
            scan.profiles
                .iter()
                .any(|profile| profile.id.as_str() == id)
        );

        let loaded = service.load_profile(&id).await.unwrap().unwrap();
        assert_eq!(serde_json::to_value(&loaded).unwrap(), expected);
        let saved: Value = serde_json::from_slice(&fs::read(&path).await.unwrap()).unwrap();
        assert_eq!(saved, expected);

        // Direct API saves and portable imports deserialize the same DTO.
        let imported: AgentSaveProfileDto =
            serde_json::from_value(json!({"profile": legacy})).unwrap();
        service
            .save_profile(imported.profile, registry.catalog())
            .await
            .unwrap();
        let saved: Value = serde_json::from_slice(&fs::read(&path).await.unwrap()).unwrap();
        assert_eq!(saved, expected);

        if version == 4 {
            let mut session_profile = loaded.clone();
            session_profile.preset.mode = AgentPresetBindingMode::Ref;
            session_profile.preset.ref_ = Some(AgentPresetRef {
                api_id: "openai".into(),
                name: "Session".into(),
            });
            session_profile.model.mode = AgentModelBindingMode::ConnectionRef;
            session_profile.model.connection_ref = Some("session-model".into());
            session_profile.model.model_id = Some("test-model".into());
            session_profile.context.initial_chat_history_messages = -1;
            session_profile.workspace.visible_roots = vec!["work".into(), "tmp".into()];
            session_profile.workspace.writable_roots =
                session_profile.workspace.visible_roots.clone();
            session_profile.output.artifacts.clear();
            let expected_session = serde_json::to_value(&session_profile).unwrap();
            let mut legacy_session = expected_session.clone();
            legacy_session["schemaVersion"] = json!(4);
            legacy_session["tools"]["maxRounds"] = json!(80);
            legacy_session["tools"]["maxCallsPerRun"] = json!(80);
            legacy_session["tools"]["maxCallsPerTool"] = json!({"builtin:workspace.read_file": 7});
            let session_path = root.join("session-workspace/sessions/profile.json");
            fs::create_dir_all(session_path.parent().unwrap())
                .await
                .unwrap();
            fs::write(&session_path, serde_json::to_vec(&legacy_session).unwrap())
                .await
                .unwrap();
            let loaded_session = service.load_session_profile().await.unwrap().unwrap();
            assert_eq!(
                serde_json::to_value(loaded_session).unwrap(),
                expected_session
            );
            let saved_session: Value =
                serde_json::from_slice(&fs::read(&session_path).await.unwrap()).unwrap();
            assert_eq!(saved_session, expected_session);

            let imported_session: AgentSaveProfileDto =
                serde_json::from_value(json!({"profile": legacy_session})).unwrap();
            service
                .save_session_profile(imported_session.profile, registry.catalog())
                .await
                .unwrap();
            let saved_session: Value =
                serde_json::from_slice(&fs::read(&session_path).await.unwrap()).unwrap();
            assert_eq!(saved_session, expected_session);
        }

        let mut invalid_current = expected;
        invalid_current["skills"]["maxReadCharsPerCall"] = json!(20_000);
        assert!(
            serde_json::from_value::<AgentSaveProfileDto>(json!({"profile": invalid_current}))
                .is_err()
        );
    }
    fs::remove_dir_all(root).await.unwrap();
}
