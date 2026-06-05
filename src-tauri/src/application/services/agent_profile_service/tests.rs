use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;
use uuid::Uuid;

use crate::application::services::agent_tools::BuiltinAgentToolRegistry;
use crate::domain::errors::DomainError;
use crate::domain::models::agent::AgentToolSpec;
use crate::domain::models::agent::profile::{
    AgentContextPolicy, AgentModelBinding, AgentModelBindingMode, AgentPresetBindingMode,
    AgentPresetRef, AgentProfileId, ResolvedAgentProfile,
};
use crate::domain::models::preset::{DefaultPreset, Preset, PresetType};
use crate::domain::repositories::preset_repository::PresetRepository;
use crate::infrastructure::repositories::file_agent_profile_repository::FileAgentProfileRepository;

use super::{AgentProfileService, materialize_agent_system_prompt};

#[test]
fn materialized_agent_system_prompt_uses_profile_override_exactly() {
    let profile = test_profile(
        Some("Custom Agent System Prompt.\nKeep this exact."),
        "foreground",
    );

    let prompt =
        materialize_agent_system_prompt(&[tool("workspace.finish", "finish_alias")], &profile);

    assert_eq!(prompt, "Custom Agent System Prompt.\nKeep this exact.");
}

#[test]
fn default_agent_system_prompt_uses_visible_tool_model_names() {
    let profile = test_profile(None, "foreground");
    let tools = vec![
        tool("chat.search", "chat_search_alias"),
        tool("workspace.commit", "workspace_commit_alias"),
        tool("workspace.finish", "workspace_finish_alias"),
    ];

    let prompt = materialize_agent_system_prompt(&tools, &profile);

    assert!(prompt.contains("tool_choice: required"));
    assert!(prompt.contains("- chat_search_alias"));
    assert!(prompt.contains("- workspace_commit_alias"));
    assert!(prompt.contains("- workspace_finish_alias"));
    assert!(!prompt.contains("TauriTavern"));
    assert!(!prompt.contains("runtime"));
    assert!(prompt.contains("use chat_search_alias to find relevant prior messages"));
    assert!(prompt.contains(
        "Before calling workspace_finish_alias, you **must successfully call workspace_commit_alias at least once**"
    ));
    assert!(
        prompt.contains("Do not answer in plain text. Finish by calling workspace_finish_alias.")
    );
    assert!(!prompt.contains("workspace_read_file"));
}

#[test]
fn requires_configuration_model_binding_is_valid_but_not_configured() {
    let binding = AgentModelBinding {
        mode: AgentModelBindingMode::RequiresConfiguration,
        connection_ref: None,
        model_id: None,
    };

    super::validation::validate_model_binding(&binding).expect("requiresConfiguration is saveable");

    let mut profile = test_profile(None, "background");
    profile.model = binding;
    let error = super::ensure_profile_model_configured(&profile)
        .expect_err("requiresConfiguration cannot run");

    assert!(
        error
            .to_string()
            .contains("agent.profile_model_requires_configuration")
    );
}

#[test]
fn requires_configuration_rejects_local_connection_fields() {
    let binding = AgentModelBinding {
        mode: AgentModelBindingMode::RequiresConfiguration,
        connection_ref: Some("local-main".to_string()),
        model_id: Some("secret-model".to_string()),
    };

    let error = super::validation::validate_model_binding(&binding)
        .expect_err("requiresConfiguration must not carry local fields");

    assert!(
        error
            .to_string()
            .contains("agent.profile_model_requires_configuration_extra_fields")
    );
}

#[test]
fn context_policy_allows_empty_initial_history_window() {
    let mut policy = AgentContextPolicy {
        initial_chat_history_messages: 0,
        include_activated_world_info: true,
    };

    super::validation::normalize_context_policy(&mut policy)
        .expect("zero means no initial chat history");

    assert_eq!(policy.initial_chat_history_messages, 0);
}

