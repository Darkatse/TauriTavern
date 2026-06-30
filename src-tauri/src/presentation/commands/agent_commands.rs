use std::sync::Arc;

use std::collections::BTreeSet;

use serde_json::Value;
use tauri::State;

use crate::app::AppState;
use crate::application::dto::agent_dto::{
    AgentApplyCurrentModelConnectionSnapshotDto, AgentApplyCurrentModelConnectionSnapshotResultDto,
    AgentApplyRunPruneDto, AgentBuildCurrentModelConnectionSnapshotDto,
    AgentBuildCurrentModelConnectionSnapshotResultDto, AgentCancelRunDto,
    AgentListProfilesResultDto, AgentListRunsDto, AgentListRunsResultDto,
    AgentListToolSpecsResultDto, AgentLoadProfileResultDto, AgentModelTurnDisplayDto,
    AgentPlanRunPruneDto, AgentPreparePromptAssemblyDto, AgentPreparePromptAssemblyResultDto,
    AgentProfileIdDto, AgentPromptAssemblyBrokerRequestDto, AgentPruneChatPersistentStatesDto,
    AgentPruneChatPersistentStatesResultDto, AgentReadEventsDto, AgentReadEventsResultDto,
    AgentReadModelTurnDto, AgentReadPromptAssemblyRequestDto, AgentReadWorkspaceFileDto,
    AgentRepairProfileFileDto, AgentResolveChatCommitDto,
    AgentResolvePersistentStateMetadataUpdateDto, AgentResolvePromptAssemblyDto,
    AgentResolveSystemPromptDto, AgentResolveSystemPromptResultDto, AgentRetargetPresetRefsDto,
    AgentRetargetPresetRefsResultDto, AgentRunHandleDto, AgentRunPruneApplyResultDto,
    AgentRunPrunePlanDto, AgentSaveProfileDto, AgentStartRunDto, AgentSubmitGuidanceDto,
    AgentSubmitGuidanceResultDto, AgentWorkspaceFileDto,
};
use crate::application::errors::ApplicationError;
use crate::application::services::agent_workspace_lifecycle_service::AgentChatWorkspaceTarget;
use crate::domain::models::agent::AgentChatRef;
use crate::domain::models::agent::profile_diagnostic::AgentProfileHealth;
use crate::domain::repositories::agent_workspace_lifecycle_repository::AgentPersistentStatePruneRequest;
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::errors::CommandError;

