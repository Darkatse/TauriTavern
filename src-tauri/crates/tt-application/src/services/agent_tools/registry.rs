use super::agent::{
    agent_await_descriptor, agent_delegate_descriptor, agent_handoff_descriptor,
    task_return_descriptor,
};
use super::chat::{chat_read_messages_descriptor, chat_search_descriptor};
use super::dice::dice_roll_descriptor;
use super::policy::stage_can_finish_run;
use super::workspace::{
    WORKSPACE_COMMIT, WORKSPACE_WRITE_FILE, workspace_apply_patch_descriptor,
    workspace_commit_descriptor, workspace_list_files_descriptor, workspace_read_file_descriptor,
    workspace_search_files_descriptor, workspace_shell_descriptor, workspace_write_file_descriptor,
};
use super::world_info::worldinfo_read_activated_descriptor;
use crate::errors::ApplicationError;
use tt_domain::models::agent::profile::ResolvedAgentProfile;
use tt_domain::models::tool::{ToolCatalog, ToolDescriptor, ToolId};

#[derive(Debug, Clone)]
pub struct BuiltinAgentToolRegistry {
    catalog: ToolCatalog,
}

impl BuiltinAgentToolRegistry {
    pub fn all() -> Self {
        let descriptors = vec![
            agent_delegate_descriptor(),
            agent_handoff_descriptor(),
            agent_await_descriptor(),
            task_return_descriptor(),
            chat_search_descriptor(),
            chat_read_messages_descriptor(),
            worldinfo_read_activated_descriptor(),
            dice_roll_descriptor(),
            workspace_list_files_descriptor(),
            workspace_search_files_descriptor(),
            workspace_read_file_descriptor(),
            workspace_write_file_descriptor(),
            workspace_apply_patch_descriptor(),
            workspace_shell_descriptor(),
            workspace_commit_descriptor(),
        ];
        let catalog = ToolCatalog::try_from_descriptors(descriptors)
            .expect("builtin Agent tool descriptors must form a valid catalog");

        Self { catalog }
    }

    pub fn catalog(&self) -> &ToolCatalog {
        &self.catalog
    }

    pub(crate) fn materialize_profile_descriptor(
        &self,
        tool_id: &ToolId,
        profile: &ResolvedAgentProfile,
    ) -> Result<ToolDescriptor, ApplicationError> {
        let mut descriptor = self.catalog.get(tool_id).cloned().ok_or_else(|| {
            ApplicationError::ValidationError(format!(
                "agent.profile_unknown_tool: unknown tool `{}`",
                tool_id.native_name()
            ))
        })?;
        apply_profile_context(&mut descriptor, profile)?;
        if let Some(override_) = profile.tools.tool_descriptions.get(tool_id) {
            descriptor.apply_description_override(override_)?;
        }
        hide_unavailable_properties(&mut descriptor, profile);
        Ok(descriptor)
    }

    pub(crate) fn apply_return_mode_context(
        &self,
        descriptor: &mut ToolDescriptor,
        profile: &ResolvedAgentProfile,
    ) -> Result<(), ApplicationError> {
        apply_return_mode_context(descriptor, profile)
    }
}

fn apply_return_mode_context(
    descriptor: &mut ToolDescriptor,
    _profile: &ResolvedAgentProfile,
) -> Result<(), ApplicationError> {
    // Delegated tasks share the caller's workspace paths; the task brief names the files.
    if descriptor.id.native_name() == WORKSPACE_WRITE_FILE {
        descriptor
            .description
            .as_mut()
            .expect("workspace.write_file has a description")
            .push_str(" Use the path requested in the task brief when one is provided.");
    }
    Ok(())
}