#[test]
fn context_policy_normalizes_negative_history_window_to_full_history() {
    let mut policy = AgentContextPolicy {
        initial_chat_history_messages: -42,
        include_activated_world_info: true,
    };

    super::validation::normalize_context_policy(&mut policy).expect("negative values normalize");

    assert_eq!(policy.initial_chat_history_messages, -1);
}

#[test]
fn default_agent_system_prompt_reflects_profile_workspace_policy() {
    let mut profile = test_profile(None, "background");
    profile.workspace.visible_roots = vec!["output".to_string()];
    profile.workspace.writable_roots = vec!["output".to_string()];
    let tools = vec![tool("workspace.finish", "workspace_finish_alias")];

    let prompt = materialize_agent_system_prompt(&tools, &profile);

    assert!(prompt.contains("- Visible workspace roots: output/."));
    assert!(prompt.contains("- Writable workspace roots: output/."));
    assert!(prompt.contains(
        "# Background runs may call workspace_finish_alias without committing a chat message."
    ));
    assert!(!prompt.contains("Use persist/"));
    assert!(!prompt.contains("must successfully call"));
}

#[test]
fn default_agent_system_prompt_makes_await_optional_and_decision_driven() {
    let profile = test_profile(None, "background");
    let tools = vec![
        tool("agent.delegate", "agent_delegate_alias"),
        tool("agent.await", "agent_await_alias"),
        tool("workspace.finish", "workspace_finish_alias"),
    ];

    let prompt = materialize_agent_system_prompt(&tools, &profile);

    assert!(prompt.contains("You can continue working after delegating"));
    assert!(prompt.contains(
        "use agent_await_alias when you need a delegated result or status before deciding"
    ));
    assert!(prompt.contains("If delegated task results are provided later"));
    assert!(!prompt.contains("collect delegated task results before finalizing"));
}

#[test]
fn default_agent_system_prompt_does_not_mention_hidden_await_tool() {
    let profile = test_profile(None, "background");
    let tools = vec![
        tool("agent.delegate", "agent_delegate_alias"),
        tool("workspace.finish", "workspace_finish_alias"),
    ];

    let prompt = materialize_agent_system_prompt(&tools, &profile);

    assert!(prompt.contains("Use agent_delegate_alias"));
    assert!(prompt.contains("You can continue working after delegating"));
    assert!(!prompt.contains("agent.await"));
    assert!(!prompt.contains("agent_await"));
}

#[test]
fn default_agent_system_prompt_describes_handoff_from_current_agent_view() {
    let profile = test_profile(None, "foreground");
    let tools = vec![tool("agent.handoff", "agent_handoff_alias")];

    let prompt = materialize_agent_system_prompt(&tools, &profile);

    assert!(prompt.contains(
        "Use agent_handoff_alias when you have finished your part and another Agent should continue"
    ));
    assert!(prompt.contains(
        "After agent_handoff_alias succeeds, your part is done; do not call more tools."
    ));
    assert!(prompt.contains("You cannot finish the run directly with the available tools"));
    assert!(
        prompt.contains("Do not answer in plain text. Continue by calling agent_handoff_alias.")
    );
    assert!(!prompt.contains("This Agent"));
    assert!(!prompt.contains("this Agent"));
    assert!(!prompt.contains("Agent invocation"));
    assert!(!prompt.contains("active run stage"));
}

#[test]
fn delegated_task_system_prompt_uses_shared_workspace_paths() {
    let mut profile = test_profile(None, "background");
    profile.workspace.visible_roots = vec!["output".to_string(), "persist".to_string()];
    profile.workspace.writable_roots = vec!["output".to_string(), "persist".to_string()];
    let tools = vec![
        tool("workspace.write_file", "workspace_write_file"),
        tool("task.return", "task_return"),
    ];

    let prompt = materialize_agent_system_prompt(&tools, &profile);

    assert!(prompt.contains("Delegated task workspace"));
    assert!(prompt.contains("same logical workspace paths"));
    assert!(!prompt.contains("summaries/parent/"));
    assert!(!prompt.contains("summaries/agents/"));
    assert!(prompt.contains("- Visible workspace roots for this task: output/, persist/."));
    assert!(prompt.contains("- Writable workspace roots for this task: output/, persist/."));
    assert!(!prompt.contains("- Visible workspace roots: output/, persist/."));
    assert!(!prompt.contains("- Writable workspace roots: output/, persist/."));
    assert!(!prompt.contains("Never"));
    assert!(prompt.contains("task_return"));
}