#[tauri::command]
pub async fn start_agent_run(
    dto: AgentStartRunDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<AgentRunHandleDto, CommandError> {
    log_command("start_agent_run");

    app_state
        .services
        .agent_runtime_service
        .start_run(dto)
        .await
        .map_err(map_command_error("Failed to start agent run"))
}

#[tauri::command]
pub async fn prepare_agent_prompt_assembly(
    dto: AgentPreparePromptAssemblyDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<AgentPreparePromptAssemblyResultDto, CommandError> {
    log_command("prepare_agent_prompt_assembly");

    let profile = app_state
        .services
        .prompt_assembly_service
        .resolve_profile(
            dto.profile_id.as_deref(),
            app_state.services.agent_runtime_service.tool_specs(),
        )
        .await
        .map_err(map_command_error(
            "Failed to resolve agent profile for prompt assembly",
        ))?;
    let visible_tools = app_state
        .services
        .agent_runtime_service
        .visible_tool_specs(&profile)
        .map_err(map_command_error(
            "Failed to resolve agent prompt assembly tool surface",
        ))?;

    app_state
        .services
        .prompt_assembly_service
        .prepare_frontend_prompt_assembly(dto, profile, &visible_tools)
        .await
        .map_err(map_command_error("Failed to prepare agent prompt assembly"))
}

#[tauri::command]
pub async fn build_agent_current_model_connection_snapshot(
    dto: AgentBuildCurrentModelConnectionSnapshotDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<AgentBuildCurrentModelConnectionSnapshotResultDto, CommandError> {
    log_command("build_agent_current_model_connection_snapshot");

    app_state
        .services
        .prompt_assembly_service
        .build_current_model_connection_snapshot(
            &dto.settings,
            &dto.model,
            dto.secret_id.as_deref(),
        )
        .map(
            |current_model_connection| AgentBuildCurrentModelConnectionSnapshotResultDto {
                current_model_connection,
            },
        )
        .map_err(map_command_error(
            "Failed to build current model connection snapshot",
        ))
}

#[tauri::command]
pub async fn apply_agent_current_model_connection_snapshot(
    dto: AgentApplyCurrentModelConnectionSnapshotDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<AgentApplyCurrentModelConnectionSnapshotResultDto, CommandError> {
    log_command("apply_agent_current_model_connection_snapshot");

    app_state
        .services
        .prompt_assembly_service
        .apply_current_model_connection_snapshot(dto.settings, &dto.current_model_connection)
        .map(|settings| AgentApplyCurrentModelConnectionSnapshotResultDto { settings })
        .map_err(map_command_error(
            "Failed to apply current model connection snapshot",
        ))
}

#[tauri::command]
pub async fn list_agent_profiles(
    app_state: State<'_, Arc<AppState>>,
) -> Result<AgentListProfilesResultDto, CommandError> {
    log_command("list_agent_profiles");

    app_state
        .services
        .agent_profile_service
        .list_profiles()
        .await
        .map(|list| AgentListProfilesResultDto {
            profiles: list.profiles,
            issues: list.issues,
        })
        .map_err(map_command_error("Failed to list agent profiles"))
}

#[tauri::command]
pub async fn list_agent_tool_specs(
    app_state: State<'_, Arc<AppState>>,
) -> Result<AgentListToolSpecsResultDto, CommandError> {
    log_command("list_agent_tool_specs");

    Ok(AgentListToolSpecsResultDto {
        tools: app_state
            .services
            .agent_runtime_service
            .tool_specs()
            .to_vec(),
    })
}

#[tauri::command]
pub async fn resolve_agent_system_prompt(
    dto: AgentResolveSystemPromptDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<AgentResolveSystemPromptResultDto, CommandError> {
    log_command("resolve_agent_system_prompt");

    app_state
        .services
        .agent_runtime_service
        .resolve_agent_system_prompt(dto.profile_id.as_deref())
        .await
        .map(|agent_system_prompt| AgentResolveSystemPromptResultDto {
            agent_system_prompt,
        })
        .map_err(map_command_error("Failed to resolve agent system prompt"))
}

#[tauri::command]
pub async fn load_agent_profile(
    dto: AgentProfileIdDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<AgentLoadProfileResultDto, CommandError> {
    log_command("load_agent_profile");

    app_state
        .services
        .agent_profile_service
        .load_profile(&dto.profile_id)
        .await
        .map(|profile| AgentLoadProfileResultDto { profile })
        .map_err(map_command_error("Failed to load agent profile"))
}

#[tauri::command]
pub async fn diagnose_agent_profile(
    dto: AgentProfileIdDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<AgentProfileHealth, CommandError> {
    log_command("diagnose_agent_profile");

    app_state
        .services
        .agent_profile_diagnostic_service
        .diagnose_profile(
            &dto.profile_id,
            app_state.services.agent_runtime_service.tool_specs(),
        )
        .await
        .map_err(map_command_error("Failed to diagnose agent profile"))
}

#[tauri::command]
pub async fn save_agent_profile(
    dto: AgentSaveProfileDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command("save_agent_profile");

    let known_tools = app_state
        .services
        .agent_runtime_service
        .tool_specs()
        .to_vec();
    app_state
        .services
        .agent_profile_service
        .save_profile(dto.profile, &known_tools)
        .await
        .map_err(map_command_error("Failed to save agent profile"))
}

#[tauri::command]
pub async fn delete_agent_profile(
    dto: AgentProfileIdDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command("delete_agent_profile");

    app_state
        .services
        .agent_profile_service
        .delete_profile(&dto.profile_id)
        .await
        .map_err(map_command_error("Failed to delete agent profile"))
}

#[tauri::command]
pub async fn repair_agent_profile_file(
    dto: AgentRepairProfileFileDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command("repair_agent_profile_file");

    app_state
        .services
        .agent_profile_service
        .repair_profile_file(&dto.profile_id, dto.action)
        .await
        .map_err(map_command_error("Failed to repair agent profile file"))
}

#[tauri::command]
pub async fn retarget_agent_profile_preset_refs(
    dto: AgentRetargetPresetRefsDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<AgentRetargetPresetRefsResultDto, CommandError> {
    log_command(format!(
        "retarget_agent_profile_preset_refs {}/{} -> {}/{}",
        dto.from.api_id, dto.from.name, dto.to.api_id, dto.to.name
    ));

    app_state
        .services
        .agent_profile_service
        .retarget_preset_refs(dto.from, dto.to)
        .await
        .map(|result| AgentRetargetPresetRefsResultDto {
            updated: result.profile_ids.len(),
            profile_ids: result
                .profile_ids
                .iter()
                .map(|id| id.as_str().to_string())
                .collect(),
        })
        .map_err(map_command_error(
            "Failed to retarget agent profile preset refs",
        ))
}

#[tauri::command]
pub async fn cancel_agent_run(
    dto: AgentCancelRunDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<AgentRunHandleDto, CommandError> {
    log_command("cancel_agent_run");

    app_state
        .services
        .agent_runtime_service
        .cancel_run(dto)
        .await
        .map_err(map_command_error("Failed to cancel agent run"))
}

#[tauri::command]
pub async fn submit_agent_run_guidance(
    dto: AgentSubmitGuidanceDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<AgentSubmitGuidanceResultDto, CommandError> {
    log_command("submit_agent_run_guidance");

    app_state
        .services
        .agent_runtime_service
        .submit_guidance(dto)
        .await
        .map_err(map_command_error("Failed to submit agent run guidance"))
}

#[tauri::command]
pub async fn list_agent_runs(
    dto: AgentListRunsDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<AgentListRunsResultDto, CommandError> {
    log_command("list_agent_runs");

    app_state
        .services
        .agent_run_history_service
        .list_runs(dto)
        .await
        .map_err(map_command_error("Failed to list agent runs"))
}

#[tauri::command]
pub async fn plan_agent_run_prune(
    dto: AgentPlanRunPruneDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<AgentRunPrunePlanDto, CommandError> {
    log_command("plan_agent_run_prune");

    app_state
        .services
        .agent_run_history_service
        .plan_run_prune(dto)
        .await
        .map_err(map_command_error("Failed to plan agent run prune"))
}

#[tauri::command]
pub async fn apply_agent_run_prune(
    dto: AgentApplyRunPruneDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<AgentRunPruneApplyResultDto, CommandError> {
    log_command("apply_agent_run_prune");

    app_state
        .services
        .agent_run_history_service
        .apply_run_prune(dto)
        .await
        .map_err(map_command_error("Failed to apply agent run prune"))
}

#[tauri::command]
pub async fn read_agent_run_events(
    dto: AgentReadEventsDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<AgentReadEventsResultDto, CommandError> {
    log_command("read_agent_run_events");

    app_state
        .services
        .agent_runtime_service
        .read_events(dto)
        .await
        .map_err(map_command_error("Failed to read agent run events"))
}

#[tauri::command]
pub async fn read_agent_workspace_file(
    dto: AgentReadWorkspaceFileDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<AgentWorkspaceFileDto, CommandError> {
    log_command("read_agent_workspace_file");

    app_state
        .services
        .agent_runtime_service
        .read_workspace_file(dto)
        .await
        .map_err(map_command_error("Failed to read agent workspace file"))
}

#[tauri::command]
pub async fn read_agent_model_turn(
    dto: AgentReadModelTurnDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<AgentModelTurnDisplayDto, CommandError> {
    log_command("read_agent_model_turn");

    app_state
        .services
        .agent_runtime_service
        .read_model_turn(dto)
        .await
        .map_err(map_command_error("Failed to read agent model turn"))
}

#[tauri::command]
pub async fn read_agent_prompt_assembly_request(
    dto: AgentReadPromptAssemblyRequestDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<AgentPromptAssemblyBrokerRequestDto, CommandError> {
    log_command("read_agent_prompt_assembly_request");

    app_state
        .services
        .agent_runtime_service
        .read_prompt_assembly_request(dto)
        .await
        .map_err(map_command_error(
            "Failed to read agent prompt assembly request",
        ))
}

#[tauri::command]
pub async fn resolve_agent_chat_commit(
    dto: AgentResolveChatCommitDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command("resolve_agent_chat_commit");

    app_state
        .services
        .agent_runtime_service
        .resolve_chat_commit(dto)
        .await
        .map_err(map_command_error("Failed to resolve agent chat commit"))
}

#[tauri::command]
pub async fn resolve_agent_prompt_assembly(
    dto: AgentResolvePromptAssemblyDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command("resolve_agent_prompt_assembly");

    app_state
        .services
        .agent_runtime_service
        .resolve_prompt_assembly(dto)
        .await
        .map_err(map_command_error("Failed to resolve agent prompt assembly"))
}

#[tauri::command]
pub async fn resolve_agent_persistent_state_metadata_update(
    dto: AgentResolvePersistentStateMetadataUpdateDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command("resolve_agent_persistent_state_metadata_update");

    app_state
        .services
        .agent_runtime_service
        .resolve_persistent_state_metadata_update(dto)
        .await
        .map_err(map_command_error(
            "Failed to resolve agent persistent state metadata update",
        ))
}

#[tauri::command]
pub async fn prune_agent_chat_persistent_states(
    dto: AgentPruneChatPersistentStatesDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<AgentPruneChatPersistentStatesResultDto, CommandError> {
    log_command("prune_agent_chat_persistent_states");

    let (character_id, file_name) = match &dto.chat_ref {
        AgentChatRef::Character {
            character_id,
            file_name,
        } => (character_id.as_str(), file_name.as_str()),
        AgentChatRef::Group { .. } => {
            return Err(
                map_command_error("Failed to prune agent persistent states")(
                    ApplicationError::ValidationError(
                        "agent.group_persistent_state_prune_unsupported".to_string(),
                    ),
                ),
            );
        }
    };

    let candidate_state_ids = dto.candidate_state_ids.ok_or_else(|| {
        map_command_error("Failed to prune agent persistent states")(
            ApplicationError::ValidationError(
                "agent.persistent_state_prune_candidates_required".to_string(),
            ),
        )
    })?;
    let payload = app_state
        .services
        .chat_service
        .get_chat_payload(character_id, file_name)
        .await
        .map_err(map_command_error(
            "Failed to read chat payload for agent prune",
        ))?;
    let retained_state_ids = collect_agent_persistent_state_ids(&payload);
    let target = AgentChatWorkspaceTarget {
        chat_ref: dto.chat_ref,
        stable_chat_id: dto.stable_chat_id,
    };

    app_state
        .services
        .chat_service
        .prune_agent_persistent_states(
            &target,
            AgentPersistentStatePruneRequest {
                retained_state_ids,
                candidate_state_ids,
            },
        )
        .await
        .map(|prune| AgentPruneChatPersistentStatesResultDto {
            workspace_id: prune.workspace_id,
            removed_state_ids: prune.removed_state_ids,
        })
        .map_err(map_command_error("Failed to prune agent persistent states"))
}

fn collect_agent_persistent_state_ids(payload: &[Value]) -> Vec<String> {
    let mut retained = BTreeSet::new();
    for item in payload {
        collect_agent_persistent_state_id_from_extra(item.get("extra"), &mut retained);
        if let Some(swipe_info) = item.get("swipe_info").and_then(Value::as_array) {
            for swipe in swipe_info {
                collect_agent_persistent_state_id_from_extra(swipe.get("extra"), &mut retained);
            }
        }
    }
    retained.into_iter().collect()
}

fn collect_agent_persistent_state_id_from_extra(
    extra: Option<&Value>,
    retained: &mut BTreeSet<String>,
) {
    let Some(state_id) = extra
        .and_then(|value| value.get("tauritavern"))
        .and_then(|value| value.get("agent"))
        .and_then(|value| value.get("persistStateId"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return;
    };
    retained.insert(state_id.to_string());
}
