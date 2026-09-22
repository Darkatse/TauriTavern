use super::*;
use tt_application::dto::agent_dto::AgentSaveProfileDto;

#[tokio::test]
async fn legacy_profiles_load_import_and_persist_without_changing_remaining_choices() {
    let root = temp_root("profile-migration");
    let repository = Arc::new(FileAgentProfileRepository::new(root.clone()));
    let service = AgentProfileService::new(
        repository.clone(),
        repository.clone(),
        Arc::new(TestPresetRepository::default()),
        Arc::new(FileAgentRepository::new(root.join("session-workspace"))),
    );
    let registry = BuiltinAgentToolRegistry::all();
    fs::create_dir_all(root.join("profiles")).await.unwrap();

    for version in [1, 2, 3] {
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
        profile
            .tools
            .max_calls_per_tool
            .insert("builtin:workspace.read_file".into(), 7);
        let expected = serde_json::to_value(&profile).unwrap();
        let mut legacy = expected.clone();
        legacy["schemaVersion"] = json!(version);
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
        if version < 3 {
            for key in ["allow", "deny"] {
                for value in legacy["tools"][key].as_array_mut().unwrap() {
                    *value = json!(value.as_str().unwrap().strip_prefix("builtin:").unwrap());
                }
            }
            for key in ["maxCallsPerTool", "toolDescriptions"] {
                let fields = legacy["tools"][key].as_object_mut().unwrap();
                *fields = std::mem::take(fields)
                    .into_iter()
                    .map(|(id, value)| (id.strip_prefix("builtin:").unwrap().to_string(), value))
                    .collect();
            }
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

        let mut invalid_current = expected;
        invalid_current["skills"]["maxReadCharsPerCall"] = json!(20_000);
        assert!(
            serde_json::from_value::<AgentSaveProfileDto>(json!({"profile": invalid_current}))
                .is_err()
        );
    }
    fs::remove_dir_all(root).await.unwrap();
}