#[test]
fn direct_runnable_profiles_require_finish_tool() {
    let run = crate::domain::models::agent::profile::AgentRunPolicy {
        presentation: crate::domain::models::agent::AgentRunPresentation::Background,
        direct_runnable: true,
        model_retry: Default::default(),
    };
    let delegation = crate::domain::models::agent::profile::AgentDelegationPolicy::default();
    let tools = test_tool_policy(&["workspace.write_file"]);

    let error = super::validation::validate_run_policy(&run, &delegation, &tools)
        .expect_err("direct runnable profile without finish should fail");

    assert!(error.to_string().contains("agent.profile_finish_required"));
}

#[test]
fn subagent_only_profiles_do_not_require_finish_tool() {
    let run = crate::domain::models::agent::profile::AgentRunPolicy {
        presentation: crate::domain::models::agent::AgentRunPresentation::Background,
        direct_runnable: false,
        model_retry: Default::default(),
    };
    let delegation = crate::domain::models::agent::profile::AgentDelegationPolicy {
        callable: true,
        allow_as_subagent: true,
        ..Default::default()
    };
    let tools = test_tool_policy(&["workspace.write_file"]);

    super::validation::validate_run_policy(&run, &delegation, &tools)
        .expect("subagent-only profile should not require workspace.finish");
}

#[test]
fn handoff_target_profiles_do_not_require_finish_tool() {
    let run = crate::domain::models::agent::profile::AgentRunPolicy {
        presentation: crate::domain::models::agent::AgentRunPresentation::Foreground,
        direct_runnable: false,
        model_retry: Default::default(),
    };
    let delegation = crate::domain::models::agent::profile::AgentDelegationPolicy {
        callable: true,
        allow_as_handoff_target: true,
        ..Default::default()
    };
    let tools = test_tool_policy(&["workspace.write_file", "agent.handoff"]);

    super::validation::validate_run_policy(&run, &delegation, &tools)
        .expect("handoff target profile should not require workspace.finish");
}

#[test]
fn default_writer_does_not_enable_dice_roll() {
    let profile = super::defaults::default_writer_profile().expect("default writer profile");

    assert!(
        !profile.tools.allow.iter().any(|tool| tool == "dice.roll"),
        "dice.roll must stay opt-in so normal Agent flows do not roll accidentally"
    );
}

#[test]
fn direct_runnable_false_requires_subagent_entrypoint() {
    let run = crate::domain::models::agent::profile::AgentRunPolicy {
        presentation: crate::domain::models::agent::AgentRunPresentation::Background,
        direct_runnable: false,
        model_retry: Default::default(),
    };
    let delegation = crate::domain::models::agent::profile::AgentDelegationPolicy::default();
    let tools = test_tool_policy(&["workspace.write_file"]);

    let error = super::validation::validate_run_policy(&run, &delegation, &tools)
        .expect_err("non-direct profiles need a implemented non-direct entrypoint");

    assert!(
        error
            .to_string()
            .contains("agent.profile_direct_runnable_disabled_requires_delegation_target")
    );
}

