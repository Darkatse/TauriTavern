use std::collections::{HashSet, VecDeque};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use serde_json::{Value, json};
use tokio::sync::{Mutex, watch};
use uuid::Uuid;

use super::AgentRuntimeService;
use super::artifacts::build_agent_manifest;
use super::commit_ledger::RunCommitLedger;
use super::delegation::workspace_policy::InvocationWorkspaceRepository;
use super::skill_scope::{resolve_run_skill_scope_refs, skill_scope_order_for_profile};
use crate::application::dto::agent_dto::{
    AgentPromptAssemblyScopeDto, AgentReadEventsDto, AgentReadModelTurnDto,
    AgentReadPromptAssemblyRequestDto, AgentResolveChatCommitDto,
    AgentResolvePersistentStateMetadataUpdateDto, AgentResolvePromptAssemblyDto,
    AgentSkillScopeRefsDto, AgentStartRunDto, AgentStartRunOptionsDto,
};
use crate::application::dto::chat_completion_dto::ChatCompletionGenerateRequestDto;
use crate::application::errors::ApplicationError;
use crate::application::services::agent_identity::workspace_id_for_stable_chat_id;
use crate::application::services::agent_model_gateway::{
    AgentModelExchange, AgentModelGateway, decode_chat_completion_response,
};
use crate::application::services::agent_profile_service::{
    AgentProfileResolveInput, AgentProfileService,
};
use crate::application::services::agent_tools::{
    AgentToolDispatcher, AgentToolEffect, AgentToolSession, BuiltinAgentToolRegistry,
};
use crate::application::services::llm_connection_service::LlmConnectionService;
use crate::application::services::prompt_assembly_service::PromptAssemblyService;
use crate::application::services::skill_service::SkillService;
use crate::domain::errors::DomainError;
use crate::domain::models::agent::profile::{
    AgentDelegationPolicy, AgentModelBindingMode, AgentPresetBindingMode, AgentPresetRef,
    AgentProfileId, ResolvedAgentProfile,
};
use crate::domain::models::agent::{
    AgentChatRef, AgentDelegationContinuation, AgentInvocationExitPolicy, AgentInvocationStatus,
    AgentModelContentPart, AgentModelRequest, AgentModelRole, AgentRun, AgentRunEventLevel,
    AgentRunPresentation, AgentRunSkillScopeRefs, AgentRunStatus, AgentTaskStatus, AgentToolCall,
    AgentToolResult, WorkspaceFileWriteMode, WorkspaceManifest, WorkspacePath,
    WorkspacePersistentChangeSet,
};
use crate::domain::models::preset::{DefaultPreset, Preset, PresetType};
use crate::domain::models::skill::{
    SkillImportInput, SkillInlineFile, SkillInstallRequest, SkillScope,
};
use crate::domain::repositories::agent_invocation_repository::AgentInvocationRepository;
use crate::domain::repositories::agent_run_repository::{
    AgentRunEventReadQuery, AgentRunRepository, event_belongs_to_invocation,
};
use crate::domain::repositories::chat_repository::ChatRepository;
use crate::domain::repositories::preset_repository::PresetRepository;
use crate::domain::repositories::skill_repository::SkillRepository;
use crate::domain::repositories::workspace_repository::{
    WorkspaceAppendResult, WorkspaceFile, WorkspaceFileList, WorkspaceRepository,
    WorkspaceWriteGuard,
};
use crate::infrastructure::repositories::chat_directory_identity::new_shared_chat_alias_store_for_user_dir;
use crate::infrastructure::repositories::file_agent_profile_repository::FileAgentProfileRepository;
use crate::infrastructure::repositories::file_agent_repository::FileAgentRepository;
use crate::infrastructure::repositories::file_chat_repository::FileChatRepository;
use crate::infrastructure::repositories::file_llm_connection_repository::FileLlmConnectionRepository;
use crate::infrastructure::repositories::file_skill_repository::FileSkillRepository;

#[test]
fn workspace_id_uses_stable_chat_id_not_character_chat_file_name() {
    let first = AgentChatRef::Character {
        character_id: "Seraphina".to_string(),
        file_name: "old-chat".to_string(),
    };
    let second = AgentChatRef::Character {
        character_id: "Seraphina".to_string(),
        file_name: "renamed-chat".to_string(),
    };

    let first_id = workspace_id_for_stable_chat_id(&first, "stable-chat").unwrap();
    let second_id = workspace_id_for_stable_chat_id(&second, "stable-chat").unwrap();

    assert_eq!(first_id, second_id);
}

#[tokio::test]
async fn resolves_agent_system_prompt_through_runtime_boundary() {
    let root = std::env::temp_dir().join(format!(
        "tauritavern-agent-system-prompt-{}",
        Uuid::new_v4().simple()
    ));
    let repository = Arc::new(FileAgentRepository::new(root.clone()));
    let service = Arc::new(AgentRuntimeService::new(
        repository.clone(),
        repository.clone(),
        repository.clone(),
        repository.clone(),
        test_chat_repository(&root),
        test_chat_repository(&root),
        test_skill_service(&root),
        Arc::new(MockAgentModelGateway::new(vec![])),
        test_profile_service(&root),
        test_llm_connection_service(&root),
    ));

    let prompt = service
        .resolve_agent_system_prompt(None)
        .await
        .expect("resolve prompt");

    assert!(prompt.contains("# Agent Mode is active."));
    assert!(!prompt.contains("TauriTavern"));
    assert!(prompt.contains("tool_choice: required"));
    assert!(prompt.contains("workspace_commit"));
    assert!(prompt.contains("workspace_finish"));
}