fn apply_profile_context(
    descriptor: &mut ToolDescriptor,
    profile: &ResolvedAgentProfile,
) -> Result<(), ApplicationError> {
    // Readable and writable roots are listed once in the system prompt, and access rules are
    // enforced (and explained) by tool errors, so descriptions only add profile facts here.
    match descriptor.id.native_name() {
        WORKSPACE_WRITE_FILE => {
            if let Some(output) = profile.output.as_ref() {
                descriptor
                    .description
                    .as_mut()
                    .expect("workspace.write_file has a description")
                    .push_str(&format!(
                        " The chat reply goes in {}.",
                        output.message_body_path
                    ));
            }
        }
        WORKSPACE_COMMIT => {
            let final_path = crate::services::agent_profile_service::require_output(profile)?
                .message_body_path
                .as_str();
            let finish_hint = if profile_can_finish_run(profile) {
                " Set finish: true on the final commit to end the run."
            } else {
                ""
            };
            descriptor.description = Some(format!(
                "Publish a workspace file as this run's chat message. Only committed text reaches the chat; plain-text replies are never shown.{finish_hint}"
            ));
            descriptor.set_property_description(
                "file_path",
                &format!("File to publish. Defaults to {final_path}."),
            )?;
        }
        _ => {}
    }

    Ok(())
}

/// Runs after description overrides: overrides are validated against the catalog schema,
/// so one written for a property this profile cannot use is simply not shown.
fn hide_unavailable_properties(descriptor: &mut ToolDescriptor, profile: &ResolvedAgentProfile) {
    if descriptor.id.native_name() == WORKSPACE_COMMIT
        && !profile_can_finish_run(profile)
        && let Some(properties) = descriptor
            .input_schema
            .get_mut("properties")
            .and_then(serde_json::Value::as_object_mut)
    {
        properties.remove("finish");
    }
}

fn profile_can_finish_run(profile: &ResolvedAgentProfile) -> bool {
    stage_can_finish_run(|name| profile_tool_visible(profile, name))
}