#[tokio::test]
async fn profile_preset_retarget_updates_only_matching_refs() {
    let root = temp_profile_root("retarget");
    let profile_service = test_profile_service_with_presets(
        &root,
        TestPresetRepository::with_user_openai("New Writer Preset"),
    );

    save_profile_with_preset_ref(&profile_service, "writer", "openai", "Old Writer Preset").await;
    save_profile_with_preset_ref(&profile_service, "critic", "openai", "Other Preset").await;

    let result = profile_service
        .retarget_preset_refs(
            preset_ref("openai", "Old Writer Preset"),
            preset_ref("openai", "New Writer Preset"),
        )
        .await
        .expect("retarget profile preset refs");

    assert_eq!(result.profile_ids.len(), 1);
    assert_eq!(result.profile_ids[0].as_str(), "writer");
    assert_eq!(
        loaded_preset_name(&profile_service, "writer").await,
        "New Writer Preset"
    );
    assert_eq!(
        loaded_preset_name(&profile_service, "critic").await,
        "Other Preset"
    );

    cleanup_profile_root(root).await;
}

#[tokio::test]
async fn profile_preset_retarget_accepts_default_target_preset() {
    let root = temp_profile_root("retarget-default-target");
    let profile_service = test_profile_service_with_presets(
        &root,
        TestPresetRepository::with_default_openai("Built In Writer Preset"),
    );
    save_profile_with_preset_ref(&profile_service, "writer", "openai", "Old Writer Preset").await;

    profile_service
        .retarget_preset_refs(
            preset_ref("openai", "Old Writer Preset"),
            preset_ref("openai", "Built In Writer Preset"),
        )
        .await
        .expect("default target preset is a valid retarget destination");

    assert_eq!(
        loaded_preset_name(&profile_service, "writer").await,
        "Built In Writer Preset"
    );

    cleanup_profile_root(root).await;
}

#[tokio::test]
async fn profile_preset_retarget_requires_existing_target_preset() {
    let root = temp_profile_root("retarget-missing-target");
    let profile_service = test_profile_service_with_presets(&root, TestPresetRepository::default());
    save_profile_with_preset_ref(&profile_service, "writer", "openai", "Old Writer Preset").await;

    let error = profile_service
        .retarget_preset_refs(
            preset_ref("openai", "Old Writer Preset"),
            preset_ref("openai", "Missing Writer Preset"),
        )
        .await
        .expect_err("missing target preset should fail");

    assert!(
        error
            .to_string()
            .contains("agent.profile_preset_retarget_target_missing")
    );
    assert_eq!(
        loaded_preset_name(&profile_service, "writer").await,
        "Old Writer Preset"
    );

    cleanup_profile_root(root).await;
}

#[tokio::test]
async fn profile_preset_retarget_ignores_unmatched_malformed_preset_refs() {
    let root = temp_profile_root("retarget-unmatched-malformed");
    let profile_service = test_profile_service_with_presets(
        &root,
        TestPresetRepository::with_user_openai("New Writer Preset"),
    );
    save_profile_with_preset_ref(&profile_service, "writer", "openai", "Old Writer Preset").await;

    let mut unrelated = profile_service
        .load_profile("default-writer")
        .await
        .expect("load default profile")
        .expect("default profile exists");
    unrelated.id = AgentProfileId::parse("unrelated").expect("profile id");
    unrelated.preset.mode = AgentPresetBindingMode::Ref;
    unrelated.preset.ref_ = Some(preset_ref("unsupported-api", "Unrelated Preset"));
    unrelated.preset.required = true;
    profile_service
        .profile_repository
        .save_profile(&unrelated)
        .await
        .expect("save malformed unrelated profile");

    let result = profile_service
        .retarget_preset_refs(
            preset_ref("openai", "Old Writer Preset"),
            preset_ref("openai", "New Writer Preset"),
        )
        .await
        .expect("unmatched malformed profile should not block retarget");

    assert_eq!(result.profile_ids.len(), 1);
    assert_eq!(
        loaded_preset_name(&profile_service, "writer").await,
        "New Writer Preset"
    );
    assert_eq!(
        loaded_preset_api_id(&profile_service, "unrelated").await,
        "unsupported-api"
    );

    cleanup_profile_root(root).await;
}