#[tokio::test]
async fn system_prompt_preview_allows_dangling_required_preset_profile() {
    let root = std::env::temp_dir().join(format!(
        "tauritavern-agent-system-prompt-dangling-preset-{}",
        Uuid::new_v4().simple()
    ));
    let repository = Arc::new(FileAgentRepository::new(root.clone()));
    let profile_service = test_profile_service(&root);
    let service = Arc::new(AgentRuntimeService::new(
        repository.clone(),
        repository.clone(),
        repository.clone(),
        repository.clone(),
        test_chat_repository(&root),
        test_chat_repository(&root),
        test_skill_service(&root),
        Arc::new(MockAgentModelGateway::new(vec![])),
        profile_service.clone(),
        test_llm_connection_service(&root),
    ));

    let mut profile = profile_service
        .load_profile("default-writer")
        .await
        .expect("load default profile")
        .expect("default profile exists");
    profile.id = AgentProfileId::parse("dangling-writer").expect("profile id");
    profile.display_name = "Dangling Writer".to_string();
    profile.preset.mode = AgentPresetBindingMode::Ref;
    profile.preset.ref_ = Some(AgentPresetRef {
        api_id: "openai".to_string(),
        name: "Missing Writer Preset".to_string(),
    });
    profile.preset.required = true;
    profile_service
        .save_profile(profile, service.tool_specs())
        .await
        .expect("dangling preset profile remains editable");

    let strict_error = profile_service
        .resolve_profile(AgentProfileResolveInput {
            profile_id: Some("dangling-writer"),
            known_tools: service.tool_specs(),
        })
        .await
        .expect_err("strict run resolution still requires preset");
    assert!(
        strict_error
            .to_string()
            .contains("agent.profile_preset_missing")
    );

    let prompt = service
        .resolve_agent_system_prompt(Some("dangling-writer"))
        .await
        .expect("system prompt preview should not require preset file");

    assert!(prompt.contains("# Agent Mode is active."));
    assert!(prompt.contains("workspace_finish"));
    tokio::fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn agent_list_returns_callable_profiles_allowed_by_delegation_policy() {
    let root = std::env::temp_dir().join(format!(
        "tauritavern-agent-list-{}",
        Uuid::new_v4().simple()
    ));
    let repository = Arc::new(FileAgentRepository::new(root.clone()));
    let profile_service = test_profile_service(&root);
    let service = Arc::new(AgentRuntimeService::new(
        repository.clone(),
        repository.clone(),
        repository.clone(),
        repository.clone(),
        test_chat_repository(&root),
        test_chat_repository(&root),
        test_skill_service(&root),
        Arc::new(MockAgentModelGateway::new(vec![])),
        profile_service.clone(),
        test_llm_connection_service(&root),
    ));

    let mut callable = profile_service
        .load_profile("default-writer")
        .await
        .expect("load default profile")
        .expect("default profile exists");
    callable.id = AgentProfileId::parse("scene-editor").expect("profile id");
    callable.display_name = "Scene Editor".to_string();
    callable.description = Some("Edits a draft scene for continuity.".to_string());
    callable.tools.allow.retain(|name| {
        !matches!(
            name.as_str(),
            "agent.list" | "agent.delegate" | "agent.await"
        )
    });
    callable.delegation = AgentDelegationPolicy {
        callable: true,
        allow_as_subagent: true,
        allowed_callers: vec!["default-writer".to_string()],
        description_for_agents: Some("Continuity editor for scene drafts.".to_string()),
        ..Default::default()
    };
    profile_service
        .save_profile(callable, service.tool_specs())
        .await
        .expect("save callable profile");
    let mut unconfigured = profile_service
        .load_profile("default-writer")
        .await
        .expect("load default profile")
        .expect("default profile exists");
    unconfigured.id = AgentProfileId::parse("unconfigured-editor").expect("profile id");
    unconfigured.display_name = "Unconfigured Editor".to_string();
    unconfigured.model.mode = AgentModelBindingMode::RequiresConfiguration;
    unconfigured.model.connection_ref = None;
    unconfigured.model.model_id = None;
    unconfigured.tools.allow.retain(|name| {
        !matches!(
            name.as_str(),
            "agent.list" | "agent.delegate" | "agent.await"
        )
    });
    unconfigured.delegation = AgentDelegationPolicy {
        callable: true,
        allow_as_subagent: true,
        allowed_callers: vec!["default-writer".to_string()],
        description_for_agents: Some("Would edit scenes after model setup.".to_string()),
        ..Default::default()
    };
    profile_service
        .save_profile(unconfigured, service.tool_specs())
        .await
        .expect("save unconfigured callable profile");
    let mut dangling_preset = profile_service
        .load_profile("default-writer")
        .await
        .expect("load default profile")
        .expect("default profile exists");
    dangling_preset.id = AgentProfileId::parse("dangling-preset-editor").expect("profile id");
    dangling_preset.display_name = "Dangling Preset Editor".to_string();
    dangling_preset.preset.mode = AgentPresetBindingMode::Ref;
    dangling_preset.preset.ref_ = Some(AgentPresetRef {
        api_id: "openai".to_string(),
        name: "Missing Editor Preset".to_string(),
    });
    dangling_preset.preset.required = true;
    dangling_preset.tools.allow.retain(|name| {
        !matches!(
            name.as_str(),
            "agent.list" | "agent.delegate" | "agent.await"
        )
    });
    dangling_preset.delegation = AgentDelegationPolicy {
        callable: true,
        allow_as_subagent: true,
        allowed_callers: vec!["default-writer".to_string()],
        description_for_agents: Some("Would edit scenes if its preset existed.".to_string()),
        ..Default::default()
    };
    profile_service
        .save_profile(dangling_preset, service.tool_specs())
        .await
        .expect("save dangling preset callable profile");

    let mut profile = profile_service
        .resolve_profile(AgentProfileResolveInput {
            profile_id: None,
            known_tools: service.tool_specs(),
        })
        .await
        .expect("resolve default profile");
    profile.run.presentation = AgentRunPresentation::Background;
    let run = AgentRun {
        id: "run_agent_list_test".to_string(),
        workspace_id: "chat_agent_list_test".to_string(),
        stable_chat_id: "stable_agent_list_test".to_string(),
        chat_ref: AgentChatRef::Character {
            character_id: "Seraphina".to_string(),
            file_name: "Seraphina.png".to_string(),
        },
        generation_type: "normal".to_string(),
        profile_id: Some(profile.id.as_str().to_string()),
        skill_scope_refs: Default::default(),
        persist_base_state_id: None,
        input_message_count: None,
        presentation: AgentRunPresentation::Background,
        status: AgentRunStatus::Created,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    repository.create_run(&run).await.expect("create run");
    repository
        .initialize_run(
            &run,
            &build_agent_manifest(&run, &profile),
            &json!({"messages": []}),
            &profile,
        )
        .await
        .expect("initialize run");

    let call = AgentToolCall {
        id: "call_agent_list".to_string(),
        name: "agent.list".to_string(),
        arguments: json!({ "purpose": "delegate" }),
        provider_metadata: Value::Null,
    };
    let (_cancel_sender, mut cancel) = watch::channel(false);
    let mut session = AgentToolSession::default();
    let mut commit_ledger = RunCommitLedger::default();
    let outcome = service
        .dispatch_tool_call(
            &run.id,
            "inv_root",
            AgentInvocationExitPolicy::RunFinishAllowed,
            1,
            &call,
            &mut session,
            &profile,
            0,
            &mut commit_ledger,
            &mut cancel,
        )
        .await
        .expect("dispatch agent.list");

    assert!(!outcome.result.is_error);
    assert_eq!(
        outcome.result.structured["agents"][0]["profileId"],
        "scene-editor"
    );
    assert_eq!(
        outcome.result.structured["agents"][0]["operations"],
        json!(["delegate"])
    );
    assert_eq!(
        outcome.result.structured["agents"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(
        outcome
            .result
            .content
            .contains("This is a read-only list; no Agent was started.")
    );

    tokio::fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn skill_scope_order_uses_invocation_preset_profile_and_run_character() {
    let root = std::env::temp_dir().join(format!(
        "tauritavern-agent-skill-scope-order-{}",
        Uuid::new_v4().simple()
    ));
    let mut profile = test_resolved_profile(&root).await;
    profile.id = AgentProfileId::parse("scene-critic").expect("profile id");
    profile.preset.mode = AgentPresetBindingMode::Ref;
    profile.preset.ref_ = Some(AgentPresetRef {
        api_id: "openai".to_string(),
        name: "Child Preset".to_string(),
    });
    profile.skills.visible = vec!["scope-marker".to_string()];

    let run_refs = AgentRunSkillScopeRefs {
        preset: Some(AgentPresetRef {
            api_id: "openai".to_string(),
            name: "Root Preset".to_string(),
        }),
        character_id: Some("alice".to_string()),
    };
    let scopes = skill_scope_order_for_profile(&profile, &run_refs).expect("resolve scopes");
    assert_eq!(
        scopes,
        vec![
            SkillScope::Global,
            SkillScope::Preset {
                api_id: "openai".to_string(),
                name: "Child Preset".to_string(),
            },
            SkillScope::Profile {
                profile_id: "scene-critic".to_string(),
            },
            SkillScope::Character {
                character_id: "alice".to_string(),
            },
        ]
    );

    let skill_repository = Arc::new(FileSkillRepository::new(root.join("skills")));
    for (scope, label) in [
        (SkillScope::Global, "global"),
        (
            SkillScope::Preset {
                api_id: "openai".to_string(),
                name: "Child Preset".to_string(),
            },
            "preset",
        ),
        (
            SkillScope::Profile {
                profile_id: "scene-critic".to_string(),
            },
            "profile",
        ),
        (
            SkillScope::Character {
                character_id: "alice".to_string(),
            },
            "character",
        ),
    ] {
        skill_repository
            .install_import(SkillInstallRequest {
                target_scope: scope,
                input: SkillImportInput::InlineFiles {
                    files: vec![SkillInlineFile {
                        path: "SKILL.md".to_string(),
                        encoding: "utf8".to_string(),
                        content: format!(
                            "---\nname: scope-marker\ndescription: {label} scoped skill.\n---\n\n# {label}\n"
                        ),
                        media_type: None,
                        size_bytes: None,
                        sha256: None,
                    }],
                    source: json!({ "kind": "test" }),
                },
                conflict_strategy: None,
            })
            .await
            .expect("install scoped skill");
    }

    let effective = SkillService::new(skill_repository)
        .resolve_effective_skills(&scopes, &profile.skills)
        .await
        .expect("resolve effective skills");
    assert_eq!(effective.len(), 1);
    assert_eq!(
        effective[0].scope,
        SkillScope::Character {
            character_id: "alice".to_string(),
        }
    );

    let mismatch = resolve_run_skill_scope_refs(
        &AgentStartRunDto {
            chat_ref: AgentChatRef::Character {
                character_id: "alice".to_string(),
                file_name: "alice.png".to_string(),
            },
            stable_chat_id: "stable-alice".to_string(),
            generation_type: "normal".to_string(),
            profile_id: None,
            persist_base_state_id: None,
            prompt_snapshot: None,
            frozen_run_input_snapshot: None,
            generation_intent: None,
            skill_scope_refs: AgentSkillScopeRefsDto {
                preset: Some(AgentPresetRef {
                    api_id: "openai".to_string(),
                    name: "Root Preset".to_string(),
                }),
                character_id: Some("alice".to_string()),
            },
            options: AgentStartRunOptionsDto::default(),
        },
        &profile,
    )
    .expect_err("mismatched explicit preset should fail fast");
    assert!(
        mismatch
            .to_string()
            .contains("agent.skill_scope_preset_mismatch")
    );

    tokio::fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn run_skill_scope_refs_capture_explicit_group_character_for_child_invocations() {
    let root = std::env::temp_dir().join(format!(
        "tauritavern-agent-skill-scope-refs-{}",
        Uuid::new_v4().simple()
    ));
    let mut profile = test_resolved_profile(&root).await;
    profile.preset.mode = AgentPresetBindingMode::CurrentPromptSnapshot;

    let dto = AgentStartRunDto {
        chat_ref: AgentChatRef::Group {
            chat_id: "group-chat".to_string(),
        },
        stable_chat_id: "stable-group-chat".to_string(),
        generation_type: "normal".to_string(),
        profile_id: None,
        persist_base_state_id: None,
        prompt_snapshot: None,
        frozen_run_input_snapshot: None,
        generation_intent: None,
        skill_scope_refs: AgentSkillScopeRefsDto {
            preset: Some(AgentPresetRef {
                api_id: "openai".to_string(),
                name: "Current Preset".to_string(),
            }),
            character_id: Some("alice".to_string()),
        },
        options: AgentStartRunOptionsDto::default(),
    };

    let refs = resolve_run_skill_scope_refs(&dto, &profile).expect("resolve run refs");
    assert_eq!(
        refs,
        AgentRunSkillScopeRefs {
            preset: Some(AgentPresetRef {
                api_id: "openai".to_string(),
                name: "Current Preset".to_string(),
            }),
            character_id: Some("alice".to_string()),
        }
    );
    let scopes = skill_scope_order_for_profile(&profile, &refs).expect("resolve scopes");
    assert!(scopes.contains(&SkillScope::Preset {
        api_id: "openai".to_string(),
        name: "Current Preset".to_string(),
    }));
    assert!(scopes.contains(&SkillScope::Character {
        character_id: "alice".to_string(),
    }));

    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn skill_scope_order_ignores_ambient_preset_when_profile_preset_mode_is_none() {
    let root = std::env::temp_dir().join(format!(
        "tauritavern-agent-skill-scope-none-{}",
        Uuid::new_v4().simple()
    ));
    let mut profile = test_resolved_profile(&root).await;
    profile.preset.mode = AgentPresetBindingMode::None;
    profile.preset.ref_ = None;

    let scopes = skill_scope_order_for_profile(
        &profile,
        &AgentRunSkillScopeRefs {
            preset: Some(AgentPresetRef {
                api_id: "openai".to_string(),
                name: "Ambient Preset".to_string(),
            }),
            character_id: Some("alice".to_string()),
        },
    )
    .expect("resolve scopes");

    assert_eq!(
        scopes,
        vec![
            SkillScope::Global,
            SkillScope::Profile {
                profile_id: "default-writer".to_string(),
            },
            SkillScope::Character {
                character_id: "alice".to_string(),
            },
        ]
    );

    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn agent_loop_inner_resolves_root_character_scoped_skills() {
    let root = std::env::temp_dir().join(format!(
        "tauritavern-agent-root-character-skill-{}",
        Uuid::new_v4().simple()
    ));
    let repository = Arc::new(FileAgentRepository::new(root.clone()));
    let skill_repository = Arc::new(FileSkillRepository::new(root.join("skills")));
    install_inline_skill(
        &skill_repository,
        SkillScope::Character {
            character_id: "alice".to_string(),
        },
        "root-character",
        "Root character voice notes.",
        "# Root Character\n\nUse quiet, specific sensory details.",
    )
    .await;
    let skill_service = Arc::new(SkillService::new(skill_repository));
    let model_gateway = Arc::new(MockAgentModelGateway::new(vec![
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_root_skill_read",
                        "type": "function",
                        "function": {
                            "name": "skill_read",
                            "arguments": "{\"name\":\"root-character\",\"path\":\"SKILL.md\"}"
                        }
                    }]
                }
            }]
        }),
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [
                        {
                            "id": "call_write_main",
                            "type": "function",
                            "function": {
                                "name": "workspace_write_file",
                                "arguments": "{\"path\":\"output/main.md\",\"content\":\"Root used character skill.\"}"
                            }
                        },
                        {
                            "id": "call_finish",
                            "type": "function",
                            "function": {
                                "name": "workspace_finish",
                                "arguments": "{}"
                            }
                        }
                    ]
                }
            }]
        }),
    ]));
    let model_gateway_probe = model_gateway.clone();
    let service = AgentRuntimeService::new(
        repository.clone(),
        repository.clone(),
        repository.clone(),
        repository.clone(),
        test_chat_repository(&root),
        test_chat_repository(&root),
        skill_service,
        model_gateway,
        test_profile_service(&root),
        test_llm_connection_service(&root),
    );
    let run = AgentRun {
        id: "run_root_character_skill_test".to_string(),
        workspace_id: "chat_root_character_skill_test".to_string(),
        stable_chat_id: "stable_root_character_skill_test".to_string(),
        chat_ref: AgentChatRef::Character {
            character_id: "alice".to_string(),
            file_name: "alice.png".to_string(),
        },
        generation_type: "normal".to_string(),
        profile_id: None,
        skill_scope_refs: AgentRunSkillScopeRefs {
            preset: None,
            character_id: Some("alice".to_string()),
        },
        persist_base_state_id: None,
        input_message_count: None,
        presentation: AgentRunPresentation::Background,
        status: AgentRunStatus::Created,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    repository.create_run(&run).await.expect("create run");
    let request = ChatCompletionGenerateRequestDto {
        payload: json!({
            "chat_completion_source": "openai",
            "model": "test-model",
            "messages": prompt_messages("read the character skill")
        })
        .as_object()
        .cloned()
        .unwrap(),
    };
    let prompt_snapshot = json!({ "chatCompletionPayload": request.payload.clone() });
    let (_cancel_sender, mut cancel_receiver) = watch::channel(false);
    let mut profile = test_resolved_profile(&root).await;
    profile.skills.visible = vec!["root-character".to_string()];

    service
        .execute_agent_loop_run_inner(
            &run.id,
            prompt_snapshot,
            request,
            profile,
            &mut cancel_receiver,
        )
        .await
        .expect("agent loop");

    let requests = model_gateway_probe.requests().await;
    assert_eq!(requests.len(), 2);
    let root_skill_results = tool_results_from_request(&requests[1])
        .into_iter()
        .filter(|result| result.name == "skill.read")
        .collect::<Vec<_>>();
    assert_eq!(root_skill_results.len(), 1);
    assert!(
        root_skill_results[0]
            .content
            .contains("Use quiet, specific sensory details.")
    );

    let resolved_skills = repository
        .read_text(
            &run.id,
            &WorkspacePath::parse("input/resolved_skills.json").unwrap(),
        )
        .await
        .expect("read resolved skills");
    let resolved_skills: Value =
        serde_json::from_str(&resolved_skills.text).expect("resolved skills JSON");
    assert_eq!(
        resolved_skills[0]["scope"],
        json!({ "kind": "character", "characterId": "alice" })
    );

    tokio::fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn subagent_current_prompt_snapshot_reads_ambient_preset_and_character_skills() {
    let root = std::env::temp_dir().join(format!(
        "tauritavern-agent-child-ambient-skills-{}",
        Uuid::new_v4().simple()
    ));
    let repository = Arc::new(FileAgentRepository::new(root.clone()));
    let skill_repository = Arc::new(FileSkillRepository::new(root.join("skills")));
    install_inline_skill(
        &skill_repository,
        SkillScope::Preset {
            api_id: "openai".to_string(),
            name: "Root Preset".to_string(),
        },
        "ambient-preset",
        "Ambient preset craft rules.",
        "# Preset Skill\n\nUse the root preset rhythm.",
    )
    .await;
    install_inline_skill(
        &skill_repository,
        SkillScope::Character {
            character_id: "alice".to_string(),
        },
        "ambient-character",
        "Ambient character continuity notes.",
        "# Character Skill\n\nAlice avoids ornate metaphors.",
    )
    .await;
    let skill_service = Arc::new(SkillService::new(skill_repository));
    let profile_service = test_profile_service(&root);
    let mut child_profile = profile_service
        .load_profile("default-writer")
        .await
        .expect("load default profile")
        .expect("default profile exists");
    child_profile.id = AgentProfileId::parse("scene-critic").expect("profile id");
    child_profile.display_name = "Scene Critic".to_string();
    child_profile.description = Some("Reads scoped skills before returning notes.".to_string());
    child_profile.tools.allow.retain(|name| {
        !matches!(
            name.as_str(),
            "agent.list" | "agent.delegate" | "agent.await"
        )
    });
    child_profile.skills.visible = vec![
        "ambient-preset".to_string(),
        "ambient-character".to_string(),
    ];
    child_profile.delegation = AgentDelegationPolicy {
        callable: true,
        allow_as_subagent: true,
        allowed_callers: vec!["default-writer".to_string()],
        description_for_agents: Some("Use scoped skills, then return concise notes.".to_string()),
        ..Default::default()
    };

    let model_gateway = Arc::new(MockAgentModelGateway::new(vec![
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [
                        {
                            "id": "call_delegate_critic",
                            "type": "function",
                            "function": {
                                "name": "agent_delegate",
                                "arguments": serde_json::to_string(&json!({
                                    "agentId": "scene-critic",
                                    "task": {
                                        "title": "Read scoped skills",
                                        "objective": "Read preset and character skills before returning.",
                                        "context": { "draft": "A quiet scene." },
                                        "expectedOutput": { "format": "short capsule" }
                                    },
                                    "budget": { "maxRounds": 4, "maxToolCalls": 6 }
                                })).unwrap()
                            }
                        },
                        {
                            "id": "call_await_critic",
                            "type": "function",
                            "function": {
                                "name": "agent_await",
                                "arguments": "{\"mode\":\"nextCompleted\",\"timeoutMs\":5000}"
                            }
                        }
                    ]
                }
            }]
        }),
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [
                        {
                            "id": "call_child_skill_list",
                            "type": "function",
                            "function": {
                                "name": "skill_list",
                                "arguments": "{}"
                            }
                        },
                        {
                            "id": "call_child_preset_skill",
                            "type": "function",
                            "function": {
                                "name": "skill_read",
                                "arguments": "{\"name\":\"ambient-preset\",\"path\":\"SKILL.md\"}"
                            }
                        },
                        {
                            "id": "call_child_character_skill",
                            "type": "function",
                            "function": {
                                "name": "skill_read",
                                "arguments": "{\"name\":\"ambient-character\",\"path\":\"SKILL.md\"}"
                            }
                        }
                    ]
                }
            }]
        }),
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_task_return",
                        "type": "function",
                        "function": {
                            "name": "task_return",
                            "arguments": serde_json::to_string(&json!({
                                "summary": "Scoped skills were available.",
                                "status": "completed",
                                "confidence": "high",
                                "findings": [{ "kind": "skill", "text": "Read preset and character skills." }]
                            })).unwrap()
                        }
                    }]
                }
            }]
        }),
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [
                        {
                            "id": "call_parent_write",
                            "type": "function",
                            "function": {
                                "name": "workspace_write_file",
                                "arguments": "{\"path\":\"output/main.md\",\"content\":\"Parent received scoped skill notes.\"}"
                            }
                        },
                        {
                            "id": "call_parent_finish",
                            "type": "function",
                            "function": {
                                "name": "workspace_finish",
                                "arguments": "{}"
                            }
                        }
                    ]
                }
            }]
        }),
    ]));
    let model_gateway_probe = model_gateway.clone();
    let service = Arc::new(AgentRuntimeService::new(
        repository.clone(),
        repository.clone(),
        repository.clone(),
        repository.clone(),
        test_chat_repository(&root),
        test_chat_repository(&root),
        skill_service,
        model_gateway,
        profile_service.clone(),
        test_llm_connection_service(&root),
    ));
    profile_service
        .save_profile(child_profile, service.tool_specs())
        .await
        .expect("save child profile");
    let run = AgentRun {
        id: "run_child_ambient_skill_test".to_string(),
        workspace_id: "chat_child_ambient_skill_test".to_string(),
        stable_chat_id: "stable_child_ambient_skill_test".to_string(),
        chat_ref: AgentChatRef::Character {
            character_id: "alice".to_string(),
            file_name: "alice.png".to_string(),
        },
        generation_type: "normal".to_string(),
        profile_id: None,
        skill_scope_refs: AgentRunSkillScopeRefs {
            preset: Some(AgentPresetRef {
                api_id: "openai".to_string(),
                name: "Root Preset".to_string(),
            }),
            character_id: Some("alice".to_string()),
        },
        persist_base_state_id: None,
        input_message_count: None,
        presentation: AgentRunPresentation::Background,
        status: AgentRunStatus::Created,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    repository.create_run(&run).await.expect("create run");
    insert_active_run_handle(&service, &run.id).await;

    let request = ChatCompletionGenerateRequestDto {
        payload: json!({
            "chat_completion_source": "openai",
            "model": "test-model",
            "messages": prompt_messages("delegate scoped skill reading")
        })
        .as_object()
        .cloned()
        .unwrap(),
    };
    let prompt_snapshot = json!({ "chatCompletionPayload": request.payload.clone() });
    let (_cancel_sender, mut cancel_receiver) = watch::channel(false);
    let profile = test_resolved_profile(&root).await;

    service
        .execute_agent_loop_run_inner(
            &run.id,
            prompt_snapshot,
            request,
            profile,
            &mut cancel_receiver,
        )
        .await
        .expect("agent loop");

    let tasks = repository.list_tasks(&run.id).await.expect("list tasks");
    assert_eq!(tasks.len(), 1);
    let task = &tasks[0];
    assert_eq!(task.status, AgentTaskStatus::Completed);

    let requests = model_gateway_probe.requests().await;
    assert_eq!(requests.len(), 4);
    let child_tool_results = tool_results_from_request(&requests[2]);
    let listed_skills = child_tool_results
        .iter()
        .find(|result| result.name == "skill.list")
        .expect("skill.list result");
    assert_eq!(
        listed_skills.structured["skills"],
        json!([
            {
                "name": "ambient-character",
                "description": "Ambient character continuity notes."
            },
            {
                "name": "ambient-preset",
                "description": "Ambient preset craft rules."
            }
        ])
    );
    assert!(
        child_tool_results
            .iter()
            .any(|result| result.name == "skill.read"
                && result.content.contains("Use the root preset rhythm."))
    );
    assert!(
        child_tool_results
            .iter()
            .any(|result| result.name == "skill.read"
                && result.content.contains("Alice avoids ornate metaphors."))
    );

    let child_resolved_skills = repository
        .read_text(
            &run.id,
            &WorkspacePath::parse(format!(
                "input/invocations/{}/resolved_skills.json",
                task.child_invocation_id
            ))
            .unwrap(),
        )
        .await
        .expect("read child resolved skills");
    let child_resolved_skills: Value =
        serde_json::from_str(&child_resolved_skills.text).expect("child resolved skills JSON");
    let resolved_scopes = child_resolved_skills
        .as_array()
        .expect("resolved skills array")
        .iter()
        .map(|skill| skill["scope"].clone())
        .collect::<Vec<_>>();
    assert!(resolved_scopes.contains(&json!({
        "kind": "preset",
        "apiId": "openai",
        "name": "Root Preset",
    })));
    assert!(resolved_scopes.contains(&json!({
        "kind": "character",
        "characterId": "alice",
    })));

    let events = repository
        .read_events(
            &run.id,
            AgentRunEventReadQuery {
                after_seq: Some(0),
                before_seq: None,
                limit: 100,
                invocation_id: None,
            },
        )
        .await
        .expect("read events");
    assert!(events.iter().any(|event| {
        event.event_type == "skill_scopes_resolved"
            && event.payload["invocationId"] == task.child_invocation_id
            && event.payload["refs"]["characterId"] == "alice"
    }));

    tokio::fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn agent_delegate_await_runs_return_mode_subagent() {
    let root = std::env::temp_dir().join(format!(
        "tauritavern-agent-subagent-{}",
        Uuid::new_v4().simple()
    ));
    let repository = Arc::new(FileAgentRepository::new(root.clone()));
    let profile_service = test_profile_service(&root);
    let mut child_profile = profile_service
        .load_profile("default-writer")
        .await
        .expect("load default profile")
        .expect("default profile exists");
    child_profile.id = AgentProfileId::parse("scene-critic").expect("profile id");
    child_profile.display_name = "Scene Critic".to_string();
    child_profile.description = Some("Reviews a scene and returns concise notes.".to_string());
    child_profile.tools.allow.retain(|name| {
        !matches!(
            name.as_str(),
            "agent.list" | "agent.delegate" | "agent.await"
        )
    });
    child_profile.delegation = AgentDelegationPolicy {
        callable: true,
        allow_as_subagent: true,
        allowed_callers: vec!["default-writer".to_string()],
        description_for_agents: Some("Return concise scene critique.".to_string()),
        ..Default::default()
    };
    child_profile.workspace.visible_roots = vec![
        "summaries".to_string(),
        "output".to_string(),
        "persist".to_string(),
    ];
    child_profile.workspace.writable_roots = vec!["summaries".to_string(), "output".to_string()];

    let model_gateway = Arc::new(MockAgentModelGateway::new(vec![
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [
                        {
                            "id": "call_delegate_critic",
                            "type": "function",
                            "function": {
                                "name": "agent_delegate",
                                "arguments": serde_json::to_string(&json!({
                                    "agentId": "scene-critic",
                                    "task": {
                                        "objective": "Find one concrete improvement.",
                                        "context": { "draft": "A quiet scene." },
                                        "expectedOutput": { "format": "short capsule" }
                                    },
                                    "budget": { "maxRounds": 4, "maxToolCalls": 4 }
                                })).unwrap()
                            }
                        },
                        {
                            "id": "call_await_critic",
                            "type": "function",
                            "function": {
                                "name": "agent_await",
                                "arguments": "{\"mode\":\"nextCompleted\",\"timeoutMs\":5000}"
                            }
                        }
                    ]
                }
            }]
        }),
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [
                        {
                            "id": "call_child_note",
                            "type": "function",
                            "function": {
                                "name": "workspace_write_file",
                                "arguments": "{\"path\":\"summaries/notes.md\",\"content\":\"Add a concrete sound or texture.\"}"
                            }
                        },
                        {
                            "id": "call_child_section",
                            "type": "function",
                            "function": {
                                "name": "workspace_write_file",
                                "arguments": "{\"path\":\"output/sections/scene_03.md\",\"content\":\"A quiet scene with rain tapping the glass.\"}"
                            }
                        }
                    ]
                }
            }]
        }),
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_task_return",
                        "type": "function",
                        "function": {
                            "name": "task_return",
                            "arguments": serde_json::to_string(&json!({
                                "summary": "The scene needs a sharper sensory anchor.",
                                "status": "completed",
                                "confidence": "high",
                                "artifacts": [{
                                    "path": "summaries/notes.md",
                                    "kind": "markdown",
                                    "role": "supportingNote"
                                }, {
                                    "path": "output/sections/scene_03.md",
                                    "kind": "markdown",
                                    "role": "draftSection"
                                }],
                                "findings": [{ "kind": "revision", "text": "Add a concrete sound or texture." }]
                            })).unwrap()
                        }
                    }]
                }
            }]
        }),
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [
                        {
                            "id": "call_parent_write",
                            "type": "function",
                            "function": {
                                "name": "workspace_write_file",
                                "arguments": "{\"path\":\"output/main.md\",\"content\":\"Parent used critic result.\"}"
                            }
                        },
                        {
                            "id": "call_parent_finish",
                            "type": "function",
                            "function": {
                                "name": "workspace_finish",
                                "arguments": "{}"
                            }
                        }
                    ]
                }
            }]
        }),
    ]));
    let model_gateway_probe = model_gateway.clone();

    let service = Arc::new(AgentRuntimeService::new(
        repository.clone(),
        repository.clone(),
        repository.clone(),
        repository.clone(),
        test_chat_repository(&root),
        test_chat_repository(&root),
        test_skill_service(&root),
        model_gateway,
        profile_service.clone(),
        test_llm_connection_service(&root),
    ));
    profile_service
        .save_profile(child_profile, service.tool_specs())
        .await
        .expect("save child profile");
    let run = AgentRun {
        id: "run_subagent_test".to_string(),
        workspace_id: "chat_subagent_test".to_string(),
        stable_chat_id: "stable_subagent_test".to_string(),
        chat_ref: AgentChatRef::Character {
            character_id: "Seraphina".to_string(),
            file_name: "Seraphina.png".to_string(),
        },
        generation_type: "normal".to_string(),
        profile_id: None,
        skill_scope_refs: Default::default(),
        persist_base_state_id: None,
        input_message_count: None,
        presentation: AgentRunPresentation::Background,
        status: AgentRunStatus::Created,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    repository.create_run(&run).await.expect("create run");
    insert_active_run_handle(&service, &run.id).await;

    let request = ChatCompletionGenerateRequestDto {
        payload: json!({
            "chat_completion_source": "openai",
            "model": "test-model",
            "messages": prompt_messages("ask a critic, then finish")
        })
        .as_object()
        .cloned()
        .unwrap(),
    };
    let prompt_snapshot = json!({ "chatCompletionPayload": request.payload.clone() });
    let (_cancel_sender, mut cancel_receiver) = watch::channel(false);
    let profile = test_resolved_profile(&root).await;

    service
        .execute_agent_loop_run_inner(
            &run.id,
            prompt_snapshot,
            request,
            profile,
            &mut cancel_receiver,
        )
        .await
        .expect("agent loop");

    let tasks = repository.list_tasks(&run.id).await.expect("list tasks");
    assert_eq!(tasks.len(), 1);
    let task = &tasks[0];
    assert_eq!(task.target_profile_id, "scene-critic");
    assert_eq!(task.workspace_key, "scene-critic");
    assert_eq!(
        task.status,
        crate::domain::models::agent::AgentTaskStatus::Completed
    );
    let child_invocation = repository
        .load_invocation(&run.id, &task.child_invocation_id)
        .await
        .expect("load child invocation");
    assert_eq!(child_invocation.status, AgentInvocationStatus::Completed);
    assert_eq!(
        child_invocation.exit_policy,
        AgentInvocationExitPolicy::TaskReturnRequired
    );

    let result_ref =
        WorkspacePath::parse(task.result_ref.as_deref().expect("result ref")).expect("result path");
    let result = repository
        .read_text(&run.id, &result_ref)
        .await
        .expect("read task result");
    let result: Value = serde_json::from_str(&result.text).expect("result JSON");
    assert_eq!(
        result["summary"],
        "The scene needs a sharper sensory anchor."
    );
    assert_eq!(result["runtime"]["taskId"], task.id);
    assert_eq!(
        result["runtime"]["childInvocationId"],
        task.child_invocation_id
    );
    assert_eq!(result["runtime"]["workspaceKey"], task.workspace_key);
    assert_eq!(result["summaryRef"], "summaries/scene-critic-result.md");
    assert_eq!(
        result["result"]["artifacts"][0]["path"],
        "summaries/notes.md"
    );
    assert_eq!(
        result["result"]["artifacts"][1]["path"],
        "output/sections/scene_03.md"
    );
    assert!(result.get("targetProfileId").is_none());
    repository
        .read_text(
            &run.id,
            &WorkspacePath::parse("summaries/notes.md").unwrap(),
        )
        .await
        .expect("read child note");
    let child_section = repository
        .read_text(
            &run.id,
            &WorkspacePath::parse("output/sections/scene_03.md").unwrap(),
        )
        .await
        .expect("read child output section");
    assert_eq!(
        child_section.text,
        "A quiet scene with rain tapping the glass."
    );

    let child_response = repository
        .read_text(
            &run.id,
            &WorkspacePath::parse(format!(
                "model-responses/{}/round-001.json",
                task.child_invocation_id
            ))
            .unwrap(),
        )
        .await
        .expect("read child model response");
    let child_response: Value =
        serde_json::from_str(&child_response.text).expect("child response JSON");
    assert_eq!(child_response["invocationId"], task.child_invocation_id);
    let child_model_turn = service
        .read_model_turn(AgentReadModelTurnDto {
            run_id: run.id.clone(),
            invocation_id: Some(task.child_invocation_id.clone()),
            round: 1,
            max_chars: 40_000,
        })
        .await
        .expect("read child model turn");
    assert_eq!(
        child_model_turn.model_response_path,
        format!(
            "model-responses/{}/round-001.json",
            task.child_invocation_id
        )
    );
    assert_eq!(child_model_turn.tool_calls.len(), 2);
    assert_eq!(child_model_turn.tool_calls[0].name, "workspace.write_file");

    let child_event_page = service
        .read_events(AgentReadEventsDto {
            run_id: run.id.clone(),
            after_seq: Some(0),
            before_seq: None,
            limit: 300,
            invocation_id: Some(task.child_invocation_id.clone()),
            include_timeline_projection: false,
        })
        .await
        .expect("read child invocation events");
    assert!(!child_event_page.events.is_empty());
    assert!(
        child_event_page
            .events
            .iter()
            .all(|event| event_belongs_to_invocation(event, &task.child_invocation_id))
    );
    assert!(child_event_page.events.iter().any(|event| {
        event.event_type == "model_completed"
            && event.payload["invocationId"] == task.child_invocation_id
    }));
    let delegate_event = child_event_page
        .events
        .iter()
        .find(|event| event.event_type == "agent_delegate_started")
        .expect("delegate event belongs to child invocation through canonical related scope");
    assert_eq!(
        delegate_event.payload["eventScope"]["invocationId"],
        "inv_root"
    );
    assert_eq!(
        delegate_event.payload["eventScope"]["relatedInvocationIds"][0],
        task.child_invocation_id
    );
    let task_return_event = child_event_page
        .events
        .iter()
        .find(|event| event.event_type == "task_return_completed")
        .expect("task return event");
    assert_eq!(task_return_event.payload["parentInvocationId"], "inv_root");
    assert_eq!(
        task_return_event.payload["childInvocationId"],
        task.child_invocation_id
    );
    assert_eq!(
        task_return_event.payload["eventScope"]["invocationId"],
        task.child_invocation_id
    );
    assert_eq!(
        task_return_event.payload["eventScope"]["relatedInvocationIds"][0],
        "inv_root"
    );
    assert!(
        !child_event_page
            .events
            .iter()
            .any(|event| event.event_type == "run_completed")
    );
    assert!(child_event_page.timeline_projection.is_none());

    let child_event_tail = service
        .read_events(AgentReadEventsDto {
            run_id: run.id.clone(),
            after_seq: None,
            before_seq: Some(u64::MAX),
            limit: 2,
            invocation_id: Some(task.child_invocation_id.clone()),
            include_timeline_projection: false,
        })
        .await
        .expect("read child invocation event tail");
    assert_eq!(child_event_tail.events.len(), 2);
    assert!(
        child_event_tail
            .events
            .iter()
            .all(|event| event_belongs_to_invocation(event, &task.child_invocation_id))
    );

    let requests = model_gateway_probe.requests().await;
    assert_eq!(requests.len(), 4);
    assert_eq!(requests[0].provider_state["invocationId"], "inv_root");
    assert_eq!(
        requests[1].provider_state["invocationId"],
        task.child_invocation_id
    );
    assert_eq!(
        requests[2].provider_state["invocationId"],
        task.child_invocation_id
    );
    assert!(
        requests[1]
            .tools
            .iter()
            .any(|tool| tool.name == "task.return")
    );
    assert!(
        !requests[1]
            .tools
            .iter()
            .any(|tool| tool.name == "workspace.finish" || tool.name == "agent.delegate")
    );
    let child_system_prompt = requests[1]
        .messages
        .iter()
        .find(|message| message.role == AgentModelRole::System)
        .and_then(|message| {
            message.parts.iter().find_map(|part| match part {
                AgentModelContentPart::Text { text } => Some(text.as_str()),
                _ => None,
            })
        })
        .expect("child system prompt");
    assert!(child_system_prompt.contains("Delegated task workspace"));
    assert!(child_system_prompt.contains("same logical workspace paths"));
    assert!(!child_system_prompt.contains("summaries/parent/"));
    assert!(!child_system_prompt.contains("summaries/agents/"));
    assert!(
        child_system_prompt
            .contains("- Visible workspace roots for this task: summaries/, output/, persist/.")
    );
    assert!(
        child_system_prompt
            .contains("- Writable workspace roots for this task: summaries/, output/.")
    );
    let child_write_spec = requests[1]
        .tools
        .iter()
        .find(|tool| tool.name == "workspace.write_file")
        .expect("child write spec");
    assert!(
        child_write_spec
            .description
            .contains("Writable prefixes are summaries/, output/")
    );
    assert!(!child_write_spec.description.contains("persist/"));
    assert!(!child_write_spec.description.contains("output/main.md"));
    let child_task_prompt = requests[1]
        .messages
        .iter()
        .find(|message| message.role == AgentModelRole::User)
        .and_then(|message| {
            message.parts.iter().find_map(|part| match part {
                AgentModelContentPart::Text { text } => Some(text.as_str()),
                _ => None,
            })
        })
        .expect("child task prompt");
    assert!(child_task_prompt.contains("# Delegated Task"));
    assert!(child_task_prompt.contains("## Objective"));
    assert!(child_task_prompt.contains("## Context"));
    assert!(child_task_prompt.contains("exact workspace paths"));
    assert!(!child_task_prompt.contains("scratch/notes.md"));
    assert!(!child_task_prompt.contains("summaries/parent/"));
    assert!(!child_task_prompt.contains("summaries/agents/"));
    assert!(!child_task_prompt.contains("Parent invocation"));
    assert!(!child_task_prompt.contains("Task packet"));
    assert!(!child_task_prompt.contains("Target Agent profile"));
    assert!(!child_task_prompt.contains("summaries/agents/scene-critic"));
    assert!(!child_task_prompt.contains("inv_"));

    let child_write_results = requests[2]
        .messages
        .iter()
        .filter(|message| message.role == AgentModelRole::Tool)
        .filter_map(|message| message.parts.first())
        .filter_map(|part| match part {
            AgentModelContentPart::ToolResult { result } => Some(result),
            _ => None,
        })
        .filter(|result| result.name == "workspace.write_file")
        .collect::<Vec<_>>();
    assert_eq!(child_write_results.len(), 2);
    assert!(
        child_write_results
            .iter()
            .any(|result| result.structured["path"] == "summaries/notes.md")
    );
    assert!(
        child_write_results
            .iter()
            .any(|result| result.structured["path"] == "output/sections/scene_03.md")
    );

    let parent_tool_results = requests[3]
        .messages
        .iter()
        .filter(|message| message.role == AgentModelRole::Tool)
        .filter_map(|message| message.parts.first())
        .filter_map(|part| match part {
            AgentModelContentPart::ToolResult { result } => Some(result),
            _ => None,
        })
        .collect::<Vec<_>>();
    let delegate_result = parent_tool_results
        .iter()
        .find(|result| result.name == "agent.delegate")
        .expect("delegate tool result");
    assert!(delegate_result.structured.get("invocationId").is_none());
    assert!(delegate_result.content.contains("continue other work"));
    assert!(!delegate_result.content.contains("collect the result"));
    let await_result = parent_tool_results
        .iter()
        .find(|result| result.name == "agent.await")
        .expect("await tool result");
    assert!(
        await_result
            .content
            .contains("Treat these delegated results as context for you")
    );
    assert!(
        await_result
            .content
            .contains("If no more work is needed, call workspace_finish")
    );
    assert!(await_result.content.contains("do not answer in plain text"));
    let await_task = &await_result.structured["tasks"][0];
    assert_eq!(
        await_task["summary"],
        "The scene needs a sharper sensory anchor."
    );
    assert!(await_task.get("invocationId").is_none());
    assert!(await_task.get("resultRef").is_none());
    assert_eq!(await_task["artifacts"][0]["path"], "summaries/notes.md");
    assert_eq!(
        await_task["artifacts"][1]["path"],
        "output/sections/scene_03.md"
    );
    assert!(!await_result.content.contains("invocation"));
    assert_eq!(requests[3].provider_state["invocationId"], "inv_root");

    wait_for_closed_sessions(
        &model_gateway_probe,
        vec![
            "run_subagent_test:inv_root".to_string(),
            format!("run_subagent_test:{}", task.child_invocation_id),
        ],
    )
    .await;

    tokio::fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn agent_handoff_continues_after_prior_commit() {
    let root = std::env::temp_dir().join(format!(
        "tauritavern-agent-handoff-{}",
        Uuid::new_v4().simple()
    ));
    let repository = Arc::new(FileAgentRepository::new(root.clone()));
    let profile_service = test_profile_service(&root);
    let mut editor_profile = profile_service
        .load_profile("default-writer")
        .await
        .expect("load default profile")
        .expect("default profile exists");
    editor_profile.id = AgentProfileId::parse("final-editor").expect("profile id");
    editor_profile.display_name = "Final Editor".to_string();
    editor_profile.description =
        Some("Takes over a draft and prepares the final message.".to_string());
    editor_profile.tools.allow.retain(|name| {
        !matches!(
            name.as_str(),
            "agent.list" | "agent.delegate" | "agent.await"
        )
    });
    editor_profile.tools.allow.push("agent.handoff".to_string());
    editor_profile.delegation = AgentDelegationPolicy {
        can_handoff: true,
        callable: true,
        allow_as_handoff_target: true,
        allowed_callers: vec!["default-writer".to_string()],
        description_for_agents: Some(
            "Revise the current draft and pass it to the acceptance finisher.".to_string(),
        ),
        ..Default::default()
    };
    let mut acceptance_profile = profile_service
        .load_profile("default-writer")
        .await
        .expect("load default profile")
        .expect("default profile exists");
    acceptance_profile.id = AgentProfileId::parse("acceptance-finisher").expect("profile id");
    acceptance_profile.display_name = "Acceptance Finisher".to_string();
    acceptance_profile.description = Some(
        "Takes over after a final commit and closes the run if no further edit is needed."
            .to_string(),
    );
    acceptance_profile.tools.allow.retain(|name| {
        matches!(
            name.as_str(),
            "workspace.finish" | "workspace.read_file" | "workspace.write_file"
        )
    });
    acceptance_profile.run.direct_runnable = false;
    acceptance_profile.run.presentation = AgentRunPresentation::Foreground;
    acceptance_profile.delegation = AgentDelegationPolicy {
        callable: true,
        allow_as_handoff_target: true,
        allowed_callers: vec!["final-editor".to_string()],
        description_for_agents: Some(
            "Accept the already committed final draft and finish the run.".to_string(),
        ),
        ..Default::default()
    };

    let model_gateway = Arc::new(MockAgentModelGateway::new(vec![
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [
                        {
                            "id": "call_root_draft",
                            "type": "function",
                            "function": {
                                "name": "workspace_write_file",
                                "arguments": "{\"path\":\"output/main.md\",\"content\":\"Draft scene.\"}"
                            }
                        },
                        {
                            "id": "call_root_commit",
                            "type": "function",
                            "function": {
                                "name": "workspace_commit",
                                "arguments": "{}"
                            }
                        },
                        {
                            "id": "call_root_handoff",
                            "type": "function",
                            "function": {
                                "name": "agent_handoff",
                                "arguments": serde_json::to_string(&json!({
                                    "agentId": "final-editor",
                                    "handoff": {
                                        "title": "Final edit",
                                        "reason": "The draft has been committed and needs final revision.",
                                        "objective": "Read output/main.md, revise it, commit the final message, then hand off for acceptance.",
                                        "contextSummary": "The current draft is already visible in chat but may be revised.",
                                        "workspaceRefs": ["output/main.md"],
                                        "mustPreserve": ["Keep the scene quiet."],
                                        "completionCriteria": ["Read output/main.md", "Commit the revised message", "Hand off to acceptance-finisher"]
                                    }
                                })).unwrap()
                            }
                        }
                    ]
                }
            }]
        }),
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_editor_read",
                        "type": "function",
                        "function": {
                            "name": "workspace_read_file",
                            "arguments": "{\"path\":\"output/main.md\"}"
                        }
                    }]
                }
            }]
        }),
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [
                        {
                            "id": "call_editor_patch",
                            "type": "function",
                            "function": {
                                "name": "workspace_apply_patch",
                                "arguments": serde_json::to_string(&json!({
                                    "path": "output/main.md",
                                    "old_string": "Draft scene.",
                                    "new_string": "Final quiet scene."
                                })).unwrap()
                            }
                        },
                        {
                            "id": "call_editor_commit",
                            "type": "function",
                            "function": {
                                "name": "workspace_commit",
                                "arguments": "{}"
                            }
                        },
                        {
                            "id": "call_editor_handoff",
                            "type": "function",
                            "function": {
                                "name": "agent_handoff",
                                "arguments": serde_json::to_string(&json!({
                                    "agentId": "acceptance-finisher",
                                    "handoff": {
                                        "title": "Acceptance finish",
                                        "reason": "The final draft has been committed; this stage only needs to close the run if acceptable.",
                                        "objective": "Read output/main.md if needed, then finish the run without creating a new chat commit.",
                                        "contextSummary": "Final quiet scene has already been committed to chat.",
                                        "workspaceRefs": ["output/main.md"],
                                        "mustPreserve": ["Do not create a duplicate chat commit."],
                                        "completionCriteria": ["Finish the run"]
                                    }
                                })).unwrap()
                            }
                        }
                    ]
                }
            }]
        }),
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_acceptance_finish",
                        "type": "function",
                        "function": {
                            "name": "workspace_finish",
                            "arguments": "{}"
                        }
                    }]
                }
            }]
        }),
    ]));
    let model_gateway_probe = model_gateway.clone();
    let service = Arc::new(AgentRuntimeService::new(
        repository.clone(),
        repository.clone(),
        repository.clone(),
        repository.clone(),
        test_chat_repository(&root),
        test_chat_repository(&root),
        test_skill_service(&root),
        model_gateway,
        profile_service.clone(),
        test_llm_connection_service(&root),
    ));
    profile_service
        .save_profile(editor_profile, service.tool_specs())
        .await
        .expect("save editor profile");
    profile_service
        .save_profile(acceptance_profile, service.tool_specs())
        .await
        .expect("save acceptance profile");
    let run = AgentRun {
        id: "run_handoff_test".to_string(),
        workspace_id: "chat_handoff_test".to_string(),
        stable_chat_id: "stable_handoff_test".to_string(),
        chat_ref: AgentChatRef::Character {
            character_id: "Seraphina".to_string(),
            file_name: "Seraphina.png".to_string(),
        },
        generation_type: "normal".to_string(),
        profile_id: None,
        skill_scope_refs: Default::default(),
        persist_base_state_id: None,
        input_message_count: None,
        presentation: AgentRunPresentation::Foreground,
        status: AgentRunStatus::Created,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    repository.create_run(&run).await.expect("create run");
    insert_active_run_handle(&service, &run.id).await;

    let request = ChatCompletionGenerateRequestDto {
        payload: json!({
            "chat_completion_source": "openai",
            "model": "test-model",
            "messages": prompt_messages("draft then hand off to editor")
        })
        .as_object()
        .cloned()
        .unwrap(),
    };
    let prompt_snapshot = json!({ "chatCompletionPayload": request.payload.clone() });
    let (_cancel_sender, mut cancel_receiver) = watch::channel(false);
    let mut profile = test_resolved_profile(&root).await;
    profile.run.presentation = AgentRunPresentation::Foreground;
    profile.delegation.can_handoff = true;
    profile.tools.allow.push("agent.handoff".to_string());

    let resolver = tokio::spawn(resolve_chat_commits_and_persistent_state_update(
        service.clone(),
        repository.clone(),
        run.id.clone(),
        vec!["message_draft", "message_final"],
    ));
    service
        .execute_agent_loop_run_inner(
            &run.id,
            prompt_snapshot,
            request,
            profile,
            &mut cancel_receiver,
        )
        .await
        .expect("agent loop");
    resolver.await.expect("resolver task");

    let saved = repository.load_run(&run.id).await.expect("load run");
    assert_eq!(saved.status, AgentRunStatus::Completed);
    let tasks = repository.list_tasks(&run.id).await.expect("list tasks");
    assert_eq!(tasks.len(), 2);
    let editor_task = tasks
        .iter()
        .find(|task| task.target_profile_id == "final-editor")
        .expect("editor handoff task");
    let acceptance_task = tasks
        .iter()
        .find(|task| task.target_profile_id == "acceptance-finisher")
        .expect("acceptance handoff task");
    assert_eq!(
        editor_task.continuation,
        AgentDelegationContinuation::TransferControl
    );
    assert_eq!(
        acceptance_task.continuation,
        AgentDelegationContinuation::TransferControl
    );
    assert_eq!(editor_task.status, AgentTaskStatus::Completed);
    assert_eq!(acceptance_task.status, AgentTaskStatus::Completed);
    let root_invocation = repository
        .load_invocation(&run.id, "inv_root")
        .await
        .expect("load root invocation");
    assert_eq!(root_invocation.status, AgentInvocationStatus::Transferred);
    let editor_invocation = repository
        .load_invocation(&run.id, editor_task.child_invocation_id.as_str())
        .await
        .expect("load editor invocation");
    assert_eq!(
        editor_invocation.kind,
        crate::domain::models::agent::AgentInvocationKind::Handoff
    );
    assert_eq!(editor_invocation.status, AgentInvocationStatus::Transferred);
    assert_eq!(
        editor_invocation.exit_policy,
        AgentInvocationExitPolicy::RunFinishAllowed
    );
    let acceptance_invocation = repository
        .load_invocation(&run.id, acceptance_task.child_invocation_id.as_str())
        .await
        .expect("load acceptance invocation");
    assert_eq!(
        acceptance_invocation.kind,
        crate::domain::models::agent::AgentInvocationKind::Handoff
    );
    assert_eq!(
        acceptance_invocation.status,
        AgentInvocationStatus::Completed
    );
    let final_output = repository
        .read_text(&run.id, &WorkspacePath::parse("output/main.md").unwrap())
        .await
        .expect("read final output");
    assert_eq!(final_output.text, "Final quiet scene.");

    let requests = model_gateway_probe.requests().await;
    assert_eq!(requests.len(), 4);
    assert_eq!(requests[0].provider_state["invocationId"], "inv_root");
    assert_eq!(
        requests[1].provider_state["invocationId"],
        editor_task.child_invocation_id
    );
    assert_eq!(
        requests[2].provider_state["invocationId"],
        editor_task.child_invocation_id
    );
    assert_eq!(
        requests[3].provider_state["invocationId"],
        acceptance_task.child_invocation_id
    );
    let handoff_prompt = requests[1]
        .messages
        .iter()
        .find(|message| message.role == AgentModelRole::User)
        .and_then(|message| {
            message.parts.iter().find_map(|part| match part {
                AgentModelContentPart::Text { text } => Some(text.as_str()),
                _ => None,
            })
        })
        .expect("handoff prompt");
    assert!(handoff_prompt.contains("# Handoff Brief"));
    assert!(handoff_prompt.contains("You are now responsible for the next stage of this run."));
    assert!(handoff_prompt.contains("output/main.md"));
    assert!(!handoff_prompt.contains("inv_"));
    assert!(!handoff_prompt.contains("active work"));
    assert!(!handoff_prompt.contains("this Agent run"));
    assert!(!handoff_prompt.contains("your Agent"));
    let events = repository
        .read_events(
            &run.id,
            AgentRunEventReadQuery {
                after_seq: Some(0),
                before_seq: None,
                limit: 300,
                invocation_id: None,
            },
        )
        .await
        .expect("read events");
    let handoff_event = events
        .iter()
        .find(|event| event.event_type == "agent_handoff_accepted")
        .expect("handoff accepted event");
    assert_eq!(handoff_event.payload["taskId"], editor_task.id);
    assert_eq!(
        handoff_event.payload["newInvocationId"],
        editor_task.child_invocation_id
    );
    assert_eq!(
        handoff_event.payload["eventScope"]["invocationId"],
        "inv_root"
    );
    assert_eq!(
        handoff_event.payload["eventScope"]["relatedInvocationIds"][0],
        editor_task.child_invocation_id
    );
    assert!(events.iter().any(|event| {
        event.event_type == "agent_handoff_accepted"
            && event.payload["taskId"] == acceptance_task.id
            && event.payload["newInvocationId"] == acceptance_task.child_invocation_id
    }));
    let acceptance_handoff_event = events
        .iter()
        .find(|event| {
            event.event_type == "agent_handoff_accepted"
                && event.payload["taskId"] == acceptance_task.id
        })
        .expect("acceptance handoff event");
    assert_eq!(
        acceptance_handoff_event.payload["eventScope"]["invocationId"],
        editor_task.child_invocation_id
    );
    assert_eq!(
        acceptance_handoff_event.payload["eventScope"]["relatedInvocationIds"][0],
        acceptance_task.child_invocation_id
    );
    let plain_page = service
        .read_events(AgentReadEventsDto {
            run_id: run.id.clone(),
            after_seq: Some(0),
            before_seq: None,
            limit: 1,
            invocation_id: None,
            include_timeline_projection: false,
        })
        .await
        .expect("read event page without projection");
    assert_eq!(plain_page.events.len(), 1);
    assert!(plain_page.timeline_projection.is_none());
    let projected_page = service
        .read_events(AgentReadEventsDto {
            run_id: run.id.clone(),
            after_seq: Some(0),
            before_seq: None,
            limit: 1,
            invocation_id: None,
            include_timeline_projection: true,
        })
        .await
        .expect("read projected event page");
    assert_eq!(projected_page.events.len(), 1);
    let projection = projected_page
        .timeline_projection
        .expect("timeline projection");
    assert_eq!(
        projection.foreground_invocation_ids,
        vec![
            "inv_root".to_string(),
            editor_task.child_invocation_id.clone(),
            acceptance_task.child_invocation_id.clone(),
        ]
    );
    assert_eq!(projection.invocations.len(), 3);
    assert!(
        projection
            .invocations
            .iter()
            .any(|invocation| invocation.invocation_id == "inv_root")
    );
    assert_eq!(projection.delegation_edges.len(), 2);
    assert_eq!(
        projection.delegation_edges[0].task_id,
        editor_task.id.as_str()
    );
    assert_eq!(
        projection.delegation_edges[0].source_invocation_id,
        "inv_root"
    );
    assert_eq!(
        projection.delegation_edges[0].target_invocation_id,
        editor_task.child_invocation_id.as_str()
    );
    assert_eq!(
        projection.delegation_edges[0].continuation,
        AgentDelegationContinuation::TransferControl
    );
    assert_eq!(
        projection.delegation_edges[0].status,
        AgentTaskStatus::Completed
    );
    assert_eq!(
        projection.delegation_edges[1].task_id,
        acceptance_task.id.as_str()
    );

    wait_for_closed_sessions(
        &model_gateway_probe,
        vec![
            "run_handoff_test:inv_root".to_string(),
            format!("run_handoff_test:{}", editor_task.child_invocation_id),
            format!("run_handoff_test:{}", acceptance_task.child_invocation_id),
        ],
    )
    .await;

    tokio::fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn agent_handoff_denies_pending_delegated_tasks() {
    let root = std::env::temp_dir().join(format!(
        "tauritavern-agent-handoff-pending-{}",
        Uuid::new_v4().simple()
    ));
    let repository = Arc::new(FileAgentRepository::new(root.clone()));
    let profile_service = test_profile_service(&root);
    let mut child_profile = profile_service
        .load_profile("default-writer")
        .await
        .expect("load default profile")
        .expect("default profile exists");
    child_profile.id = AgentProfileId::parse("scene-critic").expect("profile id");
    child_profile.display_name = "Scene Critic".to_string();
    child_profile.tools.allow.retain(|name| {
        !matches!(
            name.as_str(),
            "agent.list" | "agent.delegate" | "agent.handoff" | "agent.await"
        )
    });
    child_profile.delegation = AgentDelegationPolicy {
        callable: true,
        allow_as_subagent: true,
        allowed_callers: vec!["default-writer".to_string()],
        description_for_agents: Some("Return concise notes.".to_string()),
        ..Default::default()
    };

    let model_gateway = Arc::new(PendingDelegateHandoffModelGateway::new());
    let service = Arc::new(AgentRuntimeService::new(
        repository.clone(),
        repository.clone(),
        repository.clone(),
        repository.clone(),
        test_chat_repository(&root),
        test_chat_repository(&root),
        test_skill_service(&root),
        model_gateway.clone(),
        profile_service.clone(),
        test_llm_connection_service(&root),
    ));
    profile_service
        .save_profile(child_profile, service.tool_specs())
        .await
        .expect("save child profile");
    let run = AgentRun {
        id: "run_handoff_pending_test".to_string(),
        workspace_id: "chat_handoff_pending_test".to_string(),
        stable_chat_id: "stable_handoff_pending_test".to_string(),
        chat_ref: AgentChatRef::Character {
            character_id: "Seraphina".to_string(),
            file_name: "Seraphina.png".to_string(),
        },
        generation_type: "normal".to_string(),
        profile_id: None,
        skill_scope_refs: Default::default(),
        persist_base_state_id: None,
        input_message_count: None,
        presentation: AgentRunPresentation::Background,
        status: AgentRunStatus::Created,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    repository.create_run(&run).await.expect("create run");
    insert_active_run_handle(&service, &run.id).await;

    let request = ChatCompletionGenerateRequestDto {
        payload: json!({
            "chat_completion_source": "openai",
            "model": "test-model",
            "messages": prompt_messages("delegate then try to hand off too early")
        })
        .as_object()
        .cloned()
        .unwrap(),
    };
    let prompt_snapshot = json!({ "chatCompletionPayload": request.payload.clone() });
    let (_cancel_sender, mut cancel_receiver) = watch::channel(false);
    let mut profile = test_resolved_profile(&root).await;
    profile.delegation.can_handoff = true;
    profile.tools.allow.push("agent.handoff".to_string());

    service
        .execute_agent_loop_run_inner(
            &run.id,
            prompt_snapshot,
            request,
            profile,
            &mut cancel_receiver,
        )
        .await
        .expect("agent loop");

    let saved = repository.load_run(&run.id).await.expect("load run");
    assert_eq!(saved.status, AgentRunStatus::Completed);
    let tasks = repository.list_tasks(&run.id).await.expect("list tasks");
    assert_eq!(tasks.len(), 1);
    assert_eq!(
        tasks[0].continuation,
        AgentDelegationContinuation::ReturnToParent
    );
    assert_eq!(tasks[0].status, AgentTaskStatus::Cancelled);
    let invocations = repository
        .list_invocations(&run.id)
        .await
        .expect("list invocations");
    assert!(
        invocations.iter().all(|invocation| invocation.kind
            != crate::domain::models::agent::AgentInvocationKind::Handoff)
    );

    let requests = model_gateway.requests().await;
    assert_eq!(requests[0].provider_state["invocationId"], "inv_root");
    let root_round_two = requests
        .iter()
        .filter(|request| request.provider_state["invocationId"].as_str() == Some("inv_root"))
        .nth(1)
        .expect("root second request");
    let root_round_two_results = tool_results_from_request(root_round_two);
    let handoff_result = root_round_two_results
        .iter()
        .find(|result| result.name == "agent.handoff")
        .expect("handoff tool result");
    assert!(handoff_result.is_error);
    assert_eq!(
        handoff_result.error_code.as_deref(),
        Some("agent.handoff_pending_tasks")
    );
    assert!(handoff_result.content.contains(
        "You still have unfinished delegated tasks. Use agent.await before handing off."
    ));
    assert!(!handoff_result.content.contains("this Agent"));
    assert!(!handoff_result.content.contains("terminal"));
    let events = repository
        .read_events(
            &run.id,
            AgentRunEventReadQuery {
                after_seq: Some(0),
                before_seq: None,
                limit: 300,
                invocation_id: None,
            },
        )
        .await
        .expect("read events");
    assert!(
        events
            .iter()
            .any(|event| event.event_type == "agent_handoff_requested")
    );
    assert!(
        events
            .iter()
            .all(|event| event.event_type != "agent_handoff_accepted")
    );

    tokio::fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn child_ref_profile_prompt_assembly_round_trips_through_host_bridge() {
    let root = std::env::temp_dir().join(format!(
        "tauritavern-agent-child-prompt-assembly-{}",
        Uuid::new_v4().simple()
    ));
    let repository = Arc::new(FileAgentRepository::new(root.clone()));
    let preset_repository = Arc::new(StaticPresetRepository::openai(
        "Child Preset",
        json!({
            "chat_completion_source": "openai",
            "openai_model": "preset-model"
        }),
    ));
    let agent_profile_repository =
        Arc::new(FileAgentProfileRepository::new(root.join("agent-profiles")));
    let profile_service = Arc::new(AgentProfileService::new(
        agent_profile_repository.clone(),
        agent_profile_repository,
        preset_repository.clone(),
    ));
    let llm_connection_service = test_llm_connection_service(&root);
    let prompt_assembly_service = Arc::new(PromptAssemblyService::new(
        profile_service.clone(),
        preset_repository,
        llm_connection_service.clone(),
    ));
    let service = Arc::new(AgentRuntimeService::new_with_prompt_assembly_service(
        repository.clone(),
        repository.clone(),
        repository.clone(),
        repository.clone(),
        test_chat_repository(&root),
        test_chat_repository(&root),
        test_skill_service(&root),
        Arc::new(MockAgentModelGateway::new(vec![])),
        profile_service.clone(),
        llm_connection_service,
        prompt_assembly_service,
    ));
    let mut child_profile = profile_service
        .resolve_profile(AgentProfileResolveInput {
            profile_id: None,
            known_tools: service.tool_specs(),
        })
        .await
        .expect("resolve child profile");
    child_profile.id = AgentProfileId::parse("scene-critic").expect("profile id");
    child_profile.preset.mode = AgentPresetBindingMode::Ref;
    child_profile.preset.ref_ = Some(AgentPresetRef {
        api_id: "openai".to_string(),
        name: "Child Preset".to_string(),
    });
    child_profile.preset.required = true;
    child_profile.run.presentation = AgentRunPresentation::Background;

    let run = AgentRun {
        id: "run_child_prompt_assembly_test".to_string(),
        workspace_id: "chat_child_prompt_assembly_test".to_string(),
        stable_chat_id: "stable_child_prompt_assembly_test".to_string(),
        chat_ref: AgentChatRef::Character {
            character_id: "Seraphina".to_string(),
            file_name: "Seraphina.png".to_string(),
        },
        generation_type: "normal".to_string(),
        profile_id: None,
        skill_scope_refs: Default::default(),
        persist_base_state_id: None,
        input_message_count: None,
        presentation: AgentRunPresentation::Background,
        status: AgentRunStatus::Created,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    repository.create_run(&run).await.expect("create run");
    let root_profile = test_resolved_profile(&root).await;
    let manifest = build_agent_manifest(&run, &root_profile);
    let frozen_snapshot = json!({
        "schemaVersion": 1,
        "kind": "tauritavern.agentFrozenRunInputSnapshot",
        "generationType": "normal",
        "promptInputs": {
            "type": "normal",
            "messages": []
        },
        "worldInfoActivation": { "entries": [] },
        "macroContext": {}
    });
    let root_prompt_snapshot = json!({
        "contextPolicy": child_profile.context.clone(),
        "frozenRunInputSnapshot": frozen_snapshot.clone(),
        "chatCompletionPayload": {
            "chat_completion_source": "openai",
            "model": "parent-model",
            "messages": prompt_messages("parent prompt")
        }
    });
    repository
        .initialize_run(&run, &manifest, &root_prompt_snapshot, &root_profile)
        .await
        .expect("initialize workspace");

    let visible_tools = service
        .visible_tool_specs_for_invocation(
            &child_profile,
            AgentInvocationExitPolicy::TaskReturnRequired,
        )
        .expect("visible tools");
    let (cancel_sender, mut cancel_receiver) = watch::channel(false);
    let run_id = run.id.clone();
    let service_for_task = service.clone();
    let child_profile_for_task = child_profile.clone();
    let visible_tools_for_task = visible_tools.clone();
    let frozen_for_task = frozen_snapshot.clone();
    let assembly = tokio::spawn(async move {
        service_for_task
            .assemble_invocation_prompt_snapshot(
                run_id.as_str(),
                "inv_child_prompt_assembly",
                &child_profile_for_task,
                &visible_tools_for_task,
                "normal",
                frozen_for_task,
                AgentPromptAssemblyScopeDto {
                    run_id: run_id.clone(),
                    invocation_id: "inv_child_prompt_assembly".to_string(),
                    invocation_kind: "subagent".to_string(),
                    parent_invocation_id: Some("inv_root".to_string()),
                    task_id: Some("task_child_prompt_assembly".to_string()),
                    exit_policy: Some("taskReturnRequired".to_string()),
                },
                "# Delegated Task\n\nReview the scene.".to_string(),
                &mut cancel_receiver,
            )
            .await
    });

    let payload = wait_for_event_payload(
        repository.clone(),
        run.id.clone(),
        "prompt_assembly_requested",
    )
    .await;
    let assembly_id = payload["assemblyId"]
        .as_str()
        .expect("assembly id")
        .to_string();
    assert_eq!(
        payload["scope"]["invocationId"],
        "inv_child_prompt_assembly"
    );
    assert_eq!(payload["scope"]["taskId"], "task_child_prompt_assembly");
    assert_eq!(
        payload["eventScope"]["invocationId"],
        "inv_child_prompt_assembly"
    );
    assert!(payload.get("request").is_none());
    assert_eq!(
        payload["requestKind"],
        "tauritavern.agentPromptAssemblyRequest"
    );
    assert_eq!(payload["requestSchemaVersion"], 1);
    assert!(
        payload["requestFingerprint"]["presetSha256"]
            .as_str()
            .expect("preset fingerprint")
            .starts_with("sha256:")
    );
    let request = service
        .read_prompt_assembly_request(AgentReadPromptAssemblyRequestDto {
            run_id: run.id.clone(),
            assembly_id: assembly_id.clone(),
        })
        .await
        .expect("read prompt assembly request");
    assert_eq!(request.preset_ref.name, "Child Preset");
    assert_eq!(request.settings["openai_model"], "preset-model");
    assert_eq!(
        request.required_agent_prompt_components,
        vec!["agentSystemPrompt".to_string(), "agentTask".to_string()]
    );
    assert!(
        request
            .agent_task_prompt
            .as_deref()
            .expect("task prompt")
            .contains("Review the scene.")
    );

    let malformed_error = service
        .resolve_prompt_assembly(AgentResolvePromptAssemblyDto {
            run_id: run.id.clone(),
            assembly_id: assembly_id.clone(),
            prompt_snapshot: None,
            frozen_run_input_snapshot: None,
            generation_intent: None,
            assembly: None,
            error: None,
        })
        .await
        .expect_err("malformed success resolve should fail before consuming pending request");
    assert!(
        malformed_error
            .to_string()
            .contains("agent.prompt_assembly_snapshot_required")
    );
    let request_after_malformed = service
        .read_prompt_assembly_request(AgentReadPromptAssemblyRequestDto {
            run_id: run.id.clone(),
            assembly_id: assembly_id.clone(),
        })
        .await
        .expect("malformed resolve must leave prompt assembly pending");
    assert_eq!(
        request_after_malformed.fingerprint.preset_sha256,
        request.fingerprint.preset_sha256
    );

    service
        .resolve_prompt_assembly(AgentResolvePromptAssemblyDto {
            run_id: run.id.clone(),
            assembly_id,
            prompt_snapshot: Some(json!({
                "contextPolicy": child_profile.context.clone(),
                "chatCompletionPayload": {
                    "chat_completion_source": "openai",
                    "model": "assembled-child-model",
                    "messages": [
                        {
                            "role": "system",
                            "content": "Assembled child system prompt."
                        },
                        {
                            "role": "user",
                            "content": "Assembled child task prompt."
                        }
                    ]
                }
            })),
            frozen_run_input_snapshot: Some(frozen_snapshot),
            generation_intent: Some(json!({ "source": "test" })),
            assembly: Some(json!({ "engine": "test" })),
            error: None,
        })
        .await
        .expect("resolve prompt assembly");

    let prompt_snapshot = assembly
        .await
        .expect("join assembly")
        .expect("assemble prompt")
        .expect("child prompt snapshot");
    drop(cancel_sender);
    assert_eq!(
        prompt_snapshot["chatCompletionPayload"]["messages"][1]["content"],
        "Assembled child task prompt."
    );
    repository
        .read_text(
            &run.id,
            &WorkspacePath::parse(
                "input/invocations/inv_child_prompt_assembly/prompt_snapshot.json",
            )
            .unwrap(),
        )
        .await
        .expect("stored child prompt snapshot");
    let events = repository
        .read_events(
            &run.id,
            AgentRunEventReadQuery {
                after_seq: Some(0),
                before_seq: None,
                limit: 100,
                invocation_id: None,
            },
        )
        .await
        .expect("read events");
    assert!(
        events
            .iter()
            .any(|event| event.event_type == "prompt_assembly_completed")
    );

    tokio::fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn direct_start_rejects_subagent_only_profile() {
    let root = std::env::temp_dir().join(format!(
        "tauritavern-agent-subagent-only-start-{}",
        Uuid::new_v4().simple()
    ));
    let repository = Arc::new(FileAgentRepository::new(root.clone()));
    let profile_service = test_profile_service(&root);
    let mut child_profile = profile_service
        .load_profile("default-writer")
        .await
        .expect("load default profile")
        .expect("default profile exists");
    child_profile.id = AgentProfileId::parse("subagent-only").expect("profile id");
    child_profile.display_name = "SubAgent Only".to_string();
    child_profile.run.presentation = AgentRunPresentation::Background;
    child_profile.run.direct_runnable = false;
    child_profile.tools.allow.retain(|name| {
        !matches!(
            name.as_str(),
            "workspace.commit"
                | "workspace.finish"
                | "agent.list"
                | "agent.delegate"
                | "agent.await"
        )
    });
    child_profile.delegation = AgentDelegationPolicy {
        callable: true,
        allow_as_subagent: true,
        allowed_callers: vec!["default-writer".to_string()],
        description_for_agents: Some("Return concise notes.".to_string()),
        ..Default::default()
    };

    let service = Arc::new(AgentRuntimeService::new(
        repository.clone(),
        repository.clone(),
        repository.clone(),
        repository.clone(),
        test_chat_repository(&root),
        test_chat_repository(&root),
        test_skill_service(&root),
        Arc::new(MockAgentModelGateway::new(Vec::new())),
        profile_service.clone(),
        test_llm_connection_service(&root),
    ));
    profile_service
        .save_profile(child_profile, service.tool_specs())
        .await
        .expect("save subagent-only profile");

    let error = service
        .start_run(AgentStartRunDto {
            chat_ref: AgentChatRef::Character {
                character_id: "Seraphina".to_string(),
                file_name: "Seraphina.png".to_string(),
            },
            stable_chat_id: "stable_subagent_only_start".to_string(),
            generation_type: "normal".to_string(),
            profile_id: Some("subagent-only".to_string()),
            persist_base_state_id: None,
            prompt_snapshot: Some(json!({
                "chatCompletionPayload": {
                    "chat_completion_source": "openai",
                    "model": "test-model",
                    "messages": prompt_messages("direct start should be rejected")
                }
            })),
            frozen_run_input_snapshot: None,
            generation_intent: None,
            skill_scope_refs: Default::default(),
            options: AgentStartRunOptionsDto::default(),
        })
        .await
        .expect_err("subagent-only profile must not start directly");

    assert!(
        error
            .to_string()
            .contains("agent.profile_not_direct_runnable")
    );
    tokio::fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn direct_start_rejects_requires_configuration_profile() {
    let root = std::env::temp_dir().join(format!(
        "tauritavern-agent-unconfigured-start-{}",
        Uuid::new_v4().simple()
    ));
    let repository = Arc::new(FileAgentRepository::new(root.clone()));
    let profile_service = test_profile_service(&root);
    let service = Arc::new(AgentRuntimeService::new(
        repository.clone(),
        repository.clone(),
        repository.clone(),
        repository.clone(),
        test_chat_repository(&root),
        test_chat_repository(&root),
        test_skill_service(&root),
        Arc::new(MockAgentModelGateway::new(Vec::new())),
        profile_service.clone(),
        test_llm_connection_service(&root),
    ));

    let mut profile = profile_service
        .load_profile("default-writer")
        .await
        .expect("load default profile")
        .expect("default profile exists");
    profile.id = AgentProfileId::parse("imported-writer").expect("profile id");
    profile.display_name = "Imported Writer".to_string();
    profile.model.mode = AgentModelBindingMode::RequiresConfiguration;
    profile.model.connection_ref = None;
    profile.model.model_id = None;
    profile_service
        .save_profile(profile, service.tool_specs())
        .await
        .expect("save unconfigured profile");

    let error = service
        .start_run(AgentStartRunDto {
            chat_ref: AgentChatRef::Character {
                character_id: "Seraphina".to_string(),
                file_name: "Seraphina.png".to_string(),
            },
            stable_chat_id: "stable_unconfigured_start".to_string(),
            generation_type: "normal".to_string(),
            profile_id: Some("imported-writer".to_string()),
            persist_base_state_id: None,
            prompt_snapshot: Some(json!({
                "chatCompletionPayload": {
                    "chat_completion_source": "openai",
                    "model": "test-model",
                    "messages": prompt_messages("direct start should require local model")
                }
            })),
            frozen_run_input_snapshot: None,
            generation_intent: None,
            skill_scope_refs: Default::default(),
            options: AgentStartRunOptionsDto::default(),
        })
        .await
        .expect_err("requiresConfiguration profile must not start directly");

    assert!(
        error
            .to_string()
            .contains("agent.profile_model_requires_configuration")
    );
    tokio::fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn completed_child_results_are_added_to_next_parent_turn_once() {
    let root = std::env::temp_dir().join(format!(
        "tauritavern-agent-inbox-{}",
        Uuid::new_v4().simple()
    ));
    let repository = Arc::new(FileAgentRepository::new(root.clone()));
    let service = Arc::new(AgentRuntimeService::new(
        repository.clone(),
        repository.clone(),
        repository.clone(),
        repository.clone(),
        test_chat_repository(&root),
        test_chat_repository(&root),
        test_skill_service(&root),
        Arc::new(MockAgentModelGateway::new(vec![])),
        test_profile_service(&root),
        test_llm_connection_service(&root),
    ));
    let run = AgentRun {
        id: "run_inbox_test".to_string(),
        workspace_id: "chat_inbox_test".to_string(),
        stable_chat_id: "stable_inbox_test".to_string(),
        chat_ref: AgentChatRef::Character {
            character_id: "Seraphina".to_string(),
            file_name: "Seraphina.png".to_string(),
        },
        generation_type: "normal".to_string(),
        profile_id: None,
        skill_scope_refs: Default::default(),
        persist_base_state_id: None,
        input_message_count: None,
        presentation: AgentRunPresentation::Background,
        status: AgentRunStatus::Created,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    repository.create_run(&run).await.expect("create run");
    let task = service
        .create_child_task(
            &run.id,
            "inv_root",
            "inv_child_inbox".to_string(),
            "task_child_inbox".to_string(),
            "scene-critic".to_string(),
            "scene-critic".to_string(),
            "call_delegate_inbox".to_string(),
            json!({
                "title": "Critique",
                "objective": "Return one note."
            }),
            None,
        )
        .await
        .expect("create task");
    let result_path = WorkspacePath::parse("agent-results/inv_child_inbox.json").unwrap();
    repository
        .write_text(
            &run.id,
            &result_path,
            &serde_json::to_string_pretty(&json!({
                "summary": "The scene needs a stronger image.",
                "result": {
                    "findings": [{ "text": "Add a concrete image." }],
                    "suggestedNextActions": ["Revise the opening sentence."]
                }
            }))
            .unwrap(),
        )
        .await
        .expect("write result");
    service
        .transition_child_task(
            &run.id,
            task.id.as_str(),
            AgentTaskStatus::Completed,
            Some(result_path.as_str().to_string()),
            None,
        )
        .await
        .expect("complete task");

    let mut profile = test_resolved_profile(&root).await;
    profile.run.presentation = AgentRunPresentation::Foreground;
    let mut seen = HashSet::new();
    let message = service
        .completed_child_results_message(&run.id, "inv_root", &mut seen, &profile, 0)
        .await
        .expect("build inbox message")
        .expect("message");
    assert!(message.contains("Delegated task results are now available"));
    assert!(message.contains("Review them before deciding your next action"));
    assert!(message.contains("The scene needs a stronger image."));
    assert!(message.contains("Revise the opening sentence."));
    assert!(message.contains("Treat these delegated results as context for you"));
    assert!(message.contains("call workspace_commit, then call workspace_finish"));
    assert!(message.contains("do not answer in plain text"));
    assert!(seen.contains("task_child_inbox"));

    let mut seen_after_commit = HashSet::new();
    let post_commit_message = service
        .completed_child_results_message(&run.id, "inv_root", &mut seen_after_commit, &profile, 1)
        .await
        .expect("build post-commit inbox message")
        .expect("post-commit message");
    assert!(post_commit_message.contains("current committed reply already accounts"));
    assert!(
        post_commit_message.contains("call workspace_commit again, then call workspace_finish")
    );

    assert!(
        service
            .completed_child_results_message(&run.id, "inv_root", &mut seen, &profile, 0)
            .await
            .expect("build second inbox message")
            .is_none()
    );

    tokio::fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn cancelled_child_task_does_not_emit_failed_event() {
    let root = std::env::temp_dir().join(format!(
        "tauritavern-agent-child-cancel-event-{}",
        Uuid::new_v4().simple()
    ));
    let repository = Arc::new(FileAgentRepository::new(root.clone()));
    let service = Arc::new(AgentRuntimeService::new(
        repository.clone(),
        repository.clone(),
        repository.clone(),
        repository.clone(),
        test_chat_repository(&root),
        test_chat_repository(&root),
        test_skill_service(&root),
        Arc::new(MockAgentModelGateway::new(vec![])),
        test_profile_service(&root),
        test_llm_connection_service(&root),
    ));
    let run = AgentRun {
        id: "run_child_cancel_event_test".to_string(),
        workspace_id: "chat_child_cancel_event_test".to_string(),
        stable_chat_id: "stable_child_cancel_event_test".to_string(),
        chat_ref: AgentChatRef::Character {
            character_id: "Seraphina".to_string(),
            file_name: "Seraphina.png".to_string(),
        },
        generation_type: "normal".to_string(),
        profile_id: None,
        skill_scope_refs: Default::default(),
        persist_base_state_id: None,
        input_message_count: None,
        presentation: AgentRunPresentation::Background,
        status: AgentRunStatus::Created,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    repository.create_run(&run).await.expect("create run");
    let task = service
        .create_child_task(
            &run.id,
            "inv_root",
            "inv_child_cancel_event".to_string(),
            "task_child_cancel_event".to_string(),
            "scene-critic".to_string(),
            "scene-critic".to_string(),
            "call_delegate_cancel_event".to_string(),
            json!({
                "title": "Long critique",
                "objective": "This task should be cancelled before it starts."
            }),
            None,
        )
        .await
        .expect("create task");
    let (cancel_sender, mut cancel_receiver) = watch::channel(false);
    cancel_sender.send(true).expect("send cancel");

    service
        .run_child_task_to_terminal(
            &run.id,
            task.id.as_str(),
            task.child_invocation_id.as_str(),
            &mut cancel_receiver,
        )
        .await
        .expect("cancel child task");

    let task = repository
        .load_task(&run.id, task.id.as_str())
        .await
        .expect("load task");
    assert_eq!(task.status, AgentTaskStatus::Cancelled);
    let child_invocation = repository
        .load_invocation(&run.id, "inv_child_cancel_event")
        .await
        .expect("load child invocation");
    assert_eq!(child_invocation.status, AgentInvocationStatus::Cancelled);
    let events = repository
        .read_events(
            &run.id,
            AgentRunEventReadQuery {
                after_seq: Some(0),
                before_seq: None,
                limit: 100,
                invocation_id: None,
            },
        )
        .await
        .expect("read events");
    assert!(
        events
            .iter()
            .all(|event| event.event_type != "agent_child_invocation_failed")
    );
    assert!(
        events
            .iter()
            .any(|event| event.event_type == "agent_task_cancelled")
    );

    tokio::fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn workspace_finish_cancels_unawaited_delegated_task() {
    let root = std::env::temp_dir().join(format!(
        "tauritavern-agent-finish-cancels-subagent-{}",
        Uuid::new_v4().simple()
    ));
    let repository = Arc::new(FileAgentRepository::new(root.clone()));
    let profile_service = test_profile_service(&root);
    let mut child_profile = profile_service
        .load_profile("default-writer")
        .await
        .expect("load default profile")
        .expect("default profile exists");
    child_profile.id = AgentProfileId::parse("scene-critic").expect("profile id");
    child_profile.display_name = "Scene Critic".to_string();
    child_profile.description = Some("Reviews a scene and returns concise notes.".to_string());
    child_profile.tools.allow.retain(|name| {
        !matches!(
            name.as_str(),
            "agent.list" | "agent.delegate" | "agent.await"
        )
    });
    child_profile.delegation = AgentDelegationPolicy {
        callable: true,
        allow_as_subagent: true,
        allowed_callers: vec!["default-writer".to_string()],
        description_for_agents: Some("Return concise scene critique.".to_string()),
        ..Default::default()
    };

    let model_gateway = Arc::new(FinishCancelsDelegateModelGateway::new());
    let service = Arc::new(AgentRuntimeService::new(
        repository.clone(),
        repository.clone(),
        repository.clone(),
        repository.clone(),
        test_chat_repository(&root),
        test_chat_repository(&root),
        test_skill_service(&root),
        model_gateway.clone(),
        profile_service.clone(),
        test_llm_connection_service(&root),
    ));
    profile_service
        .save_profile(child_profile, service.tool_specs())
        .await
        .expect("save child profile");
    let run = AgentRun {
        id: "run_finish_cancels_subagent_test".to_string(),
        workspace_id: "chat_finish_cancels_subagent_test".to_string(),
        stable_chat_id: "stable_finish_cancels_subagent_test".to_string(),
        chat_ref: AgentChatRef::Character {
            character_id: "Seraphina".to_string(),
            file_name: "Seraphina.png".to_string(),
        },
        generation_type: "normal".to_string(),
        profile_id: None,
        skill_scope_refs: Default::default(),
        persist_base_state_id: None,
        input_message_count: None,
        presentation: AgentRunPresentation::Background,
        status: AgentRunStatus::Created,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    repository.create_run(&run).await.expect("create run");
    insert_active_run_handle(&service, &run.id).await;

    let request = ChatCompletionGenerateRequestDto {
        payload: json!({
            "chat_completion_source": "openai",
            "model": "test-model",
            "messages": prompt_messages("ask a critic, then finish without awaiting")
        })
        .as_object()
        .cloned()
        .unwrap(),
    };
    let prompt_snapshot = json!({ "chatCompletionPayload": request.payload.clone() });
    let (_cancel_sender, mut cancel_receiver) = watch::channel(false);
    let profile = test_resolved_profile(&root).await;

    service
        .execute_agent_loop_run_inner(
            &run.id,
            prompt_snapshot,
            request,
            profile,
            &mut cancel_receiver,
        )
        .await
        .expect("agent loop");
    model_gateway.wait_for_child_cancelled().await;
    tokio::task::yield_now().await;

    let saved = repository.load_run(&run.id).await.expect("load run");
    assert_eq!(saved.status, AgentRunStatus::Completed);
    let tasks = repository.list_tasks(&run.id).await.expect("list tasks");
    assert_eq!(tasks.len(), 1);
    let task = &tasks[0];
    assert_eq!(task.status, AgentTaskStatus::Cancelled);
    let child_invocation = repository
        .load_invocation(&run.id, task.child_invocation_id.as_str())
        .await
        .expect("load child invocation");
    assert_eq!(child_invocation.status, AgentInvocationStatus::Cancelled);
    let requests = model_gateway.requests().await;
    assert!(
        requests
            .iter()
            .any(|request| { request.provider_state["invocationId"].as_str() == Some("inv_root") })
    );
    assert!(requests.iter().any(|request| {
        request.provider_state["invocationId"].as_str() == Some(task.child_invocation_id.as_str())
    }));
    let events = repository
        .read_events(
            &run.id,
            AgentRunEventReadQuery {
                after_seq: Some(0),
                before_seq: None,
                limit: 200,
                invocation_id: None,
            },
        )
        .await
        .expect("read events");
    assert!(
        events
            .iter()
            .any(|event| event.event_type == "agent_task_cancelled")
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event.event_type == "agent_invocation_cancelled")
            .count(),
        1
    );
    assert!(
        events
            .iter()
            .any(|event| event.event_type == "run_completed")
    );
    assert!(
        events
            .iter()
            .all(|event| event.event_type != "agent_child_invocation_failed")
    );

    tokio::fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn scheduler_cancels_unfinished_child_tasks_when_parent_finishes() {
    let root = std::env::temp_dir().join(format!(
        "tauritavern-agent-scheduler-cancel-{}",
        Uuid::new_v4().simple()
    ));
    let repository = Arc::new(FileAgentRepository::new(root.clone()));
    let service = Arc::new(AgentRuntimeService::new(
        repository.clone(),
        repository.clone(),
        repository.clone(),
        repository.clone(),
        test_chat_repository(&root),
        test_chat_repository(&root),
        test_skill_service(&root),
        Arc::new(MockAgentModelGateway::new(vec![])),
        test_profile_service(&root),
        test_llm_connection_service(&root),
    ));
    let run = AgentRun {
        id: "run_scheduler_cancel_test".to_string(),
        workspace_id: "chat_scheduler_cancel_test".to_string(),
        stable_chat_id: "stable_scheduler_cancel_test".to_string(),
        chat_ref: AgentChatRef::Character {
            character_id: "Seraphina".to_string(),
            file_name: "Seraphina.png".to_string(),
        },
        generation_type: "normal".to_string(),
        profile_id: None,
        skill_scope_refs: Default::default(),
        persist_base_state_id: None,
        input_message_count: None,
        presentation: AgentRunPresentation::Background,
        status: AgentRunStatus::Created,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    repository.create_run(&run).await.expect("create run");
    let active_handle = insert_active_run_handle(&service, &run.id).await;
    let task = service
        .create_child_task(
            &run.id,
            "inv_root",
            "inv_child_cancel".to_string(),
            "task_child_cancel".to_string(),
            "scene-critic".to_string(),
            "scene-critic".to_string(),
            "call_delegate_cancel".to_string(),
            json!({
                "title": "Long critique",
                "objective": "Keep working until cancelled."
            }),
            None,
        )
        .await
        .expect("create task");

    active_handle
        .scheduler
        .cancel_unfinished_for_parent("inv_root")
        .await
        .expect("cancel child tasks");

    let task = repository
        .load_task(&run.id, task.id.as_str())
        .await
        .expect("load task");
    assert_eq!(task.status, AgentTaskStatus::Cancelled);
    let invocation = repository
        .load_invocation(&run.id, "inv_child_cancel")
        .await
        .expect("load child invocation");
    assert_eq!(invocation.status, AgentInvocationStatus::Cancelled);

    tokio::fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn agent_loop_writes_artifact_and_completes() {
    let root = std::env::temp_dir().join(format!(
        "tauritavern-agent-loop-{}",
        Uuid::new_v4().simple()
    ));
    let repository = Arc::new(FileAgentRepository::new(root.clone()));
    let run = AgentRun {
        id: "run_loop_test".to_string(),
        workspace_id: "chat_loop_test".to_string(),
        stable_chat_id: "stable_loop_test".to_string(),
        chat_ref: AgentChatRef::Character {
            character_id: "Seraphina".to_string(),
            file_name: "Seraphina.png".to_string(),
        },
        generation_type: "normal".to_string(),
        profile_id: None,
        skill_scope_refs: Default::default(),
        persist_base_state_id: None,
        input_message_count: None,
        presentation: AgentRunPresentation::Background,
        status: AgentRunStatus::Created,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    repository.create_run(&run).await.expect("create run");

    let model_gateway = Arc::new(MockAgentModelGateway::new(vec![
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "I will write the artifact.",
                    "reasoning_content": "Need to create output/main.md.",
                    "tool_calls": [{
                        "id": "call_write",
                        "type": "function",
                        "function": {
                            "name": "workspace_write_file",
                            "arguments": "{\"path\":\"output/main.md\",\"content\":\"hello from loop\"}"
                        }
                    }]
                }
            }]
        }),
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_finish",
                        "type": "function",
                        "function": {
                            "name": "workspace_finish",
                            "arguments": "{}"
                        }
                    }]
                }
            }]
        }),
    ]));
    let model_gateway_probe = model_gateway.clone();

    let service = AgentRuntimeService::new(
        repository.clone(),
        repository.clone(),
        repository.clone(),
        repository.clone(),
        test_chat_repository(&root),
        test_chat_repository(&root),
        test_skill_service(&root),
        model_gateway,
        test_profile_service(&root),
        test_llm_connection_service(&root),
    );
    let request = ChatCompletionGenerateRequestDto {
        payload: json!({
            "chat_completion_source": "openai",
            "model": "test-model",
            "messages": prompt_messages("write a message")
        })
        .as_object()
        .cloned()
        .unwrap(),
    };
    let prompt_snapshot = json!({ "chatCompletionPayload": request.payload.clone() });
    let (_cancel_sender, mut cancel_receiver) = watch::channel(false);
    let profile = test_resolved_profile(&root).await;

    service
        .execute_agent_loop_run_inner(
            &run.id,
            prompt_snapshot,
            request,
            profile,
            &mut cancel_receiver,
        )
        .await
        .expect("agent loop");

    let saved = repository.load_run(&run.id).await.expect("load run");
    assert_eq!(saved.status, AgentRunStatus::Completed);
    let root_invocation = repository
        .load_invocation(&run.id, "inv_root")
        .await
        .expect("load root invocation");
    assert_eq!(root_invocation.status, AgentInvocationStatus::Completed);
    assert_eq!(root_invocation.profile_id, "default-writer");

    let artifact = repository
        .read_text(&run.id, &WorkspacePath::parse("output/main.md").unwrap())
        .await
        .expect("read artifact");
    assert_eq!(artifact.text, "hello from loop");

    let stored_response = repository
        .read_text(
            &run.id,
            &WorkspacePath::parse("model-responses/round-001.json").unwrap(),
        )
        .await
        .expect("read stored model response");
    let stored_response: Value =
        serde_json::from_str(&stored_response.text).expect("stored response JSON");
    assert_eq!(stored_response["round"], json!(1));
    assert!(stored_response["response"]["rawResponse"]["choices"].is_array());

    let model_turn = service
        .read_model_turn(AgentReadModelTurnDto {
            run_id: run.id.clone(),
            invocation_id: None,
            round: 1,
            max_chars: 40_000,
        })
        .await
        .expect("read model turn");
    assert_eq!(model_turn.assistant.text, "I will write the artifact.");
    assert_eq!(model_turn.assistant.total_chars, 26);
    assert_eq!(model_turn.assistant.total_words, 5);
    assert!(!model_turn.assistant.truncated);
    assert_eq!(model_turn.reasoning.len(), 1);
    assert_eq!(
        model_turn.reasoning[0].text,
        "Need to create output/main.md."
    );
    assert_eq!(model_turn.reasoning[0].total_chars, 30);
    assert_eq!(model_turn.reasoning[0].total_words, 6);
    assert_eq!(model_turn.reasoning[0].source, "reasoning_content");
    assert_eq!(model_turn.tool_calls.len(), 1);
    assert_eq!(model_turn.tool_calls[0].call_id, "call_write");
    assert_eq!(model_turn.tool_calls[0].name, "workspace.write_file");

    let truncated_model_turn = service
        .read_model_turn(AgentReadModelTurnDto {
            run_id: run.id.clone(),
            invocation_id: None,
            round: 1,
            max_chars: 4,
        })
        .await
        .expect("read truncated model turn");
    assert_eq!(truncated_model_turn.assistant.text, "I wi");
    assert_eq!(truncated_model_turn.assistant.total_chars, 26);
    assert_eq!(truncated_model_turn.assistant.total_words, 5);
    assert!(truncated_model_turn.assistant.truncated);

    let model_requests = model_gateway_probe.requests().await;
    let second_request = model_requests.get(1).expect("second model request");
    let write_tool_result = second_request
        .messages
        .iter()
        .find(|message| message.role == AgentModelRole::Tool)
        .and_then(|message| message.parts.first())
        .and_then(|part| match part {
            AgentModelContentPart::ToolResult { result } => Some(result),
            _ => None,
        })
        .expect("write tool result");
    assert_eq!(
        write_tool_result.content,
        "Wrote 15 chars / 3 words to output/main.md."
    );
    assert!(!write_tool_result.content.contains("hello from loop"));
    wait_for_closed_sessions(
        &model_gateway_probe,
        vec!["run_loop_test:inv_root".to_string()],
    )
    .await;

    let events = repository
        .read_events(
            &run.id,
            AgentRunEventReadQuery {
                after_seq: Some(0),
                before_seq: None,
                limit: 100,
                invocation_id: None,
            },
        )
        .await
        .expect("read events");
    assert!(
        events
            .iter()
            .any(|event| event.event_type == "agent_loop_finished")
    );
    assert!(
        events
            .iter()
            .any(|event| event.event_type == "model_response_stored")
    );
    let model_completed = events
        .iter()
        .find(|event| event.event_type == "model_completed" && event.payload["round"] == json!(1))
        .expect("model completed event");
    assert_eq!(model_completed.payload["hasAssistantText"], json!(true));
    assert_eq!(model_completed.payload["hasReasoning"], json!(true));
    assert_eq!(model_completed.payload["assistantTextChars"], json!(26));
    assert_eq!(model_completed.payload["assistantTextWords"], json!(5));

    let tool_requested = events
        .iter()
        .find(|event| {
            event.event_type == "tool_call_requested"
                && event.payload["callId"].as_str() == Some("call_write")
        })
        .expect("tool call requested");
    assert_eq!(tool_requested.payload["round"], json!(1));
    let written = events
        .iter()
        .find(|event| event.event_type == "workspace_file_written")
        .expect("workspace write event");
    assert_eq!(written.payload["mode"], json!("replace"));

    tokio::fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn agent_loop_explicit_read_after_append_unlocks_rewrite() {
    let root = std::env::temp_dir().join(format!(
        "tauritavern-agent-append-read-{}",
        Uuid::new_v4().simple()
    ));
    let repository = Arc::new(FileAgentRepository::new(root.clone()));
    let run = AgentRun {
        id: "run_append_read_test".to_string(),
        workspace_id: "chat_append_read_test".to_string(),
        stable_chat_id: "stable_append_read_test".to_string(),
        chat_ref: AgentChatRef::Character {
            character_id: "Seraphina".to_string(),
            file_name: "Seraphina.png".to_string(),
        },
        generation_type: "normal".to_string(),
        profile_id: None,
        skill_scope_refs: Default::default(),
        persist_base_state_id: None,
        input_message_count: None,
        presentation: AgentRunPresentation::Background,
        status: AgentRunStatus::Created,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    repository.create_run(&run).await.expect("create run");

    let request = ChatCompletionGenerateRequestDto {
        payload: json!({
            "chat_completion_source": "openai",
            "model": "test-model",
            "messages": prompt_messages("append then rewrite")
        })
        .as_object()
        .cloned()
        .unwrap(),
    };
    let prompt_snapshot = json!({ "chatCompletionPayload": request.payload.clone() });
    let profile = test_resolved_profile(&root).await;
    repository
        .initialize_run(
            &run,
            &build_agent_manifest(&run, &profile),
            &prompt_snapshot,
            &profile,
        )
        .await
        .expect("initialize workspace");
    repository
        .write_text(
            &run.id,
            &WorkspacePath::parse("output/main.md").unwrap(),
            "first",
        )
        .await
        .expect("seed artifact");

    let model_gateway = Arc::new(MockAgentModelGateway::new(vec![
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_append",
                        "type": "function",
                        "function": {
                            "name": "workspace_write_file",
                            "arguments": "{\"path\":\"output/main.md\",\"content\":\"\\nsecond\",\"mode\":\"append\"}"
                        }
                    }]
                }
            }]
        }),
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_read_after_append",
                        "type": "function",
                        "function": {
                            "name": "workspace_read_file",
                            "arguments": "{\"path\":\"output/main.md\"}"
                        }
                    }]
                }
            }]
        }),
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_replace_after_read",
                        "type": "function",
                        "function": {
                            "name": "workspace_write_file",
                            "arguments": "{\"path\":\"output/main.md\",\"content\":\"rewritten after explicit read\",\"mode\":\"replace\"}"
                        }
                    }]
                }
            }]
        }),
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_finish",
                        "type": "function",
                        "function": {
                            "name": "workspace_finish",
                            "arguments": "{}"
                        }
                    }]
                }
            }]
        }),
    ]));
    let model_gateway_probe = model_gateway.clone();

    let service = AgentRuntimeService::new(
        repository.clone(),
        repository.clone(),
        repository.clone(),
        repository.clone(),
        test_chat_repository(&root),
        test_chat_repository(&root),
        test_skill_service(&root),
        model_gateway,
        test_profile_service(&root),
        test_llm_connection_service(&root),
    );
    let (_cancel_sender, mut cancel_receiver) = watch::channel(false);

    service
        .execute_agent_loop_run_inner(
            &run.id,
            prompt_snapshot,
            request,
            profile,
            &mut cancel_receiver,
        )
        .await
        .expect("agent loop");

    let artifact = repository
        .read_text(&run.id, &WorkspacePath::parse("output/main.md").unwrap())
        .await
        .expect("read artifact");
    assert_eq!(artifact.text, "rewritten after explicit read");

    let model_requests = model_gateway_probe.requests().await;
    let second_request = model_requests.get(1).expect("second model request");
    let append_tool_result = second_request
        .messages
        .iter()
        .find(|message| message.role == AgentModelRole::Tool)
        .and_then(|message| message.parts.first())
        .and_then(|part| match part {
            AgentModelContentPart::ToolResult { result } => Some(result),
            _ => None,
        })
        .expect("append result");
    assert_eq!(
        append_tool_result.content,
        "Appended 12 chars / 2 words to output/main.md."
    );
    assert!(!append_tool_result.content.contains("first\nsecond"));

    let third_request = model_requests.get(2).expect("third model request");
    let read_tool_result = third_request
        .messages
        .iter()
        .filter(|message| message.role == AgentModelRole::Tool)
        .filter_map(|message| message.parts.first())
        .find_map(|part| match part {
            AgentModelContentPart::ToolResult { result }
                if result.name == "workspace.read_file" =>
            {
                Some(result)
            }
            _ => None,
        })
        .expect("explicit read result");
    assert!(
        read_tool_result
            .content
            .contains("output/main.md lines 1-2 of 2")
    );
    assert!(read_tool_result.content.contains("first"));
    assert!(read_tool_result.content.contains("second"));
    wait_for_closed_sessions(
        &model_gateway_probe,
        vec!["run_append_read_test:inv_root".to_string()],
    )
    .await;

    tokio::fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn agent_loop_stores_tool_audit_files_with_hashed_call_id_paths() {
    let root = std::env::temp_dir().join(format!(
        "tauritavern-agent-tool-audit-{}",
        Uuid::new_v4().simple()
    ));
    let repository = Arc::new(FileAgentRepository::new(root.clone()));
    let run = AgentRun {
        id: "run_tool_audit_test".to_string(),
        workspace_id: "chat_tool_audit_test".to_string(),
        stable_chat_id: "stable_tool_audit_test".to_string(),
        chat_ref: AgentChatRef::Character {
            character_id: "Seraphina".to_string(),
            file_name: "Seraphina.png".to_string(),
        },
        generation_type: "normal".to_string(),
        profile_id: None,
        skill_scope_refs: Default::default(),
        persist_base_state_id: None,
        input_message_count: None,
        presentation: AgentRunPresentation::Background,
        status: AgentRunStatus::Created,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    repository.create_run(&run).await.expect("create run");

    let opaque_call_id = format!(
        "call_{}___thought__{}/{}\\{} {}",
        "A".repeat(240),
        "B".repeat(240),
        "C".repeat(240),
        "思考".repeat(80),
        "D".repeat(240)
    );
    let model_gateway = Arc::new(MockAgentModelGateway::new(vec![
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": opaque_call_id,
                        "type": "function",
                        "function": {
                            "name": "workspace_write_file",
                            "arguments": "{\"path\":\"output/main.md\",\"content\":\"opaque call id survived\"}"
                        }
                    }]
                }
            }]
        }),
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_finish_after_opaque_id",
                        "type": "function",
                        "function": {
                            "name": "workspace_finish",
                            "arguments": "{}"
                        }
                    }]
                }
            }]
        }),
    ]));
    let model_gateway_probe = model_gateway.clone();

    let service = AgentRuntimeService::new(
        repository.clone(),
        repository.clone(),
        repository.clone(),
        repository.clone(),
        test_chat_repository(&root),
        test_chat_repository(&root),
        test_skill_service(&root),
        model_gateway,
        test_profile_service(&root),
        test_llm_connection_service(&root),
    );
    let request = ChatCompletionGenerateRequestDto {
        payload: json!({
            "chat_completion_source": "openai",
            "model": "test-model",
            "messages": prompt_messages("write a message")
        })
        .as_object()
        .cloned()
        .unwrap(),
    };
    let prompt_snapshot = json!({ "chatCompletionPayload": request.payload.clone() });
    let (_cancel_sender, mut cancel_receiver) = watch::channel(false);
    let profile = test_resolved_profile(&root).await;

    service
        .execute_agent_loop_run_inner(
            &run.id,
            prompt_snapshot,
            request,
            profile,
            &mut cancel_receiver,
        )
        .await
        .expect("agent loop");

    let saved = repository.load_run(&run.id).await.expect("load run");
    assert_eq!(saved.status, AgentRunStatus::Completed);

    let events = repository
        .read_events(
            &run.id,
            AgentRunEventReadQuery {
                after_seq: Some(0),
                before_seq: None,
                limit: 100,
                invocation_id: None,
            },
        )
        .await
        .expect("read events");

    let arguments_ref = events
        .iter()
        .find(|event| {
            event.event_type == "tool_call_requested"
                && event.payload["callId"].as_str() == Some(opaque_call_id.as_str())
        })
        .and_then(|event| event.payload["argumentsRef"].as_str())
        .expect("arguments ref");
    assert_hashed_tool_audit_path(arguments_ref, "tool-args");

    let arguments_file = repository
        .read_text(&run.id, &WorkspacePath::parse(arguments_ref).unwrap())
        .await
        .expect("read arguments file");
    let arguments: Value = serde_json::from_str(&arguments_file.text).expect("arguments JSON");
    assert_eq!(arguments["path"], "output/main.md");

    let result_ref = events
        .iter()
        .find(|event| {
            event.event_type == "tool_result_stored"
                && event.payload["callId"].as_str() == Some(opaque_call_id.as_str())
        })
        .and_then(|event| event.payload["path"].as_str())
        .expect("result ref");
    assert_hashed_tool_audit_path(result_ref, "tool-results");

    let result_file = repository
        .read_text(&run.id, &WorkspacePath::parse(result_ref).unwrap())
        .await
        .expect("read result file");
    let result: Value = serde_json::from_str(&result_file.text).expect("result JSON");
    assert_eq!(result["callId"].as_str(), Some(opaque_call_id.as_str()));
    assert_eq!(result["structured"]["path"], "output/main.md");

    let model_requests = model_gateway_probe.requests().await;
    let second_request = model_requests.get(1).expect("second model request");
    let echoed_tool_result = second_request
        .messages
        .iter()
        .find(|message| message.role == AgentModelRole::Tool)
        .and_then(|message| message.parts.first())
        .and_then(|part| match part {
            AgentModelContentPart::ToolResult { result } => Some(result),
            _ => None,
        })
        .expect("tool result");
    assert_eq!(echoed_tool_result.call_id, opaque_call_id);
    wait_for_closed_sessions(
        &model_gateway_probe,
        vec!["run_tool_audit_test:inv_root".to_string()],
    )
    .await;

    tokio::fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn agent_loop_retries_retryable_model_errors() {
    let root = std::env::temp_dir().join(format!(
        "tauritavern-agent-retry-loop-{}",
        Uuid::new_v4().simple()
    ));
    let repository = Arc::new(FileAgentRepository::new(root.clone()));
    let run = AgentRun {
        id: "run_retry_loop_test".to_string(),
        workspace_id: "chat_retry_loop_test".to_string(),
        stable_chat_id: "stable_retry_loop_test".to_string(),
        chat_ref: AgentChatRef::Character {
            character_id: "Seraphina".to_string(),
            file_name: "Seraphina.png".to_string(),
        },
        generation_type: "normal".to_string(),
        profile_id: None,
        skill_scope_refs: Default::default(),
        persist_base_state_id: None,
        input_message_count: None,
        presentation: AgentRunPresentation::Background,
        status: AgentRunStatus::Created,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    repository.create_run(&run).await.expect("create run");

    let model_gateway = Arc::new(MockAgentModelGateway::with_results(vec![
        Err(ApplicationError::Transient(
            "temporary transport failure".to_string(),
        )),
        Err(ApplicationError::RateLimited(
            "provider rate limit".to_string(),
        )),
        Ok(json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_write",
                        "type": "function",
                        "function": {
                            "name": "workspace_write_file",
                            "arguments": "{\"path\":\"output/main.md\",\"content\":\"retry succeeded\"}"
                        }
                    }]
                }
            }]
        })),
        Ok(json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_finish",
                        "type": "function",
                        "function": {
                            "name": "workspace_finish",
                            "arguments": "{}"
                        }
                    }]
                }
            }]
        })),
    ]));
    let model_gateway_probe = model_gateway.clone();

    let service = AgentRuntimeService::new(
        repository.clone(),
        repository.clone(),
        repository.clone(),
        repository.clone(),
        test_chat_repository(&root),
        test_chat_repository(&root),
        test_skill_service(&root),
        model_gateway,
        test_profile_service(&root),
        test_llm_connection_service(&root),
    );
    let request = ChatCompletionGenerateRequestDto {
        payload: json!({
            "chat_completion_source": "openai",
            "model": "test-model",
            "messages": prompt_messages("write a message")
        })
        .as_object()
        .cloned()
        .unwrap(),
    };
    let prompt_snapshot = json!({ "chatCompletionPayload": request.payload.clone() });
    let (_cancel_sender, mut cancel_receiver) = watch::channel(false);
    let mut profile = test_resolved_profile(&root).await;
    profile.run.model_retry.max_retries = 3;
    profile.run.model_retry.interval_ms = 1;

    service
        .execute_agent_loop_run_inner(
            &run.id,
            prompt_snapshot,
            request,
            profile,
            &mut cancel_receiver,
        )
        .await
        .expect("agent loop");

    let model_requests = model_gateway_probe.requests().await;
    assert_eq!(model_requests.len(), 4);

    let artifact = repository
        .read_text(&run.id, &WorkspacePath::parse("output/main.md").unwrap())
        .await
        .expect("read artifact");
    assert_eq!(artifact.text, "retry succeeded");

    let events = repository
        .read_events(
            &run.id,
            AgentRunEventReadQuery {
                after_seq: Some(0),
                before_seq: None,
                limit: 200,
                invocation_id: None,
            },
        )
        .await
        .expect("read events");
    assert_eq!(
        events
            .iter()
            .filter(|event| event.event_type == "model_call_retry_scheduled")
            .count(),
        2
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event.event_type == "model_call_attempt_failed")
            .count(),
        2
    );

    tokio::fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn agent_loop_does_not_retry_non_retryable_model_errors() {
    let root = std::env::temp_dir().join(format!(
        "tauritavern-agent-no-retry-loop-{}",
        Uuid::new_v4().simple()
    ));
    let repository = Arc::new(FileAgentRepository::new(root.clone()));
    let run = AgentRun {
        id: "run_no_retry_loop_test".to_string(),
        workspace_id: "chat_no_retry_loop_test".to_string(),
        stable_chat_id: "stable_no_retry_loop_test".to_string(),
        chat_ref: AgentChatRef::Character {
            character_id: "Seraphina".to_string(),
            file_name: "Seraphina.png".to_string(),
        },
        generation_type: "normal".to_string(),
        profile_id: None,
        skill_scope_refs: Default::default(),
        persist_base_state_id: None,
        input_message_count: None,
        presentation: AgentRunPresentation::Background,
        status: AgentRunStatus::Created,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    repository.create_run(&run).await.expect("create run");

    let model_gateway = Arc::new(MockAgentModelGateway::with_results(vec![Err(
        ApplicationError::ValidationError("model.invalid_tool_call: missing id".to_string()),
    )]));
    let model_gateway_probe = model_gateway.clone();
    let service = AgentRuntimeService::new(
        repository.clone(),
        repository.clone(),
        repository.clone(),
        repository.clone(),
        test_chat_repository(&root),
        test_chat_repository(&root),
        test_skill_service(&root),
        model_gateway,
        test_profile_service(&root),
        test_llm_connection_service(&root),
    );
    let request = ChatCompletionGenerateRequestDto {
        payload: json!({
            "chat_completion_source": "openai",
            "model": "test-model",
            "messages": prompt_messages("write a message")
        })
        .as_object()
        .cloned()
        .unwrap(),
    };
    let prompt_snapshot = json!({ "chatCompletionPayload": request.payload.clone() });
    let (_cancel_sender, mut cancel_receiver) = watch::channel(false);
    let mut profile = test_resolved_profile(&root).await;
    profile.run.model_retry.max_retries = 3;
    profile.run.model_retry.interval_ms = 1;

    let error = service
        .execute_agent_loop_run_inner(
            &run.id,
            prompt_snapshot,
            request,
            profile,
            &mut cancel_receiver,
        )
        .await
        .expect_err("non-retryable model error");

    assert!(error.to_string().contains("missing id"));
    assert_eq!(model_gateway_probe.requests().await.len(), 1);

    let events = repository
        .read_events(
            &run.id,
            AgentRunEventReadQuery {
                after_seq: Some(0),
                before_seq: None,
                limit: 100,
                invocation_id: None,
            },
        )
        .await
        .expect("read events");
    assert!(
        events
            .iter()
            .all(|event| event.event_type != "model_call_retry_scheduled")
    );

    tokio::fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn agent_loop_reads_and_patches_workspace_artifact() {
    let root = std::env::temp_dir().join(format!(
        "tauritavern-agent-patch-loop-{}",
        Uuid::new_v4().simple()
    ));
    let repository = Arc::new(FileAgentRepository::new(root.clone()));
    let run = AgentRun {
        id: "run_patch_loop_test".to_string(),
        workspace_id: "chat_patch_loop_test".to_string(),
        stable_chat_id: "stable_patch_loop_test".to_string(),
        chat_ref: AgentChatRef::Character {
            character_id: "Seraphina".to_string(),
            file_name: "Seraphina.png".to_string(),
        },
        generation_type: "normal".to_string(),
        profile_id: None,
        skill_scope_refs: Default::default(),
        persist_base_state_id: None,
        input_message_count: None,
        presentation: AgentRunPresentation::Background,
        status: AgentRunStatus::Created,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    repository.create_run(&run).await.expect("create run");

    let model_gateway = Arc::new(MockAgentModelGateway::new(vec![
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_write",
                        "type": "function",
                        "function": {
                            "name": "workspace_write_file",
                            "arguments": "{\"path\":\"output/main.md\",\"content\":\"rough draft\"}"
                        }
                    }]
                }
            }]
        }),
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_read",
                        "type": "function",
                        "function": {
                            "name": "workspace_read_file",
                            "arguments": "{\"path\":\"output/main.md\"}"
                        }
                    }]
                }
            }]
        }),
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_patch",
                        "type": "function",
                        "function": {
                            "name": "workspace_apply_patch",
                            "arguments": "{\"path\":\"output/main.md\",\"old_string\":\"rough\",\"new_string\":\"polished\"}"
                        }
                    }]
                }
            }]
        }),
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_finish",
                        "type": "function",
                        "function": {
                            "name": "workspace_finish",
                            "arguments": "{}"
                        }
                    }]
                }
            }]
        }),
    ]));

    let service = AgentRuntimeService::new(
        repository.clone(),
        repository.clone(),
        repository.clone(),
        repository.clone(),
        test_chat_repository(&root),
        test_chat_repository(&root),
        test_skill_service(&root),
        model_gateway,
        test_profile_service(&root),
        test_llm_connection_service(&root),
    );
    let request = ChatCompletionGenerateRequestDto {
        payload: json!({
            "chat_completion_source": "openai",
            "model": "test-model",
            "messages": prompt_messages("revise a message")
        })
        .as_object()
        .cloned()
        .unwrap(),
    };
    let prompt_snapshot = json!({ "chatCompletionPayload": request.payload.clone() });
    let (_cancel_sender, mut cancel_receiver) = watch::channel(false);
    let profile = test_resolved_profile(&root).await;

    service
        .execute_agent_loop_run_inner(
            &run.id,
            prompt_snapshot,
            request,
            profile,
            &mut cancel_receiver,
        )
        .await
        .expect("agent loop");

    let saved = repository.load_run(&run.id).await.expect("load run");
    assert_eq!(saved.status, AgentRunStatus::Completed);

    let artifact = repository
        .read_text(&run.id, &WorkspacePath::parse("output/main.md").unwrap())
        .await
        .expect("read artifact");
    assert_eq!(artifact.text, "polished draft");

    let events = repository
        .read_events(
            &run.id,
            AgentRunEventReadQuery {
                after_seq: Some(0),
                before_seq: None,
                limit: 100,
                invocation_id: None,
            },
        )
        .await
        .expect("read events");
    assert!(
        events
            .iter()
            .any(|event| event.event_type == "workspace_patch_applied")
    );

    tokio::fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn finish_promotes_persistent_workspace_projection() {
    let root = std::env::temp_dir().join(format!(
        "tauritavern-agent-persist-loop-{}",
        Uuid::new_v4().simple()
    ));
    let repository = Arc::new(FileAgentRepository::new(root.clone()));
    let run = AgentRun {
        id: "run_persist_loop_test".to_string(),
        workspace_id: "chat_persist_loop_test".to_string(),
        stable_chat_id: "stable_persist_loop_test".to_string(),
        chat_ref: AgentChatRef::Character {
            character_id: "Seraphina".to_string(),
            file_name: "Seraphina.png".to_string(),
        },
        generation_type: "normal".to_string(),
        profile_id: None,
        skill_scope_refs: Default::default(),
        persist_base_state_id: None,
        input_message_count: None,
        presentation: AgentRunPresentation::Background,
        status: AgentRunStatus::Created,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    repository.create_run(&run).await.expect("create run");

    let model_gateway = Arc::new(MockAgentModelGateway::new(vec![
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [
                        {
                            "id": "call_write_output",
                            "type": "function",
                            "function": {
                                "name": "workspace_write_file",
                                "arguments": "{\"path\":\"output/main.md\",\"content\":\"committed story\"}"
                            }
                        },
                        {
                            "id": "call_write_persist",
                            "type": "function",
                            "function": {
                                "name": "workspace_write_file",
                                "arguments": "{\"path\":\"persist/MEMORY.md\",\"content\":\"The theatre sister thread is unresolved.\"}"
                            }
                        }
                    ]
                }
            }]
        }),
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_finish",
                        "type": "function",
                        "function": {
                            "name": "workspace_finish",
                            "arguments": "{}"
                        }
                    }]
                }
            }]
        }),
    ]));

    let service = AgentRuntimeService::new(
        repository.clone(),
        repository.clone(),
        repository.clone(),
        repository.clone(),
        test_chat_repository(&root),
        test_chat_repository(&root),
        test_skill_service(&root),
        model_gateway,
        test_profile_service(&root),
        test_llm_connection_service(&root),
    );
    let request = ChatCompletionGenerateRequestDto {
        payload: json!({
            "chat_completion_source": "openai",
            "model": "test-model",
            "messages": prompt_messages("write and remember")
        })
        .as_object()
        .cloned()
        .unwrap(),
    };
    let prompt_snapshot = json!({ "chatCompletionPayload": request.payload.clone() });
    let (_cancel_sender, mut cancel_receiver) = watch::channel(false);
    let profile = test_resolved_profile(&root).await;

    service
        .execute_agent_loop_run_inner(
            &run.id,
            prompt_snapshot,
            request,
            profile.clone(),
            &mut cancel_receiver,
        )
        .await
        .expect("agent loop");

    let next_run = AgentRun {
        id: "run_persist_loop_next".to_string(),
        persist_base_state_id: Some(run.id.clone()),
        status: AgentRunStatus::Created,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        ..run.clone()
    };
    repository
        .create_run(&next_run)
        .await
        .expect("create next run");
    repository
        .initialize_run(
            &next_run,
            &build_agent_manifest(&next_run, &profile),
            &json!({"messages": []}),
            &profile,
        )
        .await
        .expect("initialize next run");
    let projected = repository
        .read_text(
            &next_run.id,
            &WorkspacePath::parse("persist/MEMORY.md").unwrap(),
        )
        .await
        .expect("read projected persist");
    assert_eq!(projected.text, "The theatre sister thread is unresolved.");

    let events = repository
        .read_events(
            &run.id,
            AgentRunEventReadQuery {
                after_seq: Some(0),
                before_seq: None,
                limit: 100,
                invocation_id: None,
            },
        )
        .await
        .expect("read events");
    assert!(
        events
            .iter()
            .any(|event| event.event_type == "persistent_changes_committed")
    );

    tokio::fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn foreground_run_commits_chat_message_before_finish() {
    let root = std::env::temp_dir().join(format!(
        "tauritavern-agent-foreground-commit-{}",
        Uuid::new_v4().simple()
    ));
    let repository = Arc::new(FileAgentRepository::new(root.clone()));
    let run = AgentRun {
        id: "run_foreground_commit_test".to_string(),
        workspace_id: "chat_foreground_commit_test".to_string(),
        stable_chat_id: "stable_foreground_commit_test".to_string(),
        chat_ref: AgentChatRef::Character {
            character_id: "Seraphina".to_string(),
            file_name: "Seraphina.png".to_string(),
        },
        generation_type: "normal".to_string(),
        profile_id: None,
        skill_scope_refs: Default::default(),
        persist_base_state_id: None,
        input_message_count: None,
        presentation: AgentRunPresentation::Foreground,
        status: AgentRunStatus::Created,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    repository.create_run(&run).await.expect("create run");

    let model_gateway = Arc::new(MockAgentModelGateway::new(vec![
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_write",
                        "type": "function",
                        "function": {
                            "name": "workspace_write_file",
                            "arguments": "{\"path\":\"output/main.md\",\"content\":\"foreground answer\"}"
                        }
                    }]
                }
            }]
        }),
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [
                        {
                            "id": "call_commit",
                            "type": "function",
                            "function": {
                                "name": "workspace_commit",
                                "arguments": "{}"
                            }
                        },
                        {
                            "id": "call_finish",
                            "type": "function",
                            "function": {
                                "name": "workspace_finish",
                                "arguments": "{}"
                            }
                        }
                    ]
                }
            }]
        }),
    ]));

    let service = Arc::new(AgentRuntimeService::new(
        repository.clone(),
        repository.clone(),
        repository.clone(),
        repository.clone(),
        test_chat_repository(&root),
        test_chat_repository(&root),
        test_skill_service(&root),
        model_gateway,
        test_profile_service(&root),
        test_llm_connection_service(&root),
    ));
    let request = ChatCompletionGenerateRequestDto {
        payload: json!({
            "chat_completion_source": "openai",
            "model": "test-model",
            "messages": prompt_messages("write visibly")
        })
        .as_object()
        .cloned()
        .unwrap(),
    };
    let prompt_snapshot = json!({ "chatCompletionPayload": request.payload.clone() });
    let (_cancel_sender, mut cancel_receiver) = watch::channel(false);
    let mut profile = test_resolved_profile(&root).await;
    profile.run.presentation = AgentRunPresentation::Foreground;

    let resolver = tokio::spawn(resolve_next_chat_commit_and_persistent_state_update(
        service.clone(),
        repository.clone(),
        run.id.clone(),
        "message_1",
    ));
    service
        .execute_agent_loop_run_inner(
            &run.id,
            prompt_snapshot,
            request,
            profile,
            &mut cancel_receiver,
        )
        .await
        .expect("agent loop");
    resolver.await.expect("resolver task");

    let saved = repository.load_run(&run.id).await.expect("load run");
    assert_eq!(saved.status, AgentRunStatus::Completed);

    let events = repository
        .read_events(
            &run.id,
            AgentRunEventReadQuery {
                after_seq: Some(0),
                before_seq: None,
                limit: 100,
                invocation_id: None,
            },
        )
        .await
        .expect("read events");
    assert!(
        events
            .iter()
            .any(|event| event.event_type == "chat_commit_requested")
    );
    assert!(
        events
            .iter()
            .find(|event| event.event_type == "chat_commit_requested")
            .and_then(|event| event.payload.get("persistStateId"))
            .is_none(),
        "chat commit must not publish a reusable persistent state pointer"
    );
    assert!(
        events
            .iter()
            .any(|event| event.event_type == "chat_commit_completed")
    );
    assert!(events.iter().any(|event| {
        event.event_type == "persistent_state_metadata_update_requested"
            && event.payload["stateId"] == json!(run.id)
            && event.payload["messageId"] == json!("message_1")
    }));
    assert!(events.iter().any(|event| {
        event.event_type == "persistent_state_metadata_updated"
            && event.payload["stateId"] == json!(run.id)
    }));
    assert!(
        events
            .iter()
            .any(|event| event.event_type == "run_completed")
    );

    tokio::fs::remove_dir_all(root).await.expect("cleanup");
}

