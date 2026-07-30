use serde_json::json;

use super::commit_ledger::RunCommitLedger;
use super::skill_scope::{resolve_run_skill_scope_refs, skill_scope_order_for_profile};
use crate::dto::agent_dto::{AgentSkillScopeRefsDto, AgentStartRunDto, AgentStartRunOptionsDto};
use crate::services::agent_identity::workspace_id_for_stable_chat_id;
use tt_domain::models::agent::plan::{AgentPlanMode, AgentPlanPolicy};
use tt_domain::models::agent::profile::{
    AGENT_PROFILE_KIND, AGENT_PROFILE_SCHEMA_VERSION, AgentContextPolicy, AgentDelegationPolicy,
    AgentModelBinding, AgentModelBindingMode, AgentPresetBinding, AgentPresetBindingMode,
    AgentPresetRef, AgentProfileId, AgentProfileInstructions, AgentProfileSourceTrace,
    AgentRunPolicy, AgentSkillPolicy, AgentToolPolicy, AgentWorkspacePolicy,
    ResolvedAgentOutputPolicy, ResolvedAgentProfile,
};
use tt_domain::models::agent::{
    AgentChatCommitMode, AgentChatRef, AgentRunPresentation, AgentRunSkillScopeRefs, ArtifactSpec,
    ArtifactTarget, WorkspacePath,
};
use tt_domain::models::skill::SkillScope;

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

#[test]
fn skill_scope_order_uses_profile_preset_then_profile_then_character() {
    let preset = AgentPresetRef {
        api_id: "openai".to_string(),
        name: "story".to_string(),
    };
    let profile = resolved_profile(AgentPresetBinding {
        mode: AgentPresetBindingMode::Ref,
        ref_: Some(preset.clone()),
        required: false,
    });
    let refs = AgentRunSkillScopeRefs {
        preset: None,
        character_id: Some("Seraphina".to_string()),
    };

    let scopes = skill_scope_order_for_profile(&profile, &refs).unwrap();

    assert_eq!(
        scopes,
        vec![
            SkillScope::Global,
            SkillScope::Preset {
                api_id: preset.api_id,
                name: preset.name,
            },
            SkillScope::Profile {
                profile_id: "writer".to_string(),
            },
            SkillScope::Character {
                character_id: "Seraphina".to_string(),
            },
        ]
    );
}

#[test]
fn resolve_run_skill_scope_refs_rejects_mismatched_character() {
    let profile = resolved_profile(AgentPresetBinding {
        mode: AgentPresetBindingMode::CurrentPromptSnapshot,
        ref_: None,
        required: false,
    });
    let dto = AgentStartRunDto {
        chat_ref: AgentChatRef::Character {
            character_id: "Seraphina".to_string(),
            file_name: "Seraphina.png".to_string(),
        },
        stable_chat_id: "stable-chat".to_string(),
        generation_type: "normal".to_string(),
        profile_id: None,
        persist_base_state_id: None,
        prompt_snapshot: None,
        frozen_run_input_snapshot: None,
        generation_intent: None,
        skill_scope_refs: AgentSkillScopeRefsDto {
            preset: None,
            character_id: Some("Other".to_string()),
        },
        options: AgentStartRunOptionsDto::default(),
    };

    let error = resolve_run_skill_scope_refs(&dto, &profile).unwrap_err();

    assert!(
        error
            .to_string()
            .contains("agent.skill_scope_character_mismatch")
    );
}

#[test]
fn run_commit_ledger_preserves_commit_payloads() {
    let mut ledger = RunCommitLedger::default();
    let path = WorkspacePath::parse("output/main.md").unwrap();

    ledger.record(
        &path,
        AgentChatCommitMode::Replace,
        Some("msg_1".to_string()),
        1,
    );

    assert_eq!(ledger.len(), 1);
    assert_eq!(ledger.latest_message_id(), Some("msg_1"));
    assert_eq!(
        ledger.preserved_commits(),
        vec![json!({
            "path": "output/main.md",
            "mode": "replace",
            "messageId": "msg_1",
            "round": 1,
        })]
    );
}

fn resolved_profile(preset: AgentPresetBinding) -> ResolvedAgentProfile {
    ResolvedAgentProfile {
        schema_version: AGENT_PROFILE_SCHEMA_VERSION,
        kind: AGENT_PROFILE_KIND.to_string(),
        id: AgentProfileId::parse("writer").unwrap(),
        display_name: "Writer".to_string(),
        description: None,
        preset,
        model: AgentModelBinding {
            mode: AgentModelBindingMode::CurrentPromptSnapshot,
            connection_ref: None,
            model_id: None,
        },
        run: AgentRunPolicy {
            presentation: AgentRunPresentation::Background,
            direct_runnable: true,
            model_retry: Default::default(),
        },
        context: AgentContextPolicy::default(),
        delegation: AgentDelegationPolicy::default(),
        instructions: AgentProfileInstructions::default(),
        tools: AgentToolPolicy {
            allow: Vec::new(),
            deny: Vec::new(),
            tool_descriptions: Default::default(),
            max_rounds: 1,
            max_calls_per_run: 1,
            max_calls_per_tool: Default::default(),
        },
        skills: AgentSkillPolicy {
            visible: vec!["*".to_string()],
            deny: Vec::new(),
            max_read_chars_per_call: 1000,
            max_read_chars_per_run: 1000,
        },
        workspace: AgentWorkspacePolicy {
            visible_roots: vec!["output".to_string()],
            writable_roots: vec!["output".to_string()],
        },
        plan: AgentPlanPolicy {
            mode: AgentPlanMode::None,
            beta: true,
            nodes: Vec::new(),
        },
        output: ResolvedAgentOutputPolicy {
            artifacts: vec![ArtifactSpec {
                id: "main".to_string(),
                path: "output/main.md".to_string(),
                kind: "markdown".to_string(),
                target: ArtifactTarget::MessageBody,
                required: true,
                assembly_order: 0,
            }],
            message_body_artifact_id: "main".to_string(),
            message_body_path: "output/main.md".to_string(),
        },
        source_trace: AgentProfileSourceTrace {
            profile_source: "test".to_string(),
        },
    }
}