#[tokio::test]
async fn profile_preset_retarget_rejects_same_or_cross_api_refs() {
    let root = temp_profile_root("retarget-invalid-pair");
    let profile_service = test_profile_service_with_presets(&root, TestPresetRepository::default());

    let same_error = profile_service
        .retarget_preset_refs(
            preset_ref("openai", "Writer Preset"),
            preset_ref("openai", "Writer Preset"),
        )
        .await
        .expect_err("same refs are not a rename");
    assert!(
        same_error
            .to_string()
            .contains("agent.profile_preset_retarget_same_ref")
    );

    let cross_api_error = profile_service
        .retarget_preset_refs(
            preset_ref("openai", "Writer Preset"),
            preset_ref("textgenerationwebui", "Writer Preset"),
        )
        .await
        .expect_err("preset refs cannot cross api groups");
    assert!(
        cross_api_error
            .to_string()
            .contains("agent.profile_preset_retarget_api_mismatch")
    );

    cleanup_profile_root(root).await;
}

fn temp_profile_root(label: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "tauritavern-agent-profile-{label}-{}",
        Uuid::new_v4().simple()
    ))
}

async fn cleanup_profile_root(root: std::path::PathBuf) {
    match tokio::fs::remove_dir_all(root).await {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => panic!("cleanup: {error}"),
    }
}

fn test_profile_service_with_presets(
    root: &std::path::Path,
    preset_repository: TestPresetRepository,
) -> Arc<AgentProfileService> {
    let profile_repository = Arc::new(FileAgentProfileRepository::new(root.join("agent-profiles")));
    Arc::new(AgentProfileService::new(
        profile_repository.clone(),
        profile_repository,
        Arc::new(preset_repository),
    ))
}

async fn save_profile_with_preset_ref(
    profile_service: &AgentProfileService,
    profile_id: &str,
    api_id: &str,
    preset_name: &str,
) {
    let mut profile = profile_service
        .load_profile("default-writer")
        .await
        .expect("load default profile")
        .expect("default profile exists");
    profile.id = AgentProfileId::parse(profile_id).expect("profile id");
    profile.preset.mode = AgentPresetBindingMode::Ref;
    profile.preset.ref_ = Some(preset_ref(api_id, preset_name));
    profile.preset.required = true;
    let registry = BuiltinAgentToolRegistry::phase2c();
    profile_service
        .save_profile(profile, registry.specs())
        .await
        .expect("save profile");
}

async fn loaded_preset_name(profile_service: &AgentProfileService, profile_id: &str) -> String {
    profile_service
        .load_profile(profile_id)
        .await
        .expect("load profile")
        .expect("profile exists")
        .preset
        .ref_
        .expect("profile preset ref")
        .name
}

async fn loaded_preset_api_id(profile_service: &AgentProfileService, profile_id: &str) -> String {
    profile_service
        .load_profile(profile_id)
        .await
        .expect("load profile")
        .expect("profile exists")
        .preset
        .ref_
        .expect("profile preset ref")
        .api_id
}

fn preset_ref(api_id: &str, name: &str) -> AgentPresetRef {
    AgentPresetRef {
        api_id: api_id.to_string(),
        name: name.to_string(),
    }
}

#[derive(Default)]
struct TestPresetRepository {
    user_openai: Vec<String>,
    default_openai: Vec<String>,
}

impl TestPresetRepository {
    fn with_user_openai(name: &str) -> Self {
        Self {
            user_openai: vec![name.to_string()],
            default_openai: Vec::new(),
        }
    }

    fn with_default_openai(name: &str) -> Self {
        Self {
            user_openai: Vec::new(),
            default_openai: vec![name.to_string()],
        }
    }
}

#[async_trait]
impl PresetRepository for TestPresetRepository {
    async fn save_preset(&self, _preset: &Preset) -> Result<(), DomainError> {
        Ok(())
    }

    async fn delete_preset(
        &self,
        _name: &str,
        _preset_type: &PresetType,
    ) -> Result<(), DomainError> {
        Ok(())
    }