/// Issue #55 + #64 plus partial success: when the model commits, then
/// drifts by replying with plain text (no tool calls), the run gives the
/// model corrective nudges while normal maxRounds budget remains. If the
/// model keeps drifting until the final round, the error remains visible,
/// but the already host-confirmed chat commit is preserved and the
/// terminal state becomes `partial_success` instead of rolling the message
/// back. This is the failure-after-recovery path; the success path is
/// covered by
/// [`foreground_run_recovers_from_post_commit_drift_with_nudge`].
#[tokio::test]
async fn foreground_run_keeps_committed_chat_as_partial_success_on_tool_call_required_drift() {
    let root = std::env::temp_dir().join(format!(
        "tauritavern-agent-drift-partial-success-{}",
        Uuid::new_v4().simple()
    ));
    let repository = Arc::new(FileAgentRepository::new(root.clone()));
    let run = AgentRun {
        id: "run_drift_rollback_test".to_string(),
        workspace_id: "chat_drift_rollback_test".to_string(),
        stable_chat_id: "stable_drift_rollback_test".to_string(),
        chat_ref: AgentChatRef::Character {
            character_id: "Seraphina".to_string(),
            file_name: "Seraphina.png".to_string(),
        },
        generation_type: "normal".to_string(),
        profile_id: None,
        skill_scope_refs: Default::default(),
        persist_base_state_id: None,
        input_message_count: None,
        presentation: AgentRunPresentation::Foreground,
        status: AgentRunStatus::Created,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    repository.create_run(&run).await.expect("create run");

    let model_gateway = Arc::new(MockAgentModelGateway::new(vec![
        // Round 1: legitimately write the artifact.
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_write_drift",
                        "type": "function",
                        "function": {
                            "name": "workspace_write_file",
                            "arguments": "{\"path\":\"output/main.md\",\"content\":\"drift answer\"}"
                        }
                    }]
                }
            }]
        }),
        // Round 2: commit the artifact. This is the host-confirmed chat
        // output partial success must preserve if the later run fails.
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_commit_drift",
                        "type": "function",
                        "function": {
                            "name": "workspace_commit",
                            "arguments": "{}"
                        }
                    }]
                }
            }]
        }),
        // Round 3: drift — return plain text and no tool calls. With #64
        // this triggers a soft recovery attempt (a corrective `user`
        // message gets injected); the run does not fail yet.
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "Sorry, here is the full answer one more time...",
                }
            }]
        }),
        // Round 4: stubborn drift — model ignores the nudge and replies
        // with plain text again. Because rounds remain, the loop corrects
        // it again instead of surrendering after a single attempt.
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "Sorry, here it is one more time...",
                }
            }]
        }),
        // Round 5: final-round drift. There is no remaining model round for
        // another nudge, so the run records a partial success preserving the
        // committed message.
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "Still responding directly.",
                }
            }]
        }),
    ]));

    let service = Arc::new(AgentRuntimeService::new(
        repository.clone(),
        repository.clone(),
        repository.clone(),
        repository.clone(),
        test_chat_repository(&root),
        test_chat_repository(&root),
        test_skill_service(&root),
        model_gateway,
        test_profile_service(&root),
        test_llm_connection_service(&root),
    ));
    let request = ChatCompletionGenerateRequestDto {
        payload: json!({
            "chat_completion_source": "openai",
            "model": "test-model",
            "messages": prompt_messages("drift after commit")
        })
        .as_object()
        .cloned()
        .unwrap(),
    };
    let prompt_snapshot = json!({ "chatCompletionPayload": request.payload.clone() });
    let (_cancel_sender, mut cancel_receiver) = watch::channel(false);
    let mut profile = test_resolved_profile(&root).await;
    profile.run.presentation = AgentRunPresentation::Foreground;
    profile.tools.max_rounds = 5;

    let resolver = tokio::spawn(resolve_next_chat_commit(
        service.clone(),
        repository.clone(),
        run.id.clone(),
        "message_42",
    ));
    let outcome = service
        .execute_agent_loop_run_inner(
            &run.id,
            prompt_snapshot,
            request,
            profile,
            &mut cancel_receiver,
        )
        .await;
    resolver.await.expect("resolver task");

    assert!(
        outcome.is_err(),
        "partial success keeps the committed chat but must still expose the underlying error"
    );

    let saved = repository.load_run(&run.id).await.expect("load run");
    assert_eq!(saved.status, AgentRunStatus::PartialSuccess);

    let events = repository
        .read_events(
            &run.id,
            AgentRunEventReadQuery {
                after_seq: Some(0),
                before_seq: None,
                limit: 200,
                invocation_id: None,
            },
        )
        .await
        .expect("read events");

    let chat_commit_request = events
        .iter()
        .find(|event| event.event_type == "chat_commit_requested")
        .expect("chat commit request");
    assert!(
        chat_commit_request.payload.get("persistStateId").is_none(),
        "partial-success-prone chat commit must not publish a reusable persistent state pointer"
    );
    assert!(
        !events
            .iter()
            .any(|event| event.event_type == "persistent_state_metadata_update_requested"),
        "partial success must not ask the host to attach a persistent state pointer"
    );

    // #64: each non-final drift must produce a `drift_recovery_attempted`
    // event before we surrender to the terminal partial-success path. If
    // this assertion fails, the recovery path regressed back to a hidden
    // one-shot budget or was bypassed entirely.
    let recovery_events: Vec<_> = events
        .iter()
        .filter(|event| event.event_type == "drift_recovery_attempted")
        .collect();
    assert_eq!(
        recovery_events.len(),
        2,
        "each recoverable drift before the final round must emit a recovery event"
    );
    assert_eq!(recovery_events[0].level, AgentRunEventLevel::Warn);
    assert_eq!(recovery_events[0].payload["attempt"], 1);
    assert_eq!(recovery_events[0].payload["maxAttempts"], 4);
    assert_eq!(recovery_events[0].payload["maxRounds"], 5);
    assert_eq!(recovery_events[0].payload["limitReason"], "max_rounds");
    assert_eq!(recovery_events[0].payload["committedCount"], 1);
    assert_eq!(
        recovery_events[0].payload["reasonCode"],
        "model.tool_call_required"
    );
    assert_eq!(recovery_events[1].level, AgentRunEventLevel::Warn);
    assert_eq!(recovery_events[1].payload["attempt"], 2);
    assert_eq!(recovery_events[1].payload["maxAttempts"], 4);
    assert_eq!(recovery_events[1].payload["maxRounds"], 5);
    assert_eq!(recovery_events[1].payload["limitReason"], "max_rounds");
    assert_eq!(recovery_events[1].payload["committedCount"], 1);
    assert_eq!(
        recovery_events[1].payload["reasonCode"],
        "model.tool_call_required"
    );

    assert!(
        !events
            .iter()
            .any(|event| event.event_type == "run_rollback_targets"),
        "partial success must not auto-rollback committed chat output"
    );
    assert!(
        !events.iter().any(|event| event.event_type == "run_failed"),
        "partial success is its own terminal event, not run_failed"
    );
    assert!(
        !events
            .iter()
            .any(|event| event.event_type == "run_completed"),
        "partial success must not masquerade as clean completion"
    );

    let partial = events
        .iter()
        .find(|event| event.event_type == "run_partial_success")
        .expect("run_partial_success event must be emitted on drift after commit");
    assert_eq!(partial.level, AgentRunEventLevel::Warn);
    assert_eq!(partial.payload["code"], "model.tool_call_required");
    assert_eq!(partial.payload["retryable"], false);
    assert_eq!(partial.payload["userRetryable"], false);
    assert_eq!(partial.payload["preservedCommitCount"], 1);
    let targets = partial.payload["preservedCommits"]
        .as_array()
        .expect("preserved commits array");
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0]["messageId"], "message_42");
    assert_eq!(targets[0]["path"], "output/main.md");

    tokio::fs::remove_dir_all(root).await.expect("cleanup");
}