fn profile_tool_visible(profile: &ResolvedAgentProfile, name: &str) -> bool {
    let id = ToolId::builtin(name).expect("builtin Agent tool names form valid ToolIds");
    profile.tools.allow.iter().any(|allowed| allowed == &id)
        && !profile.tools.deny.iter().any(|denied| denied == &id)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::super::agent::{AGENT_DELEGATE, AGENT_HANDOFF, TASK_RETURN};
    use super::super::policy::{compile_invocation_tool_snapshot, prepare_tool_bindings};
    use super::super::workspace::{WORKSPACE_COMMIT, WORKSPACE_READ_FILE, WORKSPACE_SEARCH_FILES};
    use super::*;
    use tt_domain::models::agent::plan::{AgentPlanMode, AgentPlanPolicy};
    use tt_domain::models::agent::profile::{
        AGENT_PROFILE_KIND, AGENT_PROFILE_SCHEMA_VERSION, AgentContextPolicy,
        AgentDelegationPolicy, AgentModelBinding, AgentModelBindingMode, AgentPresetBinding,
        AgentPresetBindingMode, AgentProfileId, AgentProfileInstructions, AgentProfileSourceTrace,
        AgentRunPolicy, AgentSkillPolicy, AgentToolPolicy, AgentWorkspacePolicy,
        ResolvedAgentOutputPolicy, ResolvedAgentProfile,
    };
    use tt_domain::models::agent::{
        AgentInvocationExitPolicy, AgentRunPresentation, ArtifactSpec, ArtifactTarget,
    };
    use tt_domain::models::tool::{AgentToolScope, ToolId, ToolSnapshotId};

    #[test]
    fn invocation_policy_preserves_order_and_materializes_return_mode_without_profile_mutation() {
        let registry = BuiltinAgentToolRegistry::all();
        let mut profile = test_profile();
        profile.tools.allow = vec![
            ToolId::builtin(WORKSPACE_READ_FILE).unwrap(),
            ToolId::builtin(WORKSPACE_SEARCH_FILES).unwrap(),
            ToolId::builtin(WORKSPACE_COMMIT).unwrap(),
            ToolId::builtin(AGENT_DELEGATE).unwrap(),
        ];
        profile.tools.deny = vec![ToolId::builtin(WORKSPACE_SEARCH_FILES).unwrap()];
        profile
            .tools
            .max_calls_per_tool
            .insert(ToolId::builtin(WORKSPACE_READ_FILE).unwrap(), 2);

        let root = compile_invocation_tool_snapshot(
            &registry,
            &profile,
            AgentInvocationExitPolicy::RunFinishAllowed,
            ToolSnapshotId::parse("root").unwrap(),
            prepare_tool_bindings(&registry, &profile, AgentToolScope::Chat, &[]).unwrap(),
        )
        .unwrap();
        assert_eq!(
            root.bindings()
                .iter()
                .map(|binding| binding.tool_id().native_name())
                .collect::<Vec<_>>(),
            vec![WORKSPACE_READ_FILE, WORKSPACE_COMMIT, AGENT_DELEGATE]
        );
        assert_eq!(root.bindings()[0].max_calls(), Some(2));

        let child = compile_invocation_tool_snapshot(
            &registry,
            &profile,
            AgentInvocationExitPolicy::TaskReturnRequired,
            ToolSnapshotId::parse("child").unwrap(),
            prepare_tool_bindings(&registry, &profile, AgentToolScope::Chat, &[]).unwrap(),
        )
        .unwrap();
        assert_eq!(
            child
                .bindings()
                .iter()
                .map(|binding| binding.tool_id().native_name())
                .collect::<Vec<_>>(),
            vec![WORKSPACE_READ_FILE, TASK_RETURN]
        );
        assert_eq!(
            profile.tools.allow,
            vec![
                ToolId::builtin(WORKSPACE_READ_FILE).unwrap(),
                ToolId::builtin(WORKSPACE_SEARCH_FILES).unwrap(),
                ToolId::builtin(WORKSPACE_COMMIT).unwrap(),
                ToolId::builtin(AGENT_DELEGATE).unwrap(),
            ]
        );
    }

    #[test]
    fn commit_finish_property_follows_whether_the_stage_can_finish() {
        let registry = BuiltinAgentToolRegistry::all();
        let commit = ToolId::builtin(WORKSPACE_COMMIT).unwrap();
        let handoff = ToolId::builtin(AGENT_HANDOFF).unwrap();
        let mut profile = test_profile();
        profile.tools.allow = vec![commit.clone(), handoff.clone()];
        profile.tools.tool_descriptions.insert(
            commit.clone(),
            tt_domain::models::tool::ToolDescriptionOverride {
                description: None,
                properties: BTreeMap::from([("finish".to_string(), "End it.".to_string())]),
            },
        );
        let finish_property = |profile: &ResolvedAgentProfile| {
            registry
                .materialize_profile_descriptor(&commit, profile)
                .unwrap()
                .input_schema
                .pointer("/properties/finish/description")
                .cloned()
        };

        // A stage that can hand off passes the run on instead of ending it.
        assert_eq!(finish_property(&profile), None);
        profile.tools.allow = vec![commit.clone()];
        assert_eq!(finish_property(&profile), Some("End it.".into()));
    }

    fn test_profile() -> ResolvedAgentProfile {
        ResolvedAgentProfile {
            schema_version: AGENT_PROFILE_SCHEMA_VERSION,
            kind: AGENT_PROFILE_KIND.to_string(),
            id: AgentProfileId::parse("test-profile").expect("profile id"),
            display_name: "Test Profile".to_string(),
            description: None,
            preset: AgentPresetBinding {
                mode: AgentPresetBindingMode::CurrentPromptSnapshot,
                ref_: None,
                required: false,
                reasoning_effort: None,
            },
            model: AgentModelBinding {
                mode: AgentModelBindingMode::CurrentPromptSnapshot,
                connection_ref: None,
                model_id: None,
            },
            run: AgentRunPolicy {
                presentation: AgentRunPresentation::Background,
                stream: false,
                direct_runnable: true,
                model_retry: Default::default(),
            },
            context: AgentContextPolicy::default(),
            delegation: AgentDelegationPolicy::default(),
            instructions: AgentProfileInstructions::default(),
            tools: AgentToolPolicy {
                allow: vec![ToolId::builtin(WORKSPACE_READ_FILE).unwrap()],
                deny: Vec::new(),
                tool_descriptions: BTreeMap::new(),
                max_rounds: 1,
                max_calls_per_run: 1,
                external_result_inline_char_limit: 50_000,
                max_calls_per_tool: BTreeMap::new(),
            },
            skills: AgentSkillPolicy {
                visible: vec!["*".to_string()],
                deny: Vec::new(),
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
            output: Some(ResolvedAgentOutputPolicy {
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
            }),
            source_trace: AgentProfileSourceTrace {
                profile_source: "test".to_string(),
            },
        }
    }
}