    async fn preset_exists(
        &self,
        name: &str,
        preset_type: &PresetType,
    ) -> Result<bool, DomainError> {
        Ok(*preset_type == PresetType::OpenAI
            && self.user_openai.iter().any(|preset| preset == name))
    }

    async fn get_preset(
        &self,
        name: &str,
        preset_type: &PresetType,
    ) -> Result<Option<Preset>, DomainError> {
        if self.preset_exists(name, preset_type).await? {
            return Ok(Some(Preset::new(
                name.to_string(),
                preset_type.clone(),
                json!({ "chat_completion_source": "openai" }),
            )));
        }
        Ok(None)
    }

    async fn list_presets(&self, preset_type: &PresetType) -> Result<Vec<String>, DomainError> {
        if *preset_type == PresetType::OpenAI {
            return Ok(self.user_openai.clone());
        }
        Ok(Vec::new())
    }

    async fn get_default_preset(
        &self,
        name: &str,
        preset_type: &PresetType,
    ) -> Result<Option<DefaultPreset>, DomainError> {
        if *preset_type != PresetType::OpenAI
            || !self.default_openai.iter().any(|preset| preset == name)
        {
            return Ok(None);
        }
        Ok(Some(DefaultPreset {
            filename: format!("{name}.json"),
            name: name.to_string(),
            preset_type: PresetType::OpenAI,
            is_default: true,
            data: json!({ "chat_completion_source": "openai" }),
        }))
    }
}

fn tool(name: &str, model_name: &str) -> AgentToolSpec {
    AgentToolSpec {
        name: name.to_string(),
        model_name: model_name.to_string(),
        title: name.to_string(),
        description: String::new(),
        input_schema: json!({}),
        output_schema: None,
        annotations: json!({}),
        source: "test".to_string(),
    }
}

fn test_tool_policy(allow: &[&str]) -> crate::domain::models::agent::profile::AgentToolPolicy {
    crate::domain::models::agent::profile::AgentToolPolicy {
        allow: allow.iter().map(|name| name.to_string()).collect(),
        deny: Vec::new(),
        tool_descriptions: Default::default(),
        max_rounds: 1,
        max_calls_per_run: 1,
        max_calls_per_tool: Default::default(),
    }
}

fn test_profile(agent_system_prompt: Option<&str>, presentation: &str) -> ResolvedAgentProfile {
    let instructions = match agent_system_prompt {
        Some(prompt) => json!({ "agentSystemPrompt": prompt }),
        None => json!({}),
    };

    serde_json::from_value(json!({
        "schemaVersion": 1,
        "kind": "tauritavern.agentProfile",
        "id": "test",
        "displayName": "Test",
        "preset": {
            "mode": "none",
            "required": false
        },
        "model": {
            "mode": "currentPromptSnapshot"
        },
        "run": {
            "presentation": presentation,
            "modelRetry": {
                "maxRetries": 0,
                "intervalMs": 3000
            }
        },
        "context": {
            "initialChatHistoryMessages": -1,
            "includeActivatedWorldInfo": true
        },
        "instructions": instructions,
        "tools": {
            "allow": ["workspace.finish"],
            "deny": [],
            "toolDescriptions": {},
            "maxRounds": 1,
            "maxCallsPerRun": 1,
            "maxCallsPerTool": {}
        },
        "skills": {
            "visible": ["*"],
            "deny": [],
            "maxReadCharsPerCall": 1,
            "maxReadCharsPerRun": 1
        },
        "workspace": {
            "visibleRoots": ["output", "persist"],
            "writableRoots": ["output", "persist"]
        },
        "plan": {
            "mode": "none",
            "beta": true,
            "nodes": []
        },
        "output": {
            "artifacts": [{
                "id": "main",
                "path": "output/main.md",
                "kind": "markdown",
                "target": "message_body",
                "required": true,
                "assemblyOrder": 0
            }],
            "messageBodyArtifactId": "main",
            "messageBodyPath": "output/main.md"
        },
        "sourceTrace": {
            "profileSource": "test"
        }
    }))
    .expect("test profile")
}