/// Issue #64: when the model commits and then drifts (plain text, no
/// tool calls), the loop must inject a corrective `user` reminder and
/// give the model another chance to call `workspace_finish`. If the
/// model complies, the run completes normally — the commit is NOT
/// rolled back. This is the happy-path complement to
/// [`foreground_run_keeps_committed_chat_as_partial_success_on_tool_call_required_drift`].
#[tokio::test]
async fn foreground_run_recovers_from_post_commit_drift_with_nudge() {
    let root = std::env::temp_dir().join(format!(
        "tauritavern-agent-drift-recovery-{}",
        Uuid::new_v4().simple()
    ));
    let repository = Arc::new(FileAgentRepository::new(root.clone()));
    let run = AgentRun {
        id: "run_drift_recovery_test".to_string(),
        workspace_id: "chat_drift_recovery_test".to_string(),
        stable_chat_id: "stable_drift_recovery_test".to_string(),
        chat_ref: AgentChatRef::Character {
            character_id: "Seraphina".to_string(),
            file_name: "Seraphina.png".to_string(),
        },
        generation_type: "normal".to_string(),
        profile_id: None,
        skill_scope_refs: Default::default(),
        persist_base_state_id: None,
        input_message_count: None,
        presentation: AgentRunPresentation::Foreground,
        status: AgentRunStatus::Created,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    repository.create_run(&run).await.expect("create run");

    let model_gateway = Arc::new(MockAgentModelGateway::new(vec![
        // Round 1: write artifact.
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_write_recovery",
                        "type": "function",
                        "function": {
                            "name": "workspace_write_file",
                            "arguments": "{\"path\":\"output/main.md\",\"content\":\"recovered answer\"}"
                        }
                    }]
                }
            }]
        }),
        // Round 2: commit it.
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_commit_recovery",
                        "type": "function",
                        "function": {
                            "name": "workspace_commit",
                            "arguments": "{}"
                        }
                    }]
                }
            }]
        }),
        // Round 3: drift — model replies in plain text instead of calling
        // workspace_finish. #64 injects a corrective nudge.
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "Oh and here's the answer in chat form...",
                }
            }]
        }),
        // Round 4: model reads the nudge and complies — calls
        // workspace_finish. Run should complete cleanly.
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_finish_recovery",
                        "type": "function",
                        "function": {
                            "name": "workspace_finish",
                            "arguments": "{\"reason\":\"recovered after drift\"}"
                        }
                    }]
                }
            }]
        }),
    ]));

    let service = Arc::new(AgentRuntimeService::new(
        repository.clone(),
        repository.clone(),
        repository.clone(),
        repository.clone(),
        test_chat_repository(&root),
        test_chat_repository(&root),
        test_skill_service(&root),
        model_gateway.clone(),
        test_profile_service(&root),
        test_llm_connection_service(&root),
    ));
    let request = ChatCompletionGenerateRequestDto {
        payload: json!({
            "chat_completion_source": "openai",
            "model": "test-model",
            "messages": prompt_messages("drift recovery happy path")
        })
        .as_object()
        .cloned()
        .unwrap(),
    };
    let prompt_snapshot = json!({ "chatCompletionPayload": request.payload.clone() });
    let (_cancel_sender, mut cancel_receiver) = watch::channel(false);
    let mut profile = test_resolved_profile(&root).await;
    profile.run.presentation = AgentRunPresentation::Foreground;
    let max_rounds = profile.tools.max_rounds;

    let resolver = tokio::spawn(resolve_next_chat_commit_and_persistent_state_update(
        service.clone(),
        repository.clone(),
        run.id.clone(),
        "message_recovery_42",
    ));
    service
        .execute_agent_loop_run_inner(
            &run.id,
            prompt_snapshot,
            request,
            profile,
            &mut cancel_receiver,
        )
        .await
        .expect("recovery should let the run complete");
    resolver.await.expect("resolver task");

    let saved = repository.load_run(&run.id).await.expect("load run");
    assert_eq!(saved.status, AgentRunStatus::Completed);

    let events = repository
        .read_events(
            &run.id,
            AgentRunEventReadQuery {
                after_seq: Some(0),
                before_seq: None,
                limit: 200,
                invocation_id: None,
            },
        )
        .await
        .expect("read events");

    let recovery_events: Vec<_> = events
        .iter()
        .filter(|event| event.event_type == "drift_recovery_attempted")
        .collect();
    assert_eq!(
        recovery_events.len(),
        1,
        "recovery must fire exactly once for the single drift event"
    );
    assert_eq!(recovery_events[0].level, AgentRunEventLevel::Warn);
    assert_eq!(recovery_events[0].payload["attempt"], 1);
    assert_eq!(
        recovery_events[0].payload["maxAttempts"],
        max_rounds.saturating_sub(1)
    );
    assert_eq!(recovery_events[0].payload["maxRounds"], max_rounds);
    assert_eq!(recovery_events[0].payload["limitReason"], "max_rounds");
    assert_eq!(recovery_events[0].payload["committedCount"], 1);

    // The commit must NOT be rolled back when recovery succeeds.
    assert!(
        !events
            .iter()
            .any(|event| event.event_type == "run_rollback_targets"),
        "no rollback target events when recovery succeeds"
    );
    // The corrective nudge must reach the model — verify the message
    // list grew with both the drifted assistant turn and our synthetic
    // user reminder before round 4. The 4th model request should have
    // received them.
    let requests = model_gateway.requests().await;
    assert_eq!(
        requests.len(),
        4,
        "model must be called exactly 4 times (3 normal + 1 recovery)"
    );
    let last_request = requests.last().unwrap();
    let drift_user_message = last_request
        .messages
        .iter()
        .rev()
        .find(|m| matches!(m.role, AgentModelRole::User))
        .expect("recovery nudge must be present as user message");
    let nudge_text = drift_user_message
        .parts
        .iter()
        .find_map(|part| match part {
            AgentModelContentPart::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .unwrap_or("");
    assert!(
        nudge_text.contains("direct output recovery attempt 1"),
        "nudge must include attempt counter without implying a one-shot budget; got: {nudge_text}"
    );
    assert!(
        nudge_text.contains("workspace_finish"),
        "nudge must reference workspace_finish; got: {nudge_text}"
    );
    assert!(
        nudge_text.contains("workspace_commit again before workspace_finish"),
        "nudge must require another commit after revising workspace files; got: {nudge_text}"
    );
    assert!(
        nudge_text.contains("output/direct_output.md"),
        "nudge must point at the captured direct output file; got: {nudge_text}"
    );

    let captured = repository
        .read_text(
            &run.id,
            &WorkspacePath::parse("output/direct_output.md").unwrap(),
        )
        .await
        .expect("direct output should be captured in workspace");
    assert_eq!(captured.text, "Oh and here's the answer in chat form...");
    assert!(
        events
            .iter()
            .any(|event| event.event_type == "direct_output_captured"
                && event.payload["path"] == json!("output/direct_output.md")
                && event.payload["round"] == json!(3)),
        "direct output capture must be journaled"
    );
}

/// Issue #64: when the model drifts WITHOUT having committed anything,
/// the loop saves the direct text into workspace and gives the model one
/// corrective nudge. On recovery the model can commit that captured file
/// directly instead of regenerating or copying the content.
#[tokio::test]
async fn foreground_run_recovers_from_no_commit_drift_with_nudge() {
    let root = std::env::temp_dir().join(format!(
        "tauritavern-agent-drift-recovery-nocommit-{}",
        Uuid::new_v4().simple()
    ));
    let repository = Arc::new(FileAgentRepository::new(root.clone()));
    let run = AgentRun {
        id: "run_drift_recovery_nocommit_test".to_string(),
        workspace_id: "chat_drift_recovery_nocommit_test".to_string(),
        stable_chat_id: "stable_drift_recovery_nocommit_test".to_string(),
        chat_ref: AgentChatRef::Character {
            character_id: "Seraphina".to_string(),
            file_name: "Seraphina.png".to_string(),
        },
        generation_type: "normal".to_string(),
        profile_id: None,
        skill_scope_refs: Default::default(),
        persist_base_state_id: None,
        input_message_count: None,
        presentation: AgentRunPresentation::Foreground,
        status: AgentRunStatus::Created,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    repository.create_run(&run).await.expect("create run");

    let model_gateway = Arc::new(MockAgentModelGateway::new(vec![
        // Round 1: drift right away — no tool calls.
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "Sure, here is the answer directly...",
                }
            }]
        }),
        // Round 2: model recovers by committing the captured direct output.
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_commit_nocommit",
                        "type": "function",
                        "function": {
                            "name": "workspace_commit",
                            "arguments": "{\"path\":\"output/direct_output.md\"}"
                        }
                    }]
                }
            }]
        }),
        // Round 3: finish.
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_finish_nocommit",
                        "type": "function",
                        "function": {
                            "name": "workspace_finish",
                            "arguments": "{}"
                        }
                    }]
                }
            }]
        }),
    ]));

    let service = Arc::new(AgentRuntimeService::new(
        repository.clone(),
        repository.clone(),
        repository.clone(),
        repository.clone(),
        test_chat_repository(&root),
        test_chat_repository(&root),
        test_skill_service(&root),
        model_gateway.clone(),
        test_profile_service(&root),
        test_llm_connection_service(&root),
    ));
    let request = ChatCompletionGenerateRequestDto {
        payload: json!({
            "chat_completion_source": "openai",
            "model": "test-model",
            "messages": prompt_messages("no-commit drift recovery")
        })
        .as_object()
        .cloned()
        .unwrap(),
    };
    let prompt_snapshot = json!({ "chatCompletionPayload": request.payload.clone() });
    let (_cancel_sender, mut cancel_receiver) = watch::channel(false);
    let mut profile = test_resolved_profile(&root).await;
    profile.run.presentation = AgentRunPresentation::Foreground;
    let max_rounds = profile.tools.max_rounds;

    let resolver = tokio::spawn(resolve_next_chat_commit_and_persistent_state_update(
        service.clone(),
        repository.clone(),
        run.id.clone(),
        "message_nocommit_42",
    ));
    service
        .execute_agent_loop_run_inner(
            &run.id,
            prompt_snapshot,
            request,
            profile,
            &mut cancel_receiver,
        )
        .await
        .expect("recovery should let the run complete");
    resolver.await.expect("resolver task");

    let saved = repository.load_run(&run.id).await.expect("load run");
    assert_eq!(saved.status, AgentRunStatus::Completed);

    let events = repository
        .read_events(
            &run.id,
            AgentRunEventReadQuery {
                after_seq: Some(0),
                before_seq: None,
                limit: 200,
                invocation_id: None,
            },
        )
        .await
        .expect("read events");
    let recovery_events: Vec<_> = events
        .iter()
        .filter(|event| event.event_type == "drift_recovery_attempted")
        .collect();
    assert_eq!(recovery_events.len(), 1);
    assert_eq!(recovery_events[0].payload["attempt"], 1);
    assert_eq!(
        recovery_events[0].payload["maxAttempts"],
        max_rounds.saturating_sub(1)
    );
    assert_eq!(recovery_events[0].payload["maxRounds"], max_rounds);
    assert_eq!(recovery_events[0].payload["limitReason"], "max_rounds");
    assert_eq!(recovery_events[0].payload["committedCount"], 0);

    let captured = repository
        .read_text(
            &run.id,
            &WorkspacePath::parse("output/direct_output.md").unwrap(),
        )
        .await
        .expect("direct output should be captured in workspace");
    assert_eq!(captured.text, "Sure, here is the answer directly...");
    assert!(
        events
            .iter()
            .any(|event| event.event_type == "direct_output_captured"
                && event.payload["path"] == json!("output/direct_output.md")
                && event.payload["round"] == json!(1)),
        "direct output capture must be journaled"
    );

    assert!(
        !events
            .iter()
            .any(|event| event.event_type == "run_rollback_targets"),
        "no rollback when no commits existed before recovery"
    );
}

#[tokio::test]
async fn foreground_run_without_commit_still_fails_when_drift_recovery_hits_max_rounds() {
    let root = std::env::temp_dir().join(format!(
        "tauritavern-agent-drift-no-commit-failure-{}",
        Uuid::new_v4().simple()
    ));
    let repository = Arc::new(FileAgentRepository::new(root.clone()));
    let run = AgentRun {
        id: "run_drift_no_commit_failure_test".to_string(),
        workspace_id: "chat_drift_no_commit_failure_test".to_string(),
        stable_chat_id: "stable_drift_no_commit_failure_test".to_string(),
        chat_ref: AgentChatRef::Character {
            character_id: "Seraphina".to_string(),
            file_name: "Seraphina.png".to_string(),
        },
        generation_type: "normal".to_string(),
        profile_id: None,
        skill_scope_refs: Default::default(),
        persist_base_state_id: None,
        input_message_count: None,
        presentation: AgentRunPresentation::Foreground,
        status: AgentRunStatus::Created,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    repository.create_run(&run).await.expect("create run");

    let model_gateway = Arc::new(MockAgentModelGateway::new(vec![
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "I will answer directly instead of using tools.",
                }
            }]
        }),
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "Still answering directly.",
                }
            }]
        }),
    ]));

    let service = AgentRuntimeService::new(
        repository.clone(),
        repository.clone(),
        repository.clone(),
        repository.clone(),
        test_chat_repository(&root),
        test_chat_repository(&root),
        test_skill_service(&root),
        model_gateway,
        test_profile_service(&root),
        test_llm_connection_service(&root),
    );
    let request = ChatCompletionGenerateRequestDto {
        payload: json!({
            "chat_completion_source": "openai",
            "model": "test-model",
            "messages": prompt_messages("no commit stubborn drift")
        })
        .as_object()
        .cloned()
        .unwrap(),
    };
    let prompt_snapshot = json!({ "chatCompletionPayload": request.payload.clone() });
    let (_cancel_sender, mut cancel_receiver) = watch::channel(false);
    let mut profile = test_resolved_profile(&root).await;
    profile.run.presentation = AgentRunPresentation::Foreground;
    profile.tools.max_rounds = 2;

    let outcome = service
        .execute_agent_loop_run_inner(
            &run.id,
            prompt_snapshot,
            request,
            profile,
            &mut cancel_receiver,
        )
        .await;
    assert!(
        outcome.is_err(),
        "no-commit drift must remain a hard failure"
    );

    let saved = repository.load_run(&run.id).await.expect("load run");
    assert_eq!(saved.status, AgentRunStatus::Failed);

    let events = repository
        .read_events(
            &run.id,
            AgentRunEventReadQuery {
                after_seq: Some(0),
                before_seq: None,
                limit: 100,
                invocation_id: None,
            },
        )
        .await
        .expect("read events");
    let recovery_events: Vec<_> = events
        .iter()
        .filter(|event| event.event_type == "drift_recovery_attempted")
        .collect();
    assert_eq!(
        recovery_events.len(),
        1,
        "the first direct output should be corrected, then the final round should fail visibly"
    );
    assert_eq!(recovery_events[0].payload["attempt"], 1);
    assert_eq!(recovery_events[0].payload["maxAttempts"], 1);
    assert_eq!(recovery_events[0].payload["maxRounds"], 2);
    assert_eq!(recovery_events[0].payload["limitReason"], "max_rounds");
    assert!(
        events.iter().any(|event| {
            event.event_type == "run_failed"
                && event.payload["code"] == json!("model.tool_call_required")
                && event.payload["userRetryable"] == json!(true)
        }),
        "no-commit drift should expose the existing user-retryable failure"
    );
    assert!(
        !events
            .iter()
            .any(|event| event.event_type == "run_partial_success"),
        "partial success requires at least one successful chat commit"
    );

    tokio::fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn foreground_run_with_commit_becomes_partial_success_when_persistent_commit_fails() {
    let root = std::env::temp_dir().join(format!(
        "tauritavern-agent-persistent-partial-success-{}",
        Uuid::new_v4().simple()
    ));
    let repository = Arc::new(FileAgentRepository::new(root.clone()));
    let workspace_repository = Arc::new(FailingPersistentCommitWorkspaceRepository {
        inner: repository.clone(),
    });
    let run = AgentRun {
        id: "run_persistent_partial_success_test".to_string(),
        workspace_id: "chat_persistent_partial_success_test".to_string(),
        stable_chat_id: "stable_persistent_partial_success_test".to_string(),
        chat_ref: AgentChatRef::Character {
            character_id: "Seraphina".to_string(),
            file_name: "Seraphina.png".to_string(),
        },
        generation_type: "normal".to_string(),
        profile_id: None,
        skill_scope_refs: Default::default(),
        persist_base_state_id: None,
        input_message_count: None,
        presentation: AgentRunPresentation::Foreground,
        status: AgentRunStatus::Created,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    repository.create_run(&run).await.expect("create run");

    let model_gateway = Arc::new(MockAgentModelGateway::new(vec![
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_write_persistent_failure",
                        "type": "function",
                        "function": {
                            "name": "workspace_write_file",
                            "arguments": "{\"path\":\"output/main.md\",\"content\":\"visible answer\"}"
                        }
                    }]
                }
            }]
        }),
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_commit_persistent_failure",
                        "type": "function",
                        "function": {
                            "name": "workspace_commit",
                            "arguments": "{}"
                        }
                    }]
                }
            }]
        }),
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_finish_persistent_failure",
                        "type": "function",
                        "function": {
                            "name": "workspace_finish",
                            "arguments": "{}"
                        }
                    }]
                }
            }]
        }),
    ]));

    let service = Arc::new(AgentRuntimeService::new(
        repository.clone(),
        repository.clone(),
        workspace_repository,
        repository.clone(),
        test_chat_repository(&root),
        test_chat_repository(&root),
        test_skill_service(&root),
        model_gateway,
        test_profile_service(&root),
        test_llm_connection_service(&root),
    ));
    let request = ChatCompletionGenerateRequestDto {
        payload: json!({
            "chat_completion_source": "openai",
            "model": "test-model",
            "messages": prompt_messages("persistent failure after commit")
        })
        .as_object()
        .cloned()
        .unwrap(),
    };
    let prompt_snapshot = json!({ "chatCompletionPayload": request.payload.clone() });
    let (_cancel_sender, mut cancel_receiver) = watch::channel(false);
    let mut profile = test_resolved_profile(&root).await;
    profile.run.presentation = AgentRunPresentation::Foreground;

    let resolver = tokio::spawn(resolve_next_chat_commit(
        service.clone(),
        repository.clone(),
        run.id.clone(),
        "message_persistent_failure",
    ));
    let outcome = service
        .execute_agent_loop_run_inner(
            &run.id,
            prompt_snapshot,
            request,
            profile,
            &mut cancel_receiver,
        )
        .await;
    resolver.await.expect("resolver task");
    assert!(
        outcome.is_err(),
        "persistent commit failure must still expose the underlying error"
    );

    let saved = repository.load_run(&run.id).await.expect("load run");
    assert_eq!(saved.status, AgentRunStatus::PartialSuccess);

    let events = repository
        .read_events(
            &run.id,
            AgentRunEventReadQuery {
                after_seq: Some(0),
                before_seq: None,
                limit: 200,
                invocation_id: None,
            },
        )
        .await
        .expect("read events");
    assert!(
        events
            .iter()
            .any(|event| event.event_type == "persistent_changes_commit_failed")
    );
    assert!(
        !events
            .iter()
            .any(|event| event.event_type == "persistent_state_metadata_update_requested"),
        "failed persistent commit must not publish a reusable persistent state pointer"
    );
    assert!(
        events.iter().any(|event| {
            event.event_type == "run_partial_success"
                && event.payload["code"] == json!("agent.test_persistent_failure")
                && event.payload["preservedCommitCount"] == json!(1)
        }),
        "persistent commit failure after chat commit should become partial success"
    );
    assert!(
        !events
            .iter()
            .any(|event| event.event_type == "run_completed"),
        "persistent failure must not masquerade as clean completion"
    );
    assert!(
        !events.iter().any(|event| event.event_type == "run_failed"),
        "partial success is its own terminal event"
    );

    tokio::fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn foreground_finish_before_commit_returns_recoverable_error() {
    let root = std::env::temp_dir().join(format!(
        "tauritavern-agent-foreground-finish-guard-{}",
        Uuid::new_v4().simple()
    ));
    let repository = Arc::new(FileAgentRepository::new(root.clone()));
    let run = AgentRun {
        id: "run_foreground_finish_guard_test".to_string(),
        workspace_id: "chat_foreground_finish_guard_test".to_string(),
        stable_chat_id: "stable_foreground_finish_guard_test".to_string(),
        chat_ref: AgentChatRef::Character {
            character_id: "Seraphina".to_string(),
            file_name: "Seraphina.png".to_string(),
        },
        generation_type: "normal".to_string(),
        profile_id: None,
        skill_scope_refs: Default::default(),
        persist_base_state_id: None,
        input_message_count: None,
        presentation: AgentRunPresentation::Foreground,
        status: AgentRunStatus::Created,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    repository.create_run(&run).await.expect("create run");

    let model_gateway = Arc::new(MockAgentModelGateway::new(vec![
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_finish_too_early",
                        "type": "function",
                        "function": {
                            "name": "workspace_finish",
                            "arguments": "{}"
                        }
                    }]
                }
            }]
        }),
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [
                        {
                            "id": "call_write_after_guard",
                            "type": "function",
                            "function": {
                                "name": "workspace_write_file",
                                "arguments": "{\"path\":\"output/main.md\",\"content\":\"guarded answer\"}"
                            }
                        },
                        {
                            "id": "call_commit_after_guard",
                            "type": "function",
                            "function": {
                                "name": "workspace_commit",
                                "arguments": "{\"mode\":\"append\"}"
                            }
                        }
                    ]
                }
            }]
        }),
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_finish_after_commit",
                        "type": "function",
                        "function": {
                            "name": "workspace_finish",
                            "arguments": "{}"
                        }
                    }]
                }
            }]
        }),
    ]));

    let service = Arc::new(AgentRuntimeService::new(
        repository.clone(),
        repository.clone(),
        repository.clone(),
        repository.clone(),
        test_chat_repository(&root),
        test_chat_repository(&root),
        test_skill_service(&root),
        model_gateway,
        test_profile_service(&root),
        test_llm_connection_service(&root),
    ));
    let request = ChatCompletionGenerateRequestDto {
        payload: json!({
            "chat_completion_source": "openai",
            "model": "test-model",
            "messages": prompt_messages("finish too early then recover")
        })
        .as_object()
        .cloned()
        .unwrap(),
    };
    let prompt_snapshot = json!({ "chatCompletionPayload": request.payload.clone() });
    let (_cancel_sender, mut cancel_receiver) = watch::channel(false);
    let mut profile = test_resolved_profile(&root).await;
    profile.run.presentation = AgentRunPresentation::Foreground;

    let resolver = tokio::spawn(resolve_next_chat_commit_and_persistent_state_update(
        service.clone(),
        repository.clone(),
        run.id.clone(),
        "message_1",
    ));
    service
        .execute_agent_loop_run_inner(
            &run.id,
            prompt_snapshot,
            request,
            profile,
            &mut cancel_receiver,
        )
        .await
        .expect("agent loop");
    resolver.await.expect("resolver task");

    let saved = repository.load_run(&run.id).await.expect("load run");
    assert_eq!(saved.status, AgentRunStatus::Completed);

    let events = repository
        .read_events(
            &run.id,
            AgentRunEventReadQuery {
                after_seq: Some(0),
                before_seq: None,
                limit: 100,
                invocation_id: None,
            },
        )
        .await
        .expect("read events");
    let guard_failure = events
        .iter()
        .find(|event| {
            event.event_type == "tool_call_failed"
                && event.payload["callId"] == "call_finish_too_early"
        })
        .expect("foreground finish guard failure");
    assert_eq!(guard_failure.level, AgentRunEventLevel::Warn);
    assert_eq!(
        guard_failure.payload["errorCode"],
        "agent.foreground_commit_required"
    );

    tokio::fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn agent_loop_returns_recoverable_tool_errors_to_model() {
    let root = std::env::temp_dir().join(format!(
        "tauritavern-agent-tool-error-loop-{}",
        Uuid::new_v4().simple()
    ));
    let repository = Arc::new(FileAgentRepository::new(root.clone()));
    let run = AgentRun {
        id: "run_tool_error_loop_test".to_string(),
        workspace_id: "chat_tool_error_loop_test".to_string(),
        stable_chat_id: "stable_tool_error_loop_test".to_string(),
        chat_ref: AgentChatRef::Character {
            character_id: "Seraphina".to_string(),
            file_name: "Seraphina.png".to_string(),
        },
        generation_type: "normal".to_string(),
        profile_id: None,
        skill_scope_refs: Default::default(),
        persist_base_state_id: None,
        input_message_count: None,
        presentation: AgentRunPresentation::Background,
        status: AgentRunStatus::Created,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    repository.create_run(&run).await.expect("create run");

    let model_gateway = Arc::new(MockAgentModelGateway::new(vec![
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_bad_read",
                        "type": "function",
                        "function": {
                            "name": "workspace_read_file",
                            "arguments": "{\"path\":\".\"}"
                        }
                    }]
                }
            }]
        }),
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_write_after_error",
                        "type": "function",
                        "function": {
                            "name": "workspace_write_file",
                            "arguments": "{\"path\":\"output/main.md\",\"content\":\"recovered\"}"
                        }
                    }]
                }
            }]
        }),
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_finish_after_error",
                        "type": "function",
                        "function": {
                            "name": "workspace_finish",
                            "arguments": "{}"
                        }
                    }]
                }
            }]
        }),
    ]));

    let service = AgentRuntimeService::new(
        repository.clone(),
        repository.clone(),
        repository.clone(),
        repository.clone(),
        test_chat_repository(&root),
        test_chat_repository(&root),
        test_skill_service(&root),
        model_gateway,
        test_profile_service(&root),
        test_llm_connection_service(&root),
    );
    let request = ChatCompletionGenerateRequestDto {
        payload: json!({
            "chat_completion_source": "openai",
            "model": "test-model",
            "messages": prompt_messages("recover from tool error")
        })
        .as_object()
        .cloned()
        .unwrap(),
    };
    let prompt_snapshot = json!({ "chatCompletionPayload": request.payload.clone() });
    let (_cancel_sender, mut cancel_receiver) = watch::channel(false);
    let profile = test_resolved_profile(&root).await;

    service
        .execute_agent_loop_run_inner(
            &run.id,
            prompt_snapshot,
            request,
            profile,
            &mut cancel_receiver,
        )
        .await
        .expect("agent loop");

    let saved = repository.load_run(&run.id).await.expect("load run");
    assert_eq!(saved.status, AgentRunStatus::Completed);

    let artifact = repository
        .read_text(&run.id, &WorkspacePath::parse("output/main.md").unwrap())
        .await
        .expect("read artifact");
    assert_eq!(artifact.text, "recovered");

    let events = repository
        .read_events(
            &run.id,
            AgentRunEventReadQuery {
                after_seq: Some(0),
                before_seq: None,
                limit: 100,
                invocation_id: None,
            },
        )
        .await
        .expect("read events");
    let failed = events
        .iter()
        .find(|event| {
            event.event_type == "tool_call_failed" && event.payload["callId"] == "call_bad_read"
        })
        .expect("tool failure event");
    assert_eq!(failed.level, AgentRunEventLevel::Warn);
    assert_eq!(failed.payload["errorCode"], "workspace.invalid_path");
    assert!(
        failed.payload["message"]
            .as_str()
            .expect("message")
            .contains("Workspace path cannot be empty")
    );

    tokio::fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn workspace_patch_allows_partial_read_when_old_string_was_observed() {
    let root = std::env::temp_dir().join(format!(
        "tauritavern-agent-patch-guard-{}",
        Uuid::new_v4().simple()
    ));
    let repository = Arc::new(FileAgentRepository::new(root.clone()));
    let run = AgentRun {
        id: "run_patch_guard_test".to_string(),
        workspace_id: "chat_patch_guard_test".to_string(),
        stable_chat_id: "stable_patch_guard_test".to_string(),
        chat_ref: AgentChatRef::Character {
            character_id: "Seraphina".to_string(),
            file_name: "Seraphina.png".to_string(),
        },
        generation_type: "normal".to_string(),
        profile_id: None,
        skill_scope_refs: Default::default(),
        persist_base_state_id: None,
        input_message_count: None,
        presentation: AgentRunPresentation::Background,
        status: AgentRunStatus::Created,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    repository.create_run(&run).await.expect("create run");
    let profile = test_resolved_profile(&root).await;
    repository
        .initialize_run(
            &run,
            &build_agent_manifest(&run, &profile),
            &json!({"messages": []}),
            &profile,
        )
        .await
        .expect("initialize workspace");
    repository
        .write_text(
            &run.id,
            &WorkspacePath::parse("output/main.md").unwrap(),
            "hello draft\nunchanged tail",
        )
        .await
        .expect("seed artifact");

    let dispatcher = test_dispatcher(repository.clone(), &root);
    let mut session = AgentToolSession::default();
    let patch_call = AgentToolCall {
        id: "call_patch".to_string(),
        name: "workspace.apply_patch".to_string(),
        arguments: json!({
            "path": "output/main.md",
            "old_string": "draft",
            "new_string": "final",
        }),
        provider_metadata: Value::Null,
    };

    let first = dispatcher
        .dispatch(&run.id, &patch_call, &mut session, &profile)
        .await
        .expect("dispatch patch");
    assert!(first.result.is_error);
    assert_eq!(
        first.result.error_code.as_deref(),
        Some("workspace.patch_requires_read")
    );

    let read_call = AgentToolCall {
        id: "call_read".to_string(),
        name: "workspace.read_file".to_string(),
        arguments: json!({
            "path": "output/main.md",
            "start_line": 1,
            "line_count": 1,
        }),
        provider_metadata: Value::Null,
    };
    let read = dispatcher
        .dispatch(&run.id, &read_call, &mut session, &profile)
        .await
        .expect("dispatch read");
    assert_eq!(read.result.structured["fullRead"], false);

    let patched = dispatcher
        .dispatch(&run.id, &patch_call, &mut session, &profile)
        .await
        .expect("dispatch patch after read");
    assert!(!patched.result.is_error);
    assert!(matches!(
        patched.effect,
        AgentToolEffect::WorkspaceFilePatched {
            replacements: 1,
            ..
        }
    ));

    let artifact = repository
        .read_text(&run.id, &WorkspacePath::parse("output/main.md").unwrap())
        .await
        .expect("read artifact");
    assert_eq!(artifact.text, "hello final\nunchanged tail");

    let second_patch_call = AgentToolCall {
        id: "call_patch_again".to_string(),
        name: "workspace.apply_patch".to_string(),
        arguments: json!({
            "path": "output/main.md",
            "old_string": "hello final",
            "new_string": "hello done",
        }),
        provider_metadata: Value::Null,
    };
    let second_patch = dispatcher
        .dispatch(&run.id, &second_patch_call, &mut session, &profile)
        .await
        .expect("dispatch second patch after partial patch");
    assert!(!second_patch.result.is_error);
    let artifact = repository
        .read_text(&run.id, &WorkspacePath::parse("output/main.md").unwrap())
        .await
        .expect("read second patched artifact");
    assert_eq!(artifact.text, "hello done\nunchanged tail");

    tokio::fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn workspace_patch_partial_failure_requires_full_read_before_retry() {
    let root = std::env::temp_dir().join(format!(
        "tauritavern-agent-patch-partial-failure-{}",
        Uuid::new_v4().simple()
    ));
    let repository = Arc::new(FileAgentRepository::new(root.clone()));
    let run = AgentRun {
        id: "run_patch_partial_failure_test".to_string(),
        workspace_id: "chat_patch_partial_failure_test".to_string(),
        stable_chat_id: "stable_patch_partial_failure_test".to_string(),
        chat_ref: AgentChatRef::Character {
            character_id: "Seraphina".to_string(),
            file_name: "Seraphina.png".to_string(),
        },
        generation_type: "normal".to_string(),
        profile_id: None,
        skill_scope_refs: Default::default(),
        persist_base_state_id: None,
        input_message_count: None,
        presentation: AgentRunPresentation::Background,
        status: AgentRunStatus::Created,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    repository.create_run(&run).await.expect("create run");
    let profile = test_resolved_profile(&root).await;
    repository
        .initialize_run(
            &run,
            &build_agent_manifest(&run, &profile),
            &json!({"messages": []}),
            &profile,
        )
        .await
        .expect("initialize workspace");
    repository
        .write_text(
            &run.id,
            &WorkspacePath::parse("output/main.md").unwrap(),
            "alpha target\nomega target\nzeta",
        )
        .await
        .expect("seed artifact");

    let dispatcher = test_dispatcher(repository.clone(), &root);
    let mut session = AgentToolSession::default();
    let read_first_line = AgentToolCall {
        id: "call_read_first_line".to_string(),
        name: "workspace.read_file".to_string(),
        arguments: json!({
            "path": "output/main.md",
            "start_line": 1,
            "line_count": 1,
        }),
        provider_metadata: Value::Null,
    };
    dispatcher
        .dispatch(&run.id, &read_first_line, &mut session, &profile)
        .await
        .expect("dispatch partial read");

    let ambiguous_patch_call = AgentToolCall {
        id: "call_ambiguous_patch".to_string(),
        name: "workspace.apply_patch".to_string(),
        arguments: json!({
            "path": "output/main.md",
            "old_string": "target",
            "new_string": "TARGET",
        }),
        provider_metadata: Value::Null,
    };
    let ambiguous = dispatcher
        .dispatch(&run.id, &ambiguous_patch_call, &mut session, &profile)
        .await
        .expect("dispatch ambiguous partial patch");
    assert!(ambiguous.result.is_error);
    assert_eq!(
        ambiguous.result.error_code.as_deref(),
        Some("workspace.patch_old_string_not_unique")
    );
    assert!(ambiguous.result.content.contains("Fully read the file"));

    let specific_patch_call = AgentToolCall {
        id: "call_specific_patch_before_full_read".to_string(),
        name: "workspace.apply_patch".to_string(),
        arguments: json!({
            "path": "output/main.md",
            "old_string": "alpha target",
            "new_string": "alpha TARGET",
        }),
        provider_metadata: Value::Null,
    };
    let still_requires_full_read = dispatcher
        .dispatch(&run.id, &specific_patch_call, &mut session, &profile)
        .await
        .expect("dispatch patch after failed partial patch");
    assert!(still_requires_full_read.result.is_error);
    assert_eq!(
        still_requires_full_read.result.error_code.as_deref(),
        Some("workspace.patch_requires_full_read")
    );

    let full_read = AgentToolCall {
        id: "call_full_read".to_string(),
        name: "workspace.read_file".to_string(),
        arguments: json!({ "path": "output/main.md" }),
        provider_metadata: Value::Null,
    };
    let full_read_result = dispatcher
        .dispatch(&run.id, &full_read, &mut session, &profile)
        .await
        .expect("dispatch full read");
    assert_eq!(full_read_result.result.structured["fullRead"], true);

    let patched = dispatcher
        .dispatch(&run.id, &specific_patch_call, &mut session, &profile)
        .await
        .expect("dispatch patch after full read");
    assert!(!patched.result.is_error);
    let artifact = repository
        .read_text(&run.id, &WorkspacePath::parse("output/main.md").unwrap())
        .await
        .expect("read artifact");
    assert_eq!(artifact.text, "alpha TARGET\nomega target\nzeta");

    let mut replace_all_session = AgentToolSession::default();
    let read_second_line = AgentToolCall {
        id: "call_read_second_line".to_string(),
        name: "workspace.read_file".to_string(),
        arguments: json!({
            "path": "output/main.md",
            "start_line": 2,
            "line_count": 1,
        }),
        provider_metadata: Value::Null,
    };
    dispatcher
        .dispatch(
            &run.id,
            &read_second_line,
            &mut replace_all_session,
            &profile,
        )
        .await
        .expect("dispatch second partial read");

    let replace_all_call = AgentToolCall {
        id: "call_replace_all_partial".to_string(),
        name: "workspace.apply_patch".to_string(),
        arguments: json!({
            "path": "output/main.md",
            "old_string": "target",
            "new_string": "TARGET",
            "replace_all": true,
        }),
        provider_metadata: Value::Null,
    };
    let replace_all_without_full_read = dispatcher
        .dispatch(
            &run.id,
            &replace_all_call,
            &mut replace_all_session,
            &profile,
        )
        .await
        .expect("dispatch replace_all after partial read");
    assert!(replace_all_without_full_read.result.is_error);
    assert_eq!(
        replace_all_without_full_read.result.error_code.as_deref(),
        Some("workspace.patch_requires_full_read")
    );

    let full_read_for_replace_all = AgentToolCall {
        id: "call_full_read_for_replace_all".to_string(),
        name: "workspace.read_file".to_string(),
        arguments: json!({ "path": "output/main.md" }),
        provider_metadata: Value::Null,
    };
    dispatcher
        .dispatch(
            &run.id,
            &full_read_for_replace_all,
            &mut replace_all_session,
            &profile,
        )
        .await
        .expect("dispatch full read before replace_all");
    let replaced_all = dispatcher
        .dispatch(
            &run.id,
            &replace_all_call,
            &mut replace_all_session,
            &profile,
        )
        .await
        .expect("dispatch replace_all after full read");
    assert!(!replaced_all.result.is_error);
    let artifact = repository
        .read_text(&run.id, &WorkspacePath::parse("output/main.md").unwrap())
        .await
        .expect("read replace_all artifact");
    assert_eq!(artifact.text, "alpha TARGET\nomega TARGET\nzeta");

    tokio::fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn workspace_write_file_uses_session_cas_for_existing_files() {
    let root = std::env::temp_dir().join(format!(
        "tauritavern-agent-write-cas-{}",
        Uuid::new_v4().simple()
    ));
    let repository = Arc::new(FileAgentRepository::new(root.clone()));
    let run = AgentRun {
        id: "run_write_cas_test".to_string(),
        workspace_id: "chat_write_cas_test".to_string(),
        stable_chat_id: "stable_write_cas_test".to_string(),
        chat_ref: AgentChatRef::Character {
            character_id: "Seraphina".to_string(),
            file_name: "Seraphina.png".to_string(),
        },
        generation_type: "normal".to_string(),
        profile_id: None,
        skill_scope_refs: Default::default(),
        persist_base_state_id: None,
        input_message_count: None,
        presentation: AgentRunPresentation::Background,
        status: AgentRunStatus::Created,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    repository.create_run(&run).await.expect("create run");
    let profile = test_resolved_profile(&root).await;
    repository
        .initialize_run(
            &run,
            &build_agent_manifest(&run, &profile),
            &json!({"messages": []}),
            &profile,
        )
        .await
        .expect("initialize workspace");
    repository
        .write_text(
            &run.id,
            &WorkspacePath::parse("output/main.md").unwrap(),
            "first draft",
        )
        .await
        .expect("seed artifact");

    let dispatcher = test_dispatcher(repository.clone(), &root);
    let mut session = AgentToolSession::default();
    let write_call = AgentToolCall {
        id: "call_write".to_string(),
        name: "workspace.write_file".to_string(),
        arguments: json!({
            "path": "output/main.md",
            "content": "model rewrite",
        }),
        provider_metadata: Value::Null,
    };

    let without_read = dispatcher
        .dispatch(&run.id, &write_call, &mut session, &profile)
        .await
        .expect("dispatch write without read");
    assert!(without_read.result.is_error);
    assert_eq!(
        without_read.result.error_code.as_deref(),
        Some("workspace.write_requires_read")
    );

    let read_call = AgentToolCall {
        id: "call_read".to_string(),
        name: "workspace.read_file".to_string(),
        arguments: json!({ "path": "output/main.md" }),
        provider_metadata: Value::Null,
    };
    dispatcher
        .dispatch(&run.id, &read_call, &mut session, &profile)
        .await
        .expect("dispatch read");
    repository
        .write_text(
            &run.id,
            &WorkspacePath::parse("output/main.md").unwrap(),
            "concurrent rewrite",
        )
        .await
        .expect("simulate concurrent write");

    let stale = dispatcher
        .dispatch(&run.id, &write_call, &mut session, &profile)
        .await
        .expect("dispatch stale write");
    assert!(stale.result.is_error);
    assert_eq!(
        stale.result.error_code.as_deref(),
        Some("workspace.write_stale_file")
    );
    assert!(
        stale
            .result
            .content
            .contains("file changed since you last read or wrote it")
    );
    assert!(!stale.result.content.contains("sha256"));

    dispatcher
        .dispatch(&run.id, &read_call, &mut session, &profile)
        .await
        .expect("dispatch reread");
    let written = dispatcher
        .dispatch(&run.id, &write_call, &mut session, &profile)
        .await
        .expect("dispatch write after reread");
    assert!(!written.result.is_error);
    let artifact = repository
        .read_text(&run.id, &WorkspacePath::parse("output/main.md").unwrap())
        .await
        .expect("read artifact");
    assert_eq!(artifact.text, "model rewrite");

    tokio::fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn workspace_write_file_append_mode_adds_text_without_rewrite_read() {
    let root = std::env::temp_dir().join(format!(
        "tauritavern-agent-write-append-{}",
        Uuid::new_v4().simple()
    ));
    let repository = Arc::new(FileAgentRepository::new(root.clone()));
    let run = AgentRun {
        id: "run_write_append_test".to_string(),
        workspace_id: "chat_write_append_test".to_string(),
        stable_chat_id: "stable_write_append_test".to_string(),
        chat_ref: AgentChatRef::Character {
            character_id: "Seraphina".to_string(),
            file_name: "Seraphina.png".to_string(),
        },
        generation_type: "normal".to_string(),
        profile_id: None,
        skill_scope_refs: Default::default(),
        persist_base_state_id: None,
        input_message_count: None,
        presentation: AgentRunPresentation::Background,
        status: AgentRunStatus::Created,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    repository.create_run(&run).await.expect("create run");
    let profile = test_resolved_profile(&root).await;
    repository
        .initialize_run(
            &run,
            &build_agent_manifest(&run, &profile),
            &json!({"messages": []}),
            &profile,
        )
        .await
        .expect("initialize workspace");
    repository
        .write_text(
            &run.id,
            &WorkspacePath::parse("output/main.md").unwrap(),
            "first",
        )
        .await
        .expect("seed artifact");

    let dispatcher = test_dispatcher(repository.clone(), &root);
    let mut session = AgentToolSession::default();
    let append_call = AgentToolCall {
        id: "call_append".to_string(),
        name: "workspace.write_file".to_string(),
        arguments: json!({
            "path": "output/main.md",
            "content": "\nsecond",
            "mode": "append",
        }),
        provider_metadata: Value::Null,
    };

    let appended = dispatcher
        .dispatch(&run.id, &append_call, &mut session, &profile)
        .await
        .expect("dispatch append");
    assert!(!appended.result.is_error);
    assert_eq!(appended.result.structured["mode"], "append");
    assert!(matches!(
        appended.effect,
        AgentToolEffect::WorkspaceFileWritten {
            mode: WorkspaceFileWriteMode::Append,
            ..
        }
    ));
    let artifact = repository
        .read_text(&run.id, &WorkspacePath::parse("output/main.md").unwrap())
        .await
        .expect("read artifact");
    assert_eq!(artifact.text, "first\nsecond");

    let replace_after_unread_append_call = AgentToolCall {
        id: "call_replace_after_unread_append".to_string(),
        name: "workspace.write_file".to_string(),
        arguments: json!({
            "path": "output/main.md",
            "content": "rewritten without read",
            "mode": "replace",
        }),
        provider_metadata: Value::Null,
    };
    let replace_after_unread_append = dispatcher
        .dispatch(
            &run.id,
            &replace_after_unread_append_call,
            &mut session,
            &profile,
        )
        .await
        .expect("dispatch replace after unread append");
    assert!(replace_after_unread_append.result.is_error);
    assert_eq!(
        replace_after_unread_append.result.error_code.as_deref(),
        Some("workspace.write_requires_read")
    );

    let patch_call = AgentToolCall {
        id: "call_patch_after_append".to_string(),
        name: "workspace.apply_patch".to_string(),
        arguments: json!({
            "path": "output/main.md",
            "old_string": "first",
            "new_string": "FIRST",
        }),
        provider_metadata: Value::Null,
    };
    let patch_without_full_read = dispatcher
        .dispatch(&run.id, &patch_call, &mut session, &profile)
        .await
        .expect("dispatch patch after append");
    assert!(patch_without_full_read.result.is_error);
    assert_eq!(
        patch_without_full_read.result.error_code.as_deref(),
        Some("workspace.patch_requires_read")
    );

    let invalid_mode_call = AgentToolCall {
        id: "call_invalid_append_mode".to_string(),
        name: "workspace.write_file".to_string(),
        arguments: json!({
            "path": "output/main.md",
            "content": "ignored",
            "mode": "merge",
        }),
        provider_metadata: Value::Null,
    };
    let invalid_mode = dispatcher
        .dispatch(&run.id, &invalid_mode_call, &mut session, &profile)
        .await
        .expect("dispatch invalid write mode");
    assert!(invalid_mode.result.is_error);
    assert_eq!(
        invalid_mode.result.error_code.as_deref(),
        Some("workspace.write_mode_invalid")
    );

    let append_new_call = AgentToolCall {
        id: "call_append_new".to_string(),
        name: "workspace.write_file".to_string(),
        arguments: json!({
            "path": "output/new.md",
            "content": "created by append",
            "mode": "append",
        }),
        provider_metadata: Value::Null,
    };
    let appended_new = dispatcher
        .dispatch(&run.id, &append_new_call, &mut session, &profile)
        .await
        .expect("dispatch append new file");
    assert!(!appended_new.result.is_error);
    let new_file = repository
        .read_text(&run.id, &WorkspacePath::parse("output/new.md").unwrap())
        .await
        .expect("read appended new file");
    assert_eq!(new_file.text, "created by append");

    let replace_new_call = AgentToolCall {
        id: "call_replace_append_new".to_string(),
        name: "workspace.write_file".to_string(),
        arguments: json!({
            "path": "output/new.md",
            "content": "rewritten after append create",
            "mode": "replace",
        }),
        provider_metadata: Value::Null,
    };
    let replaced_new = dispatcher
        .dispatch(&run.id, &replace_new_call, &mut session, &profile)
        .await
        .expect("dispatch replace appended new file");
    assert!(!replaced_new.result.is_error);
    let new_file = repository
        .read_text(&run.id, &WorkspacePath::parse("output/new.md").unwrap())
        .await
        .expect("read replaced new file");
    assert_eq!(new_file.text, "rewritten after append create");

    repository
        .write_text(
            &run.id,
            &WorkspacePath::parse("output/known.md").unwrap(),
            "alpha",
        )
        .await
        .expect("seed known artifact");
    let read_known_call = AgentToolCall {
        id: "call_read_known_before_append".to_string(),
        name: "workspace.read_file".to_string(),
        arguments: json!({
            "path": "output/known.md",
        }),
        provider_metadata: Value::Null,
    };
    let read_known = dispatcher
        .dispatch(&run.id, &read_known_call, &mut session, &profile)
        .await
        .expect("dispatch read known file");
    assert!(!read_known.result.is_error);
    assert_eq!(read_known.result.structured["fullRead"], true);

    let append_known_call = AgentToolCall {
        id: "call_append_known".to_string(),
        name: "workspace.write_file".to_string(),
        arguments: json!({
            "path": "output/known.md",
            "content": "\nbeta",
            "mode": "append",
        }),
        provider_metadata: Value::Null,
    };
    let appended_known = dispatcher
        .dispatch(&run.id, &append_known_call, &mut session, &profile)
        .await
        .expect("dispatch append known file");
    assert!(!appended_known.result.is_error);

    let replace_known_call = AgentToolCall {
        id: "call_replace_known_after_append".to_string(),
        name: "workspace.write_file".to_string(),
        arguments: json!({
            "path": "output/known.md",
            "content": "rewritten after known append",
            "mode": "replace",
        }),
        provider_metadata: Value::Null,
    };
    let replaced_known = dispatcher
        .dispatch(&run.id, &replace_known_call, &mut session, &profile)
        .await
        .expect("dispatch replace known file");
    assert!(!replaced_known.result.is_error);
    let known_file = repository
        .read_text(&run.id, &WorkspacePath::parse("output/known.md").unwrap())
        .await
        .expect("read replaced known file");
    assert_eq!(known_file.text, "rewritten after known append");

    tokio::fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn dispatcher_searches_and_reads_current_chat_messages() {
    let root = std::env::temp_dir().join(format!(
        "tauritavern-agent-chat-tools-{}",
        Uuid::new_v4().simple()
    ));
    let repository = Arc::new(FileAgentRepository::new(root.clone()));
    let chat_repository = test_chat_repository(&root);
    let run = AgentRun {
        id: "run_chat_tools_test".to_string(),
        workspace_id: "chat_tools_test".to_string(),
        stable_chat_id: "stable_chat_tools_test".to_string(),
        chat_ref: AgentChatRef::Character {
            character_id: "alice".to_string(),
            file_name: "session".to_string(),
        },
        generation_type: "normal".to_string(),
        profile_id: None,
        skill_scope_refs: Default::default(),
        persist_base_state_id: None,
        input_message_count: None,
        presentation: AgentRunPresentation::Background,
        status: AgentRunStatus::Created,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    repository.create_run(&run).await.expect("create run");
    let profile = test_resolved_profile(&root).await;
    repository
        .initialize_run(
            &run,
            &build_agent_manifest(&run, &profile),
            &json!({"messages": []}),
            &profile,
        )
        .await
        .expect("initialize workspace");
    save_character_payload(
        &chat_repository,
        &root,
        "alice",
        "session",
        &[
            json!({
                "chat_metadata": {},
                "user_name": "unused",
                "character_name": "unused",
            }),
            json!({
                "name": "User",
                "is_user": true,
                "is_system": false,
                "send_date": "2026-01-01T00:00:00.000Z",
                "mes": "hello",
                "extra": {},
            }),
            json!({
                "name": "Alice",
                "is_user": false,
                "is_system": false,
                "send_date": "2026-01-01T00:00:01.000Z",
                "mes": "the blue lantern is hidden under the bridge",
                "extra": {},
            }),
        ],
    )
    .await;

    let dispatcher = AgentToolDispatcher::new(
        repository.clone(),
        chat_repository.clone(),
        chat_repository,
        repository.clone(),
        test_skill_service(&root),
    );
    let mut session = AgentToolSession::default();
    let search_call = AgentToolCall {
        id: "call_search".to_string(),
        name: "chat.search".to_string(),
        arguments: json!({ "query": "blue lantern" }),
        provider_metadata: Value::Null,
    };
    let searched = dispatcher
        .dispatch(&run.id, &search_call, &mut session, &profile)
        .await
        .expect("dispatch search");
    assert!(!searched.result.is_error);
    assert_eq!(searched.result.structured["hits"][0]["index"], 1);
    assert!(searched.result.structured["hits"][0].get("text").is_none());

    let read_call = AgentToolCall {
        id: "call_read_messages".to_string(),
        name: "chat.read_messages".to_string(),
        arguments: json!({
            "messages": [{ "index": 1, "start_char": 4, "max_chars": 12 }]
        }),
        provider_metadata: Value::Null,
    };
    let read = dispatcher
        .dispatch(&run.id, &read_call, &mut session, &profile)
        .await
        .expect("dispatch read messages");
    assert!(!read.result.is_error);
    assert_eq!(
        read.result.structured["messages"][0]["text"],
        "blue lantern"
    );
    assert_eq!(read.result.structured["messages"][0]["chars"], 12);
    assert_eq!(read.result.structured["messages"][0]["words"], 2);
    assert_eq!(read.result.structured["messages"][0]["totalWords"], 8);
    assert_eq!(read.result.structured["messages"][0]["truncated"], true);
    assert_eq!(read.result.resource_refs[0], "chat:current#1:chars=4..16");

    tokio::fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn agent_input_context_excludes_swipe_target_from_history_and_persist_base() {
    let root = std::env::temp_dir().join(format!(
        "tauritavern-agent-input-context-{}",
        Uuid::new_v4().simple()
    ));
    tokio::fs::create_dir_all(&root).await.expect("create root");
    let repository = Arc::new(FileAgentRepository::new(root.clone()));
    let chat_repository = test_chat_repository(&root);
    let service = AgentRuntimeService::new(
        repository.clone(),
        repository.clone(),
        repository.clone(),
        repository,
        chat_repository.clone(),
        chat_repository.clone(),
        test_skill_service(&root),
        Arc::new(MockAgentModelGateway::new(vec![])),
        test_profile_service(&root),
        test_llm_connection_service(&root),
    );
    save_character_payload(
        &chat_repository,
        &root,
        "alice",
        "session",
        &[
            json!({
                "chat_metadata": {},
                "user_name": "unused",
                "character_name": "unused",
            }),
            json!({
                "name": "User",
                "is_user": true,
                "is_system": false,
                "mes": "hello",
                "extra": {},
            }),
            json!({
                "name": "Alice",
                "is_user": false,
                "is_system": false,
                "mes": "visible assistant",
                "extra": {
                    "tauritavern": {
                        "agent": {
                            "persistStateStatus": "committed",
                            "persistStateId": "state_visible"
                        }
                    }
                },
            }),
            json!({
                "name": "Alice",
                "is_user": false,
                "is_system": false,
                "mes": "old swipe target",
                "extra": {
                    "tauritavern": {
                        "agent": {
                            "persistStateStatus": "committed",
                            "persistStateId": "state_hidden"
                        }
                    }
                },
            }),
        ],
    )
    .await;

    let context = service
        .resolve_agent_run_input_context(
            &AgentChatRef::Character {
                character_id: "alice".to_string(),
                file_name: "session".to_string(),
            },
            "swipe",
        )
        .await
        .expect("resolve input context");

    assert_eq!(context.input_message_count, 2);
    assert_eq!(
        context.persist_base_state_id,
        Some("state_visible".to_string())
    );

    tokio::fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn dispatcher_chat_tools_hide_messages_after_run_input_boundary() {
    let root = std::env::temp_dir().join(format!(
        "tauritavern-agent-chat-boundary-{}",
        Uuid::new_v4().simple()
    ));
    let repository = Arc::new(FileAgentRepository::new(root.clone()));
    let chat_repository = test_chat_repository(&root);
    let run = AgentRun {
        id: "run_chat_boundary_test".to_string(),
        workspace_id: "chat_boundary_test".to_string(),
        stable_chat_id: "stable_chat_boundary_test".to_string(),
        chat_ref: AgentChatRef::Character {
            character_id: "alice".to_string(),
            file_name: "session".to_string(),
        },
        generation_type: "swipe".to_string(),
        profile_id: None,
        skill_scope_refs: Default::default(),
        persist_base_state_id: None,
        input_message_count: Some(2),
        presentation: AgentRunPresentation::Background,
        status: AgentRunStatus::Created,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    repository.create_run(&run).await.expect("create run");
    save_character_payload(
        &chat_repository,
        &root,
        "alice",
        "session",
        &[
            json!({
                "chat_metadata": {},
                "user_name": "unused",
                "character_name": "unused",
            }),
            json!({
                "name": "User",
                "is_user": true,
                "is_system": false,
                "send_date": "2026-01-01T00:00:00.000Z",
                "mes": "hello",
                "extra": {},
            }),
            json!({
                "name": "Alice",
                "is_user": false,
                "is_system": false,
                "send_date": "2026-01-01T00:00:01.000Z",
                "mes": "the blue lantern is hidden under the bridge",
                "extra": {},
            }),
            json!({
                "name": "Alice",
                "is_user": false,
                "is_system": false,
                "send_date": "2026-01-01T00:00:02.000Z",
                "mes": "zirconium old swipe target",
                "extra": {},
            }),
        ],
    )
    .await;

    let profile = test_resolved_profile(&root).await;
    let dispatcher = AgentToolDispatcher::new(
        repository.clone(),
        chat_repository.clone(),
        chat_repository,
        repository,
        test_skill_service(&root),
    );
    let mut session = AgentToolSession::default();

    let hidden_search_call = AgentToolCall {
        id: "call_hidden_search".to_string(),
        name: "chat.search".to_string(),
        arguments: json!({ "query": "zirconium" }),
        provider_metadata: Value::Null,
    };
    let hidden_search = dispatcher
        .dispatch(&run.id, &hidden_search_call, &mut session, &profile)
        .await
        .expect("dispatch hidden search");
    assert!(!hidden_search.result.is_error);
    assert_eq!(hidden_search.result.structured["hits"], json!([]));

    let visible_search_call = AgentToolCall {
        id: "call_visible_search".to_string(),
        name: "chat.search".to_string(),
        arguments: json!({ "query": "blue lantern", "scan_limit": 1 }),
        provider_metadata: Value::Null,
    };
    let visible_search = dispatcher
        .dispatch(&run.id, &visible_search_call, &mut session, &profile)
        .await
        .expect("dispatch visible search");
    assert!(!visible_search.result.is_error);
    assert_eq!(visible_search.result.structured["hits"][0]["index"], 1);

    let hidden_read_call = AgentToolCall {
        id: "call_hidden_read".to_string(),
        name: "chat.read_messages".to_string(),
        arguments: json!({ "messages": [{ "index": 2 }] }),
        provider_metadata: Value::Null,
    };
    let hidden_read = dispatcher
        .dispatch(&run.id, &hidden_read_call, &mut session, &profile)
        .await
        .expect("dispatch hidden read");
    assert!(hidden_read.result.is_error);
    assert_eq!(
        hidden_read.result.error_code.as_deref(),
        Some("chat.message_not_found")
    );
    assert!(hidden_read.result.content.contains("total messages: 2"));

    tokio::fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn dispatcher_searches_visible_workspace_files_and_reads_char_ranges() {
    let root = std::env::temp_dir().join(format!(
        "tauritavern-agent-workspace-search-{}",
        Uuid::new_v4().simple()
    ));
    let repository = Arc::new(FileAgentRepository::new(root.clone()));
    let run = AgentRun {
        id: "run_workspace_search_test".to_string(),
        workspace_id: "workspace_search_test".to_string(),
        stable_chat_id: "stable_workspace_search_test".to_string(),
        chat_ref: AgentChatRef::Character {
            character_id: "alice".to_string(),
            file_name: "session".to_string(),
        },
        generation_type: "normal".to_string(),
        profile_id: None,
        skill_scope_refs: Default::default(),
        persist_base_state_id: None,
        input_message_count: None,
        presentation: AgentRunPresentation::Background,
        status: AgentRunStatus::Created,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    repository.create_run(&run).await.expect("create run");
    let profile = test_resolved_profile(&root).await;
    repository
        .initialize_run(
            &run,
            &build_agent_manifest(&run, &profile),
            &json!({"messages": []}),
            &profile,
        )
        .await
        .expect("initialize workspace");
    repository
        .write_text(
            &run.id,
            &WorkspacePath::parse("persist/memory.md").unwrap(),
            "alpha\nblue lantern under the bridge\nomega",
        )
        .await
        .expect("seed persist");

    let dispatcher = test_dispatcher(repository.clone(), &root);
    let mut session = AgentToolSession::default();
    let search_call = AgentToolCall {
        id: "call_workspace_search".to_string(),
        name: "workspace.search_files".to_string(),
        arguments: json!({ "query": "blue lantern", "path": "persist/", "context_lines": 0 }),
        provider_metadata: Value::Null,
    };
    let searched = dispatcher
        .dispatch(&run.id, &search_call, &mut session, &profile)
        .await
        .expect("dispatch workspace search");
    assert!(!searched.result.is_error);
    assert_eq!(
        searched.result.structured["hits"][0]["path"],
        "persist/memory.md"
    );
    assert_eq!(searched.result.structured["hits"][0]["startLine"], 2);

    let char_read_call = AgentToolCall {
        id: "call_workspace_char_read".to_string(),
        name: "workspace.read_file".to_string(),
        arguments: json!({ "path": "persist/memory.md", "start_char": 6, "max_chars": 12 }),
        provider_metadata: Value::Null,
    };
    let read = dispatcher
        .dispatch(&run.id, &char_read_call, &mut session, &profile)
        .await
        .expect("dispatch char read");
    assert!(!read.result.is_error);
    assert!(read.result.content.contains("blue lantern"));
    assert_eq!(read.result.structured["startChar"], 6);
    assert_eq!(read.result.structured["endChar"], 18);
    assert_eq!(read.result.structured["chars"], 12);
    assert_eq!(read.result.structured["words"], 2);
    assert_eq!(read.result.structured["totalWords"], 7);
    assert_eq!(read.result.structured["truncated"], true);

    tokio::fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn dispatcher_searches_skills_and_reads_skill_ranges() {
    let root = std::env::temp_dir().join(format!(
        "tauritavern-agent-skill-search-{}",
        Uuid::new_v4().simple()
    ));
    let repository = Arc::new(FileAgentRepository::new(root.clone()));
    let skill_repository = Arc::new(FileSkillRepository::new(root.join("skills")));
    skill_repository
        .install_import(SkillInstallRequest {
            target_scope: SkillScope::Global,
            input: SkillImportInput::InlineFiles {
                files: vec![
                    SkillInlineFile {
                        path: "SKILL.md".to_string(),
                        encoding: "utf8".to_string(),
                        content: "---\nname: test-skill\ndescription: Skill for search tests.\n---\n\n# Test\n".to_string(),
                        media_type: None,
                        size_bytes: None,
                        sha256: None,
                    },
                    SkillInlineFile {
                        path: "references/guide.md".to_string(),
                        encoding: "utf8".to_string(),
                        content: "alpha\nblue lantern under the bridge\nomega".to_string(),
                        media_type: None,
                        size_bytes: None,
                        sha256: None,
                    },
                ],
                source: json!({ "kind": "test" }),
            },
            conflict_strategy: None,
        })
        .await
        .expect("install skill");
    let skill_service = Arc::new(SkillService::new(skill_repository));
    let profile = test_resolved_profile(&root).await;
    let effective_skills = skill_service
        .resolve_effective_skills(&[SkillScope::Global], &profile.skills)
        .await
        .expect("resolve effective skills");
    let dispatcher = AgentToolDispatcher::new(
        repository.clone(),
        test_chat_repository(&root),
        test_chat_repository(&root),
        repository,
        skill_service.clone(),
    );
    let mut session = AgentToolSession::new(effective_skills);

    let search_call = AgentToolCall {
        id: "call_skill_search".to_string(),
        name: "skill.search".to_string(),
        arguments: json!({ "name": "test-skill", "query": "blue lantern", "path": "references", "context_lines": 0 }),
        provider_metadata: Value::Null,
    };
    let searched = dispatcher
        .dispatch("unused", &search_call, &mut session, &profile)
        .await
        .expect("dispatch skill search");
    assert!(!searched.result.is_error);
    assert_eq!(
        searched.result.structured["hits"][0]["path"],
        "references/guide.md"
    );

    let read_call = AgentToolCall {
        id: "call_skill_read_range".to_string(),
        name: "skill.read".to_string(),
        arguments: json!({
            "name": "test-skill",
            "path": "references/guide.md",
            "start_line": 2,
            "line_count": 1
        }),
        provider_metadata: Value::Null,
    };
    let read = dispatcher
        .dispatch("unused", &read_call, &mut session, &profile)
        .await
        .expect("dispatch skill read");
    assert!(!read.result.is_error);
    assert!(
        read.result
            .content
            .contains("blue lantern under the bridge")
    );
    assert_eq!(read.result.structured["startLine"], 2);
    assert_eq!(read.result.structured["endLine"], 2);
    assert_eq!(read.result.structured["chars"], 29);
    assert_eq!(read.result.structured["words"], 5);
    assert_eq!(read.result.structured["totalWords"], 7);
    assert_eq!(read.result.structured["truncated"], true);

    tokio::fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn dispatcher_uses_profile_skill_read_budget_above_default_fallback() {
    let root = std::env::temp_dir().join(format!(
        "tauritavern-agent-skill-profile-budget-{}",
        Uuid::new_v4().simple()
    ));
    let repository = Arc::new(FileAgentRepository::new(root.clone()));
    let skill_repository = Arc::new(FileSkillRepository::new(root.join("skills")));
    let long_content = "a".repeat(120_000);
    skill_repository
        .install_import(SkillInstallRequest {
            target_scope: SkillScope::Global,
            input: SkillImportInput::InlineFiles {
                files: vec![
                    SkillInlineFile {
                        path: "SKILL.md".to_string(),
                        encoding: "utf8".to_string(),
                        content: "---\nname: test-skill\ndescription: Skill for read budget tests.\n---\n\n# Test\n".to_string(),
                        media_type: None,
                        size_bytes: None,
                        sha256: None,
                    },
                    SkillInlineFile {
                        path: "references/long.md".to_string(),
                        encoding: "utf8".to_string(),
                        content: long_content,
                        media_type: None,
                        size_bytes: None,
                        sha256: None,
                    },
                ],
                source: json!({ "kind": "test" }),
            },
            conflict_strategy: None,
        })
        .await
        .expect("install skill");
    let skill_service = Arc::new(SkillService::new(skill_repository));
    let mut profile = test_resolved_profile(&root).await;
    profile.skills.max_read_chars_per_call = 100_000;
    profile.skills.max_read_chars_per_run = 100_000;
    let effective_skills = skill_service
        .resolve_effective_skills(&[SkillScope::Global], &profile.skills)
        .await
        .expect("resolve effective skills");
    let dispatcher = AgentToolDispatcher::new(
        repository.clone(),
        test_chat_repository(&root),
        test_chat_repository(&root),
        repository,
        skill_service.clone(),
    );
    let mut session = AgentToolSession::new(effective_skills);

    let read_call = AgentToolCall {
        id: "call_skill_read_profile_budget".to_string(),
        name: "skill.read".to_string(),
        arguments: json!({
            "name": "test-skill",
            "path": "references/long.md",
            "max_chars": 100000
        }),
        provider_metadata: Value::Null,
    };
    let read = dispatcher
        .dispatch("unused", &read_call, &mut session, &profile)
        .await
        .expect("dispatch profile-budget skill read");
    assert!(!read.result.is_error);
    assert_eq!(read.result.structured["chars"], 100_000);
    assert_eq!(read.result.structured["totalChars"], 120_000);
    assert_eq!(read.result.structured["truncated"], true);
    assert_eq!(session.skill_read_chars(), 100_000);

    tokio::fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn dispatcher_progressively_reads_worldinfo_activation_from_run_snapshot() {
    let root = std::env::temp_dir().join(format!(
        "tauritavern-agent-worldinfo-tool-{}",
        Uuid::new_v4().simple()
    ));
    let repository = Arc::new(FileAgentRepository::new(root.clone()));
    let run = AgentRun {
        id: "run_worldinfo_tool_test".to_string(),
        workspace_id: "worldinfo_tool_test".to_string(),
        stable_chat_id: "stable_worldinfo_tool_test".to_string(),
        chat_ref: AgentChatRef::Character {
            character_id: "alice".to_string(),
            file_name: "session".to_string(),
        },
        generation_type: "normal".to_string(),
        profile_id: None,
        skill_scope_refs: Default::default(),
        persist_base_state_id: None,
        input_message_count: None,
        presentation: AgentRunPresentation::Background,
        status: AgentRunStatus::Created,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    repository.create_run(&run).await.expect("create run");
    let profile = test_resolved_profile(&root).await;
    repository
        .initialize_run(
            &run,
            &build_agent_manifest(&run, &profile),
            &json!({
                "chatCompletionPayload": {
                    "messages": prompt_messages("hello")
                },
                "worldInfoActivation": {
                    "timestampMs": 1,
                    "trigger": "normal",
                    "entries": [
                        {
                            "world": "lorebook",
                            "uid": 7,
                            "displayName": "Hidden bridge",
                            "constant": false,
                            "position": "before",
                            "content": "The bridge has a hidden blue lantern."
                        },
                        {
                            "world": "lorebook",
                            "uid": 8,
                            "displayName": "Clock tower",
                            "constant": true,
                            "position": "after",
                            "content": "The clock tower bell rings only for agents."
                        }
                    ]
                }
            }),
            &profile,
        )
        .await
        .expect("initialize workspace");

    let dispatcher = test_dispatcher(repository.clone(), &root);
    let mut session = AgentToolSession::default();
    let index_call = AgentToolCall {
        id: "call_worldinfo_index".to_string(),
        name: "worldinfo.read_activated".to_string(),
        arguments: json!({}),
        provider_metadata: Value::Null,
    };
    let index = dispatcher
        .dispatch(&run.id, &index_call, &mut session, &profile)
        .await
        .expect("dispatch worldinfo index");

    assert!(!index.result.is_error);
    assert_eq!(index.result.structured["mode"], "index");
    assert_eq!(index.result.structured["totalEntries"], 2);
    assert_eq!(
        index.result.structured["entries"][0]["ref"],
        "worldinfo:lorebook#7"
    );
    assert_eq!(
        index.result.structured["entries"][0]["totalChars"],
        "The bridge has a hidden blue lantern.".chars().count()
    );
    assert_eq!(index.result.structured["entries"][0]["totalWords"], 7);
    assert!(
        index.result.structured["entries"][0]
            .get("content")
            .is_none()
    );
    assert!(index.result.content.contains("Content is omitted"));
    assert!(index.result.content.contains("worldinfo:lorebook#7"));
    assert!(!index.result.content.contains("hidden blue lantern"));
    assert_eq!(index.result.resource_refs[0], "worldinfo:lorebook#7");
    assert_eq!(index.result.resource_refs[1], "worldinfo:lorebook#8");

    let read_call = AgentToolCall {
        id: "call_worldinfo_read".to_string(),
        name: "worldinfo.read_activated".to_string(),
        arguments: json!({
            "entries": [{
                "ref": "worldinfo:lorebook#7",
                "start_char": 4,
                "max_chars": 6
            }]
        }),
        provider_metadata: Value::Null,
    };
    let read = dispatcher
        .dispatch(&run.id, &read_call, &mut session, &profile)
        .await
        .expect("dispatch worldinfo read");

    assert!(!read.result.is_error);
    assert_eq!(read.result.structured["mode"], "content");
    assert_eq!(read.result.structured["entries"][0]["content"], "bridge");
    assert_eq!(read.result.structured["entries"][0]["startChar"], 4);
    assert_eq!(read.result.structured["entries"][0]["endChar"], 10);
    assert_eq!(read.result.structured["entries"][0]["chars"], 6);
    assert_eq!(read.result.structured["entries"][0]["words"], 1);
    assert_eq!(read.result.structured["entries"][0]["totalWords"], 7);
    assert_eq!(read.result.structured["entries"][0]["truncated"], true);
    assert_eq!(read.result.resource_refs[0], "worldinfo:lorebook#7");

    let suffix_read_call = AgentToolCall {
        id: "call_worldinfo_suffix_read".to_string(),
        name: "worldinfo.read_activated".to_string(),
        arguments: json!({
            "entries": [{
                "ref": "worldinfo:lorebook#7",
                "start_char": 4
            }]
        }),
        provider_metadata: Value::Null,
    };
    let suffix_read = dispatcher
        .dispatch(&run.id, &suffix_read_call, &mut session, &profile)
        .await
        .expect("dispatch worldinfo suffix read");

    assert!(!suffix_read.result.is_error);
    assert_eq!(
        suffix_read.result.structured["entries"][0]["endChar"],
        "The bridge has a hidden blue lantern.".chars().count()
    );
    assert_eq!(
        suffix_read.result.structured["entries"][0]["content"],
        "bridge has a hidden blue lantern."
    );
    assert_eq!(
        suffix_read.result.structured["entries"][0]["truncated"],
        true
    );

    let missing_ref_call = AgentToolCall {
        id: "call_worldinfo_missing".to_string(),
        name: "worldinfo.read_activated".to_string(),
        arguments: json!({
            "entries": [{ "ref": "worldinfo:lorebook#404" }]
        }),
        provider_metadata: Value::Null,
    };
    let missing_ref = dispatcher
        .dispatch(&run.id, &missing_ref_call, &mut session, &profile)
        .await
        .expect("dispatch missing worldinfo ref");
    assert!(missing_ref.result.is_error);
    assert_eq!(
        missing_ref.result.error_code.as_deref(),
        Some("worldinfo.entry_not_found")
    );

    let old_max_chars_call = AgentToolCall {
        id: "call_worldinfo_old_arg".to_string(),
        name: "worldinfo.read_activated".to_string(),
        arguments: json!({ "max_chars": 20000 }),
        provider_metadata: Value::Null,
    };
    let old_max_chars = dispatcher
        .dispatch(&run.id, &old_max_chars_call, &mut session, &profile)
        .await
        .expect("dispatch obsolete worldinfo arg");
    assert!(old_max_chars.result.is_error);
    assert_eq!(
        old_max_chars.result.error_code.as_deref(),
        Some("tool.invalid_arguments")
    );

    tokio::fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn child_worldinfo_reads_run_snapshot_without_exposing_input_workspace() {
    let root = std::env::temp_dir().join(format!(
        "tauritavern-agent-child-worldinfo-tool-{}",
        Uuid::new_v4().simple()
    ));
    let repository = Arc::new(FileAgentRepository::new(root.clone()));
    let service = AgentRuntimeService::new(
        repository.clone(),
        repository.clone(),
        repository.clone(),
        repository.clone(),
        test_chat_repository(&root),
        test_chat_repository(&root),
        test_skill_service(&root),
        Arc::new(MockAgentModelGateway::new(vec![])),
        test_profile_service(&root),
        test_llm_connection_service(&root),
    );
    let run = AgentRun {
        id: "run_child_worldinfo_tool_test".to_string(),
        workspace_id: "child_worldinfo_tool_test".to_string(),
        stable_chat_id: "stable_child_worldinfo_tool_test".to_string(),
        chat_ref: AgentChatRef::Character {
            character_id: "alice".to_string(),
            file_name: "session".to_string(),
        },
        generation_type: "normal".to_string(),
        profile_id: None,
        skill_scope_refs: Default::default(),
        persist_base_state_id: None,
        input_message_count: None,
        presentation: AgentRunPresentation::Background,
        status: AgentRunStatus::Created,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    repository.create_run(&run).await.expect("create run");
    let profile = test_resolved_profile(&root).await;
    repository
        .initialize_run(
            &run,
            &build_agent_manifest(&run, &profile),
            &json!({
                "chatCompletionPayload": {
                    "messages": prompt_messages("hello")
                },
                "worldInfoActivation": {
                    "timestampMs": 1,
                    "trigger": "normal",
                    "entries": [{
                        "world": "lorebook",
                        "uid": 7,
                        "displayName": "Hidden bridge",
                        "constant": false,
                        "position": "before",
                        "content": "The bridge has a hidden blue lantern."
                    }]
                }
            }),
            &profile,
        )
        .await
        .expect("initialize workspace");
    let task = service
        .create_child_task(
            &run.id,
            "inv_root",
            "inv_child_worldinfo".to_string(),
            "task_child_worldinfo".to_string(),
            profile.id.as_str().to_string(),
            "scene-critic".to_string(),
            "call_delegate_worldinfo".to_string(),
            json!({
                "objective": "Read activated World Info."
            }),
            None,
        )
        .await
        .expect("create child task");

    let mut session = AgentToolSession::default();
    let mut commit_ledger = RunCommitLedger::default();
    let (_cancel_sender, mut cancel_receiver) = watch::channel(false);
    let worldinfo_call = AgentToolCall {
        id: "call_child_worldinfo_index".to_string(),
        name: "worldinfo.read_activated".to_string(),
        arguments: json!({}),
        provider_metadata: Value::Null,
    };
    let worldinfo = service
        .dispatch_tool_call(
            &run.id,
            task.child_invocation_id.as_str(),
            AgentInvocationExitPolicy::TaskReturnRequired,
            1,
            &worldinfo_call,
            &mut session,
            &profile,
            0,
            &mut commit_ledger,
            &mut cancel_receiver,
        )
        .await
        .expect("dispatch child worldinfo");
    assert!(!worldinfo.result.is_error);
    assert_eq!(worldinfo.result.structured["totalEntries"], 1);
    assert_eq!(
        worldinfo.result.structured["entries"][0]["ref"],
        "worldinfo:lorebook#7"
    );
    assert!(!worldinfo.result.content.contains("hidden blue lantern"));

    let hidden_input_read_call = AgentToolCall {
        id: "call_child_hidden_input_read".to_string(),
        name: "workspace.read_file".to_string(),
        arguments: json!({ "path": "input/prompt_snapshot.json" }),
        provider_metadata: Value::Null,
    };
    let hidden_input_read = service
        .dispatch_tool_call(
            &run.id,
            task.child_invocation_id.as_str(),
            AgentInvocationExitPolicy::TaskReturnRequired,
            1,
            &hidden_input_read_call,
            &mut session,
            &profile,
            0,
            &mut commit_ledger,
            &mut cancel_receiver,
        )
        .await
        .expect("dispatch child hidden input read");
    assert!(hidden_input_read.result.is_error);
    assert_eq!(
        hidden_input_read.result.error_code.as_deref(),
        Some("workspace.path_not_visible")
    );
    assert!(
        hidden_input_read
            .result
            .content
            .contains("input/prompt_snapshot.json")
    );

    tokio::fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn child_workspace_policy_scopes_manifest_roots_without_mapping() {
    let root = std::env::temp_dir().join(format!(
        "tauritavern-agent-child-workspace-policy-{}",
        Uuid::new_v4().simple()
    ));
    let repository = Arc::new(FileAgentRepository::new(root.clone()));
    let run = AgentRun {
        id: "run_child_workspace_policy_test".to_string(),
        workspace_id: "child_workspace_policy_test".to_string(),
        stable_chat_id: "stable_child_workspace_policy_test".to_string(),
        chat_ref: AgentChatRef::Character {
            character_id: "alice".to_string(),
            file_name: "session".to_string(),
        },
        generation_type: "normal".to_string(),
        profile_id: None,
        skill_scope_refs: Default::default(),
        persist_base_state_id: None,
        input_message_count: None,
        presentation: AgentRunPresentation::Background,
        status: AgentRunStatus::Created,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    repository.create_run(&run).await.expect("create run");
    let root_profile = test_resolved_profile(&root).await;
    repository
        .initialize_run(
            &run,
            &build_agent_manifest(&run, &root_profile),
            &json!({ "chatCompletionPayload": { "messages": prompt_messages("hello") } }),
            &root_profile,
        )
        .await
        .expect("initialize workspace");

    let mut child_profile = root_profile.clone();
    child_profile.workspace.visible_roots = vec!["output".to_string(), "persist".to_string()];
    child_profile.workspace.writable_roots = vec!["output".to_string()];
    let policy_repository = InvocationWorkspaceRepository::new(repository.as_ref(), &child_profile);
    let manifest = policy_repository
        .read_manifest(&run.id)
        .await
        .expect("read invocation manifest");

    let root_spec = |path: &str| {
        manifest
            .roots
            .iter()
            .find(|root| root.path == path)
            .expect("workspace root")
    };
    assert!(root_spec("output").visible);
    assert!(root_spec("output").writable);
    assert!(root_spec("persist").visible);
    assert!(!root_spec("persist").writable);
    assert!(!root_spec("summaries").visible);
    assert!(!root_spec("summaries").writable);
    assert!(manifest.roots.iter().all(|root| !root.path.contains('/')));

    tokio::fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn task_return_rejects_artifact_path_outside_child_visible_roots() {
    let root = std::env::temp_dir().join(format!(
        "tauritavern-agent-task-return-artifact-policy-{}",
        Uuid::new_v4().simple()
    ));
    let repository = Arc::new(FileAgentRepository::new(root.clone()));
    let service = AgentRuntimeService::new(
        repository.clone(),
        repository.clone(),
        repository.clone(),
        repository.clone(),
        test_chat_repository(&root),
        test_chat_repository(&root),
        test_skill_service(&root),
        Arc::new(MockAgentModelGateway::new(vec![])),
        test_profile_service(&root),
        test_llm_connection_service(&root),
    );
    let run = AgentRun {
        id: "run_task_return_artifact_policy_test".to_string(),
        workspace_id: "task_return_artifact_policy_test".to_string(),
        stable_chat_id: "stable_task_return_artifact_policy_test".to_string(),
        chat_ref: AgentChatRef::Character {
            character_id: "alice".to_string(),
            file_name: "session".to_string(),
        },
        generation_type: "normal".to_string(),
        profile_id: None,
        skill_scope_refs: Default::default(),
        persist_base_state_id: None,
        input_message_count: None,
        presentation: AgentRunPresentation::Background,
        status: AgentRunStatus::Created,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    repository.create_run(&run).await.expect("create run");
    let root_profile = test_resolved_profile(&root).await;
    repository
        .initialize_run(
            &run,
            &build_agent_manifest(&run, &root_profile),
            &json!({ "chatCompletionPayload": { "messages": prompt_messages("hello") } }),
            &root_profile,
        )
        .await
        .expect("initialize workspace");
    let mut child_profile = root_profile.clone();
    child_profile.workspace.visible_roots = vec!["output".to_string()];
    child_profile.workspace.writable_roots = vec!["output".to_string()];
    let task = service
        .create_child_task(
            &run.id,
            "inv_root",
            "inv_child_task_return_artifact_policy".to_string(),
            "task_child_task_return_artifact_policy".to_string(),
            child_profile.id.as_str().to_string(),
            "scene-critic".to_string(),
            "call_delegate_artifact_policy".to_string(),
            json!({ "objective": "Return a result." }),
            None,
        )
        .await
        .expect("create child task");

    let mut session = AgentToolSession::default();
    let mut commit_ledger = RunCommitLedger::default();
    let (_cancel_sender, mut cancel_receiver) = watch::channel(false);
    let call = AgentToolCall {
        id: "call_task_return_hidden_artifact".to_string(),
        name: "task.return".to_string(),
        arguments: json!({
            "summary": "Done.",
            "status": "completed",
            "artifacts": [{
                "path": "input/prompt_snapshot.json",
                "kind": "json",
                "role": "evidence"
            }]
        }),
        provider_metadata: Value::Null,
    };
    let outcome = service
        .dispatch_tool_call(
            &run.id,
            task.child_invocation_id.as_str(),
            AgentInvocationExitPolicy::TaskReturnRequired,
            1,
            &call,
            &mut session,
            &child_profile,
            0,
            &mut commit_ledger,
            &mut cancel_receiver,
        )
        .await
        .expect("dispatch task.return");

    assert!(outcome.result.is_error);
    assert_eq!(
        outcome.result.error_code.as_deref(),
        Some("workspace.path_not_visible")
    );
    assert!(outcome.result.content.contains("Regenerate task_return"));
    assert!(
        outcome
            .result
            .content
            .contains("input/prompt_snapshot.json")
    );
    let task = repository
        .load_task(&run.id, task.id.as_str())
        .await
        .expect("load task");
    assert_eq!(task.status, AgentTaskStatus::Queued);
    assert!(task.result_ref.is_none());
    let result_ref =
        WorkspacePath::parse("agent-results/inv_child_task_return_artifact_policy.json").unwrap();
    assert!(matches!(
        repository.read_text(&run.id, &result_ref).await,
        Err(DomainError::NotFound(_))
    ));

    tokio::fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn return_mode_child_write_rejects_non_writable_visible_root() {
    let root = std::env::temp_dir().join(format!(
        "tauritavern-agent-child-write-policy-{}",
        Uuid::new_v4().simple()
    ));
    let repository = Arc::new(FileAgentRepository::new(root.clone()));
    let service = AgentRuntimeService::new(
        repository.clone(),
        repository.clone(),
        repository.clone(),
        repository.clone(),
        test_chat_repository(&root),
        test_chat_repository(&root),
        test_skill_service(&root),
        Arc::new(MockAgentModelGateway::new(vec![])),
        test_profile_service(&root),
        test_llm_connection_service(&root),
    );
    let run = AgentRun {
        id: "run_child_write_policy_test".to_string(),
        workspace_id: "child_write_policy_test".to_string(),
        stable_chat_id: "stable_child_write_policy_test".to_string(),
        chat_ref: AgentChatRef::Character {
            character_id: "alice".to_string(),
            file_name: "session".to_string(),
        },
        generation_type: "normal".to_string(),
        profile_id: None,
        skill_scope_refs: Default::default(),
        persist_base_state_id: None,
        input_message_count: None,
        presentation: AgentRunPresentation::Background,
        status: AgentRunStatus::Created,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    repository.create_run(&run).await.expect("create run");
    let root_profile = test_resolved_profile(&root).await;
    repository
        .initialize_run(
            &run,
            &build_agent_manifest(&run, &root_profile),
            &json!({ "chatCompletionPayload": { "messages": prompt_messages("hello") } }),
            &root_profile,
        )
        .await
        .expect("initialize workspace");
    let mut child_profile = root_profile.clone();
    child_profile.workspace.visible_roots = vec!["output".to_string(), "persist".to_string()];
    child_profile.workspace.writable_roots = vec!["output".to_string()];
    let task = service
        .create_child_task(
            &run.id,
            "inv_root",
            "inv_child_write_policy".to_string(),
            "task_child_write_policy".to_string(),
            child_profile.id.as_str().to_string(),
            "scene-critic".to_string(),
            "call_delegate_write_policy".to_string(),
            json!({ "objective": "Write only allowed files." }),
            None,
        )
        .await
        .expect("create child task");

    let mut session = AgentToolSession::default();
    let mut commit_ledger = RunCommitLedger::default();
    let (_cancel_sender, mut cancel_receiver) = watch::channel(false);
    let call = AgentToolCall {
        id: "call_child_write_persist".to_string(),
        name: "workspace.write_file".to_string(),
        arguments: json!({
            "path": "persist/notes.md",
            "content": "Should not be written."
        }),
        provider_metadata: Value::Null,
    };
    let outcome = service
        .dispatch_tool_call(
            &run.id,
            task.child_invocation_id.as_str(),
            AgentInvocationExitPolicy::TaskReturnRequired,
            1,
            &call,
            &mut session,
            &child_profile,
            0,
            &mut commit_ledger,
            &mut cancel_receiver,
        )
        .await
        .expect("dispatch child write");

    assert!(outcome.result.is_error);
    assert_eq!(
        outcome.result.error_code.as_deref(),
        Some("workspace.path_not_writable")
    );
    let path = WorkspacePath::parse("persist/notes.md").unwrap();
    assert!(matches!(
        repository.read_text(&run.id, &path).await,
        Err(DomainError::NotFound(_))
    ));

    tokio::fs::remove_dir_all(root).await.expect("cleanup");
}

fn prompt_messages(user_content: &str) -> Value {
    json!([
        {
            "role": "system",
            "content": "Materialized Agent System Prompt.",
        },
        {
            "role": "user",
            "content": user_content,
        }
    ])
}

fn assert_hashed_tool_audit_path(path: &str, root: &str) {
    const TOOL_CALL_AUDIT_DIGEST_HEX_CHARS: usize = 16;

    let prefix = format!("{root}/call_");
    assert!(path.starts_with(&prefix), "{path}");
    assert!(path.ends_with(".json"), "{path}");
    assert_eq!(
        path.len(),
        prefix.len() + TOOL_CALL_AUDIT_DIGEST_HEX_CHARS + ".json".len(),
        "{path}"
    );
    let digest = &path[prefix.len()..path.len() - ".json".len()];
    assert!(
        digest.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "{path}"
    );
}

fn tool_results_from_request(request: &AgentModelRequest) -> Vec<&AgentToolResult> {
    request
        .messages
        .iter()
        .filter(|message| message.role == AgentModelRole::Tool)
        .filter_map(|message| message.parts.first())
        .filter_map(|part| match part {
            AgentModelContentPart::ToolResult { result } => Some(result),
            _ => None,
        })
        .collect()
}

struct MockAgentModelGateway {
    responses: Mutex<VecDeque<Result<Value, ApplicationError>>>,
    requests: Mutex<Vec<AgentModelRequest>>,
    closed_sessions: Mutex<Vec<String>>,
}

struct FinishCancelsDelegateModelGateway {
    root_calls: Mutex<usize>,
    requests: Mutex<Vec<AgentModelRequest>>,
    child_started_sender: watch::Sender<bool>,
    child_cancelled_sender: watch::Sender<bool>,
    closed_sessions: Mutex<Vec<String>>,
}

struct PendingDelegateHandoffModelGateway {
    root_calls: Mutex<usize>,
    requests: Mutex<Vec<AgentModelRequest>>,
    closed_sessions: Mutex<Vec<String>>,
}

fn test_skill_service(root: &Path) -> Arc<SkillService> {
    Arc::new(SkillService::new(Arc::new(FileSkillRepository::new(
        root.join("skills"),
    ))))
}

async fn install_inline_skill(
    skill_repository: &Arc<FileSkillRepository>,
    scope: SkillScope,
    name: &str,
    description: &str,
    body: &str,
) {
    skill_repository
        .install_import(SkillInstallRequest {
            target_scope: scope,
            input: SkillImportInput::InlineFiles {
                files: vec![SkillInlineFile {
                    path: "SKILL.md".to_string(),
                    encoding: "utf8".to_string(),
                    content: format!(
                        "---\nname: {name}\ndescription: {description}\n---\n\n{body}\n"
                    ),
                    media_type: None,
                    size_bytes: None,
                    sha256: None,
                }],
                source: json!({ "kind": "test" }),
            },
            conflict_strategy: None,
        })
        .await
        .expect("install inline skill");
}

fn test_chat_repository(root: &Path) -> Arc<FileChatRepository> {
    Arc::new(FileChatRepository::with_chat_aliases(
        root.join("characters"),
        root.join("chats"),
        root.join("group_chats"),
        root.join("backups"),
        new_shared_chat_alias_store_for_user_dir(root),
    ))
}

fn test_dispatcher(repository: Arc<FileAgentRepository>, root: &Path) -> AgentToolDispatcher {
    let chat_repository = test_chat_repository(root);
    AgentToolDispatcher::new(
        repository.clone(),
        chat_repository.clone(),
        chat_repository,
        repository,
        test_skill_service(root),
    )
}

fn test_profile_service(root: &Path) -> Arc<AgentProfileService> {
    let agent_profile_repository =
        Arc::new(FileAgentProfileRepository::new(root.join("agent-profiles")));
    Arc::new(AgentProfileService::new(
        agent_profile_repository.clone(),
        agent_profile_repository,
        Arc::new(NullPresetRepository),
    ))
}

fn test_llm_connection_service(root: &Path) -> Arc<LlmConnectionService> {
    Arc::new(LlmConnectionService::new(Arc::new(
        FileLlmConnectionRepository::new(root.join("llm-connections")),
    )))
}

async fn test_resolved_profile(root: &Path) -> ResolvedAgentProfile {
    let registry = BuiltinAgentToolRegistry::phase2c();
    let mut profile = test_profile_service(root)
        .resolve_profile(AgentProfileResolveInput {
            profile_id: None,
            known_tools: registry.specs(),
        })
        .await
        .expect("resolve default profile");
    profile.run.presentation = AgentRunPresentation::Background;
    profile
}

async fn resolve_next_chat_commit(
    service: Arc<AgentRuntimeService>,
    repository: Arc<FileAgentRepository>,
    run_id: String,
    message_id: &'static str,
) {
    let commit_id = tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            let events = repository
                .read_events(
                    &run_id,
                    AgentRunEventReadQuery {
                        after_seq: Some(0),
                        before_seq: None,
                        limit: 100,
                        invocation_id: None,
                    },
                )
                .await
                .expect("read events");
            if let Some(commit_id) = events
                .iter()
                .find(|event| event.event_type == "chat_commit_requested")
                .and_then(|event| event.payload["commitId"].as_str())
            {
                return commit_id.to_string();
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("chat commit request");

    service
        .resolve_chat_commit(AgentResolveChatCommitDto {
            run_id,
            commit_id,
            message_id: Some(message_id.to_string()),
            error: None,
        })
        .await
        .expect("resolve chat commit");
}

async fn resolve_chat_commits_and_persistent_state_update(
    service: Arc<AgentRuntimeService>,
    repository: Arc<FileAgentRepository>,
    run_id: String,
    message_ids: Vec<&'static str>,
) {
    let mut resolved_commit_ids = Vec::<String>::new();
    for message_id in message_ids {
        let commit_id = tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                let events = repository
                    .read_events(
                        &run_id,
                        AgentRunEventReadQuery {
                            after_seq: Some(0),
                            before_seq: None,
                            limit: 300,
                            invocation_id: None,
                        },
                    )
                    .await
                    .expect("read events");
                if let Some(commit_id) = events
                    .iter()
                    .filter(|event| event.event_type == "chat_commit_requested")
                    .filter_map(|event| event.payload["commitId"].as_str())
                    .find(|commit_id| {
                        !resolved_commit_ids
                            .iter()
                            .any(|resolved| resolved == *commit_id)
                    })
                {
                    return commit_id.to_string();
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("chat commit request");
        service
            .resolve_chat_commit(AgentResolveChatCommitDto {
                run_id: run_id.clone(),
                commit_id: commit_id.clone(),
                message_id: Some(message_id.to_string()),
                error: None,
            })
            .await
            .expect("resolve chat commit");
        resolved_commit_ids.push(commit_id);
    }

    let update_id = tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            let events = repository
                .read_events(
                    &run_id,
                    AgentRunEventReadQuery {
                        after_seq: Some(0),
                        before_seq: None,
                        limit: 300,
                        invocation_id: None,
                    },
                )
                .await
                .expect("read events");
            if let Some(update_id) = events
                .iter()
                .find(|event| event.event_type == "persistent_state_metadata_update_requested")
                .and_then(|event| event.payload["updateId"].as_str())
            {
                return update_id.to_string();
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("persistent state metadata update request");

    service
        .resolve_persistent_state_metadata_update(AgentResolvePersistentStateMetadataUpdateDto {
            run_id,
            update_id,
            error: None,
        })
        .await
        .expect("resolve persistent state metadata update");
}

async fn wait_for_event_payload(
    repository: Arc<FileAgentRepository>,
    run_id: String,
    event_type: &'static str,
) -> Value {
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            let events = repository
                .read_events(
                    &run_id,
                    AgentRunEventReadQuery {
                        after_seq: Some(0),
                        before_seq: None,
                        limit: 200,
                        invocation_id: None,
                    },
                )
                .await
                .expect("read events");
            if let Some(payload) = events
                .iter()
                .find(|event| event.event_type == event_type)
                .map(|event| event.payload.clone())
            {
                return payload;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("event payload")
}

async fn resolve_next_chat_commit_and_persistent_state_update(
    service: Arc<AgentRuntimeService>,
    repository: Arc<FileAgentRepository>,
    run_id: String,
    message_id: &'static str,
) {
    resolve_next_chat_commit(
        service.clone(),
        repository.clone(),
        run_id.clone(),
        message_id,
    )
    .await;

    let update_id = tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            let events = repository
                .read_events(
                    &run_id,
                    AgentRunEventReadQuery {
                        after_seq: Some(0),
                        before_seq: None,
                        limit: 200,
                        invocation_id: None,
                    },
                )
                .await
                .expect("read events");
            if let Some(update_id) = events
                .iter()
                .find(|event| event.event_type == "persistent_state_metadata_update_requested")
                .and_then(|event| event.payload["updateId"].as_str())
            {
                return update_id.to_string();
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("persistent state metadata update request");

    service
        .resolve_persistent_state_metadata_update(AgentResolvePersistentStateMetadataUpdateDto {
            run_id,
            update_id,
            error: None,
        })
        .await
        .expect("resolve persistent state metadata update");
}

async fn save_character_payload(
    repository: &FileChatRepository,
    root: &Path,
    character_name: &str,
    file_name: &str,
    payload: &[Value],
) {
    let source_path = root.join(format!("chat-payload-{}.jsonl", Uuid::new_v4().simple()));
    tokio::fs::write(&source_path, payload_to_jsonl(payload))
        .await
        .expect("write payload");
    repository
        .save_chat_payload_from_path(character_name, file_name, &source_path, false)
        .await
        .expect("save payload");
}

fn payload_to_jsonl(payload: &[Value]) -> String {
    let mut text = String::new();
    for value in payload {
        text.push_str(&serde_json::to_string(value).expect("serialize jsonl value"));
        text.push('\n');
    }
    text
}

impl MockAgentModelGateway {
    fn new(responses: Vec<Value>) -> Self {
        Self::with_results(responses.into_iter().map(Ok).collect())
    }

    fn with_results(responses: Vec<Result<Value, ApplicationError>>) -> Self {
        Self {
            responses: Mutex::new(responses.into()),
            requests: Mutex::new(Vec::new()),
            closed_sessions: Mutex::new(Vec::new()),
        }
    }

    async fn requests(&self) -> Vec<AgentModelRequest> {
        self.requests.lock().await.clone()
    }

    async fn closed_sessions(&self) -> Vec<String> {
        self.closed_sessions.lock().await.clone()
    }
}

impl FinishCancelsDelegateModelGateway {
    fn new() -> Self {
        let (child_started_sender, _) = watch::channel(false);
        let (child_cancelled_sender, _) = watch::channel(false);
        Self {
            root_calls: Mutex::new(0),
            requests: Mutex::new(Vec::new()),
            child_started_sender,
            child_cancelled_sender,
            closed_sessions: Mutex::new(Vec::new()),
        }
    }

    async fn requests(&self) -> Vec<AgentModelRequest> {
        self.requests.lock().await.clone()
    }

    async fn wait_for_child_started(&self) -> Result<(), ApplicationError> {
        let mut child_started = self.child_started_sender.subscribe();
        if *child_started.borrow() {
            return Ok(());
        }
        tokio::time::timeout(Duration::from_secs(1), child_started.changed())
            .await
            .map_err(|_| {
                ApplicationError::ValidationError(
                    "mock_model.child_not_started: delegated task did not start".to_string(),
                )
            })?
            .map_err(|_| {
                ApplicationError::ValidationError(
                    "mock_model.child_started_channel_closed: child start signal closed"
                        .to_string(),
                )
            })?;
        Ok(())
    }

    async fn wait_for_child_cancelled(&self) {
        let mut child_cancelled = self.child_cancelled_sender.subscribe();
        if *child_cancelled.borrow() {
            return;
        }
        tokio::time::timeout(Duration::from_secs(1), child_cancelled.changed())
            .await
            .expect("delegated child model call cancelled")
            .expect("child cancellation signal");
    }
}

impl PendingDelegateHandoffModelGateway {
    fn new() -> Self {
        Self {
            root_calls: Mutex::new(0),
            requests: Mutex::new(Vec::new()),
            closed_sessions: Mutex::new(Vec::new()),
        }
    }

    async fn requests(&self) -> Vec<AgentModelRequest> {
        self.requests.lock().await.clone()
    }
}

struct NullPresetRepository;

struct StaticPresetRepository {
    name: String,
    data: Value,
}

struct FailingPersistentCommitWorkspaceRepository {
    inner: Arc<FileAgentRepository>,
}

#[async_trait]
impl WorkspaceRepository for FailingPersistentCommitWorkspaceRepository {
    async fn initialize_run(
        &self,
        run: &AgentRun,
        manifest: &WorkspaceManifest,
        prompt_snapshot: &Value,
        resolved_profile: &ResolvedAgentProfile,
    ) -> Result<(), DomainError> {
        self.inner
            .initialize_run(run, manifest, prompt_snapshot, resolved_profile)
            .await
    }

    async fn read_manifest(&self, run_id: &str) -> Result<WorkspaceManifest, DomainError> {
        self.inner.read_manifest(run_id).await
    }

    async fn write_text(
        &self,
        run_id: &str,
        path: &WorkspacePath,
        text: &str,
    ) -> Result<WorkspaceFile, DomainError> {
        self.inner.write_text(run_id, path, text).await
    }

    async fn write_text_guarded(
        &self,
        run_id: &str,
        path: &WorkspacePath,
        text: &str,
        guard: WorkspaceWriteGuard,
    ) -> Result<WorkspaceFile, DomainError> {
        self.inner
            .write_text_guarded(run_id, path, text, guard)
            .await
    }

    async fn append_text(
        &self,
        run_id: &str,
        path: &WorkspacePath,
        text: &str,
    ) -> Result<WorkspaceAppendResult, DomainError> {
        self.inner.append_text(run_id, path, text).await
    }

    async fn read_text(
        &self,
        run_id: &str,
        path: &WorkspacePath,
    ) -> Result<WorkspaceFile, DomainError> {
        self.inner.read_text(run_id, path).await
    }

    async fn list_files(
        &self,
        run_id: &str,
        path: Option<&WorkspacePath>,
        depth: usize,
        max_entries: usize,
    ) -> Result<WorkspaceFileList, DomainError> {
        self.inner
            .list_files(run_id, path, depth, max_entries)
            .await
    }

    async fn commit_persistent_changes(
        &self,
        _run_id: &str,
    ) -> Result<WorkspacePersistentChangeSet, DomainError> {
        Err(DomainError::InternalError(
            "agent.test_persistent_failure: simulated persistent commit failure".to_string(),
        ))
    }
}

#[async_trait]
impl PresetRepository for NullPresetRepository {
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
        _name: &str,
        _preset_type: &PresetType,
    ) -> Result<bool, DomainError> {
        Ok(false)
    }

    async fn get_preset(
        &self,
        _name: &str,
        _preset_type: &PresetType,
    ) -> Result<Option<Preset>, DomainError> {
        Ok(None)
    }

    async fn list_presets(&self, _preset_type: &PresetType) -> Result<Vec<String>, DomainError> {
        Ok(Vec::new())
    }

    async fn get_default_preset(
        &self,
        _name: &str,
        _preset_type: &PresetType,
    ) -> Result<Option<DefaultPreset>, DomainError> {
        Ok(None)
    }
}

impl StaticPresetRepository {
    fn openai(name: &str, data: Value) -> Self {
        Self {
            name: name.to_string(),
            data,
        }
    }
}

#[async_trait]
impl PresetRepository for StaticPresetRepository {
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
        Ok(name == self.name && *preset_type == PresetType::OpenAI)
    }

    async fn get_preset(
        &self,
        name: &str,
        preset_type: &PresetType,
    ) -> Result<Option<Preset>, DomainError> {
        if name == self.name && *preset_type == PresetType::OpenAI {
            return Ok(Some(Preset::new(
                self.name.clone(),
                PresetType::OpenAI,
                self.data.clone(),
            )));
        }
        Ok(None)
    }

    async fn list_presets(&self, preset_type: &PresetType) -> Result<Vec<String>, DomainError> {
        if *preset_type == PresetType::OpenAI {
            return Ok(vec![self.name.clone()]);
        }
        Ok(Vec::new())
    }

    async fn get_default_preset(
        &self,
        _name: &str,
        _preset_type: &PresetType,
    ) -> Result<Option<DefaultPreset>, DomainError> {
        Ok(None)
    }
}

async fn wait_for_closed_sessions(gateway: &MockAgentModelGateway, expected: Vec<String>) {
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if gateway.closed_sessions().await == expected {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("agent provider session cleanup");
}

async fn insert_active_run_handle(
    service: &Arc<AgentRuntimeService>,
    run_id: &str,
) -> Arc<super::scheduler::ActiveRunHandle> {
    let (active_cancel_sender, _) = watch::channel(false);
    let active_handle = Arc::new(super::scheduler::ActiveRunHandle::new(
        service,
        run_id.to_string(),
        active_cancel_sender,
    ));
    service
        .active_runs
        .write()
        .await
        .insert(run_id.to_string(), active_handle.clone());
    active_handle
}

#[async_trait]
impl AgentModelGateway for MockAgentModelGateway {
    async fn generate_with_cancel(
        &self,
        request: AgentModelRequest,
        _cancel: watch::Receiver<bool>,
    ) -> Result<AgentModelExchange, ApplicationError> {
        self.requests.lock().await.push(request.clone());
        let response = self.responses.lock().await.pop_front().ok_or_else(|| {
            ApplicationError::ValidationError(
                "mock_model.empty_responses: no response left".to_string(),
            )
        })??;
        let response = decode_chat_completion_response(response, &request.tools)?;
        Ok(AgentModelExchange {
            response,
            provider_state: request.provider_state,
        })
    }

    async fn close_session(&self, session_id: &str) {
        self.closed_sessions
            .lock()
            .await
            .push(session_id.to_string());
    }
}

#[async_trait]
impl AgentModelGateway for PendingDelegateHandoffModelGateway {
    async fn generate_with_cancel(
        &self,
        request: AgentModelRequest,
        mut cancel: watch::Receiver<bool>,
    ) -> Result<AgentModelExchange, ApplicationError> {
        self.requests.lock().await.push(request.clone());
        let invocation_id = request
            .provider_state
            .get("invocationId")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if invocation_id == "inv_root" {
            let mut root_calls = self.root_calls.lock().await;
            let response = if *root_calls == 0 {
                *root_calls += 1;
                pending_handoff_delegate_and_handoff_response()
            } else {
                *root_calls += 1;
                finish_cancel_finish_response()
            };
            let response = decode_chat_completion_response(response, &request.tools)?;
            return Ok(AgentModelExchange {
                response,
                provider_state: request.provider_state,
            });
        }

        loop {
            if *cancel.borrow() {
                return Err(ApplicationError::Cancelled(
                    "mock child cancelled while handoff was pending".to_string(),
                ));
            }
            cancel.changed().await.map_err(|_| {
                ApplicationError::Cancelled(
                    "mock child cancel channel closed while handoff was pending".to_string(),
                )
            })?;
        }
    }

    async fn close_session(&self, session_id: &str) {
        self.closed_sessions
            .lock()
            .await
            .push(session_id.to_string());
    }
}

#[async_trait]
impl AgentModelGateway for FinishCancelsDelegateModelGateway {
    async fn generate_with_cancel(
        &self,
        request: AgentModelRequest,
        mut cancel: watch::Receiver<bool>,
    ) -> Result<AgentModelExchange, ApplicationError> {
        self.requests.lock().await.push(request.clone());
        let invocation_id = request
            .provider_state
            .get("invocationId")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if invocation_id != "inv_root" {
            self.child_started_sender.send_replace(true);
            loop {
                if *cancel.borrow() {
                    self.child_cancelled_sender.send_replace(true);
                    return Err(ApplicationError::Cancelled(
                        "mock delegated child cancelled".to_string(),
                    ));
                }
                if cancel.changed().await.is_err() {
                    self.child_cancelled_sender.send_replace(true);
                    return Err(ApplicationError::Cancelled(
                        "mock delegated child cancel channel closed".to_string(),
                    ));
                }
            }
        }

        let call_index = {
            let mut root_calls = self.root_calls.lock().await;
            *root_calls += 1;
            *root_calls
        };
        let response = match call_index {
            1 => finish_cancel_delegate_response(),
            2 => {
                self.wait_for_child_started().await?;
                finish_cancel_finish_response()
            }
            _ => {
                return Err(ApplicationError::ValidationError(format!(
                    "mock_model.unexpected_root_call: unexpected root model call {call_index}"
                )));
            }
        };
        let response = decode_chat_completion_response(response, &request.tools)?;
        Ok(AgentModelExchange {
            response,
            provider_state: request.provider_state,
        })
    }

    async fn close_session(&self, session_id: &str) {
        self.closed_sessions
            .lock()
            .await
            .push(session_id.to_string());
    }
}

fn finish_cancel_delegate_response() -> Value {
    json!({
        "choices": [{
            "message": {
                "role": "assistant",
                "content": null,
                "tool_calls": [{
                    "id": "call_delegate_without_await",
                    "type": "function",
                    "function": {
                        "name": "agent_delegate",
                        "arguments": serde_json::to_string(&json!({
                            "agentId": "scene-critic",
                            "task": {
                                "title": "Critique scene",
                                "objective": "Find one concrete improvement.",
                                "context": { "draft": "A quiet scene." },
                                "expectedOutput": { "format": "short capsule" }
                            },
                            "budget": { "maxRounds": 4, "maxToolCalls": 4 }
                        })).unwrap()
                    }
                }]
            }
        }]
    })
}

fn pending_handoff_delegate_and_handoff_response() -> Value {
    json!({
        "choices": [{
            "message": {
                "role": "assistant",
                "content": null,
                "tool_calls": [
                    {
                        "id": "call_delegate_before_handoff",
                        "type": "function",
                        "function": {
                            "name": "agent_delegate",
                            "arguments": serde_json::to_string(&json!({
                                "agentId": "scene-critic",
                                "task": {
                                    "title": "Critique scene",
                                    "objective": "Find one concrete improvement.",
                                    "context": { "draft": "A quiet scene." },
                                    "expectedOutput": { "format": "short capsule" }
                                },
                                "budget": { "maxRounds": 4, "maxToolCalls": 4 }
                            })).unwrap()
                        }
                    },
                    {
                        "id": "call_handoff_with_pending_task",
                        "type": "function",
                        "function": {
                            "name": "agent_handoff",
                            "arguments": serde_json::to_string(&json!({
                                "agentId": "final-editor",
                                "handoff": {
                                    "objective": "Take over after the critic finishes.",
                                    "contextSummary": "This should be rejected because a delegated task is still pending.",
                                    "workspaceRefs": ["output/main.md"]
                                }
                            })).unwrap()
                        }
                    }
                ]
            }
        }]
    })
}

fn finish_cancel_finish_response() -> Value {
    json!({
        "choices": [{
            "message": {
                "role": "assistant",
                "content": null,
                "tool_calls": [{
                    "id": "call_finish_without_await",
                    "type": "function",
                    "function": {
                        "name": "workspace_finish",
                        "arguments": "{}"
                    }
                }]
            }
        }]
    })
}
