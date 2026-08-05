use super::agent::{
    agent_await_descriptor, agent_delegate_descriptor, agent_handoff_descriptor,
    agent_list_descriptor, task_return_descriptor,
};
use super::chat::{chat_read_messages_descriptor, chat_search_descriptor};
use super::dice::dice_roll_descriptor;
use super::skill::{
    SKILL_READ, skill_list_descriptor, skill_read_descriptor, skill_search_descriptor,
};
use super::workspace::{
    WORKSPACE_APPLY_PATCH, WORKSPACE_COMMIT, WORKSPACE_FINISH, WORKSPACE_LIST_FILES,
    WORKSPACE_READ_FILE, WORKSPACE_SEARCH_FILES, WORKSPACE_WRITE_FILE,
    workspace_apply_patch_descriptor, workspace_commit_descriptor, workspace_finish_descriptor,
    workspace_list_files_descriptor, workspace_read_file_descriptor,
    workspace_search_files_descriptor, workspace_write_file_descriptor,
};
use super::world_info::worldinfo_read_activated_descriptor;
use crate::errors::ApplicationError;
use crate::services::agent_workspace_scope::format_model_workspace_roots;
use tt_domain::models::agent::profile::{AgentToolDescriptionOverride, ResolvedAgentProfile};
use tt_domain::models::tool::{ToolCatalog, ToolDescriptor, ToolId};

#[derive(Debug, Clone)]
pub struct BuiltinAgentToolRegistry {
    catalog: ToolCatalog,
}

impl BuiltinAgentToolRegistry {
    pub fn all() -> Self {
        let descriptors = vec![
            agent_list_descriptor(),
            agent_delegate_descriptor(),
            agent_handoff_descriptor(),
            agent_await_descriptor(),
            task_return_descriptor(),
            chat_search_descriptor(),
            chat_read_messages_descriptor(),
            worldinfo_read_activated_descriptor(),
            dice_roll_descriptor(),
            skill_list_descriptor(),
            skill_search_descriptor(),
            skill_read_descriptor(),
            workspace_list_files_descriptor(),
            workspace_search_files_descriptor(),
            workspace_read_file_descriptor(),
            workspace_write_file_descriptor(),
            workspace_apply_patch_descriptor(),
            workspace_commit_descriptor(),
            workspace_finish_descriptor(),
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
        if let Some(override_) = profile.tools.tool_descriptions.get(tool_id.native_name()) {
            apply_description_override(&mut descriptor, override_)?;
        }
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
    profile: &ResolvedAgentProfile,
) -> Result<(), ApplicationError> {
    let visible_roots = format_model_workspace_roots(&profile.workspace.visible_roots);
    let writable_roots = format_model_workspace_roots(&profile.workspace.writable_roots);
    match descriptor.id.native_name() {
        WORKSPACE_LIST_FILES => {
            descriptor.description = Some(format!(
                "List files visible to this delegated task under {visible_roots}. This is the same logical workspace used by the requesting Agent; use the paths named in the task brief."
            ));
            set_property_description(
                descriptor,
                "path",
                &format!(
                    "Optional task workspace path under {visible_roots}. Omit to list visible roots."
                ),
            )?;
        }
        WORKSPACE_READ_FILE => {
            descriptor.description = Some(format!(
                "Read a visible UTF-8 task workspace file with line numbers. Visible roots are {visible_roots}. Use ordinary workspace paths exactly as they appear in the task brief or file list."
            ));
            set_property_description(
                descriptor,
                "path",
                &format!("Visible task workspace file path under {visible_roots}."),
            )?;
        }
        WORKSPACE_SEARCH_FILES => {
            descriptor.description = Some(format!(
                "Search visible UTF-8 task workspace files under {visible_roots}. Use this before reading exact ranges."
            ));
            set_property_description(
                descriptor,
                "path",
                "Optional visible task workspace file or directory path. Omit to search all visible task paths.",
            )?;
        }
        WORKSPACE_WRITE_FILE => {
            descriptor.description = Some(format!(
                "Write UTF-8 text to a writable workspace file for this delegated task. mode replace writes the complete file; mode append adds content exactly to the end and creates the file when missing. Writable prefixes are {writable_roots}. Use the path requested in the task brief when one is provided."
            ));
            set_property_description(
                descriptor,
                "path",
                &format!(
                    "Writable task path under {writable_roots}. Use the path requested in the task when one is provided."
                ),
            )?;
        }
        WORKSPACE_APPLY_PATCH => {
            descriptor.description = Some(format!(
                "Apply a precise single-file string replacement to a writable delegated-task workspace file. Writable prefixes are {writable_roots}. Fully read an existing file before editing it; if the tool reports that it changed, read it again and retry."
            ));
            set_property_description(
                descriptor,
                "path",
                &format!(
                    "Writable task path under {writable_roots}. Use the path requested in the task when one is provided."
                ),
            )?;
        }
        WORKSPACE_COMMIT | WORKSPACE_FINISH => {}
        _ => {}
    }
    Ok(())
}

fn apply_profile_context(
    descriptor: &mut ToolDescriptor,
    profile: &ResolvedAgentProfile,
) -> Result<(), ApplicationError> {
    let visible_roots = format_model_workspace_roots(&profile.workspace.visible_roots);
    let writable_roots = format_model_workspace_roots(&profile.workspace.writable_roots);
    let final_path = profile.output.message_body_path.as_str();

    match descriptor.id.native_name() {
        WORKSPACE_LIST_FILES => {
            descriptor.description = Some(format!(
                "List visible Agent workspace files under {visible_roots}. Use this before reading when you need to inspect available artifacts."
            ));
            set_property_description(
                descriptor,
                "path",
                &format!(
                    "Optional relative workspace directory or file path under {visible_roots}. Omit to list the visible workspace roots."
                ),
            )?;
        }
        WORKSPACE_READ_FILE => {
            let patch_hint = if profile_tool_visible(profile, WORKSPACE_APPLY_PATCH) {
                " Read the exact text you want to replace before using workspace_apply_patch; if a patch fails, fully read the file before retrying."
            } else {
                " Partial reads are only for inspection."
            };
            descriptor.description = Some(format!(
                "Read a visible UTF-8 Agent workspace file with line numbers.{patch_hint}"
            ));
            set_property_description(
                descriptor,
                "path",
                &format!("Relative workspace file path under {visible_roots}."),
            )?;
        }
        WORKSPACE_SEARCH_FILES => {
            descriptor.description = Some(format!(
                "Search visible UTF-8 Agent workspace files under {visible_roots}. Results return snippets and refs; use workspace_read_file to read exact ranges."
            ));
            set_property_description(
                descriptor,
                "path",
                &format!(
                    "Optional visible workspace file or directory path under {visible_roots}. Omit to search all visible roots."
                ),
            )?;
        }
        WORKSPACE_WRITE_FILE => {
            descriptor.description = Some(format!(
                "Write UTF-8 text to a writable Agent workspace file. mode replace writes the complete file; mode append adds content exactly to the end and creates the file when missing. Use {final_path} for the default chat message body."
            ));
            set_property_description(
                descriptor,
                "path",
                &format!("Relative workspace path. Writable prefixes are {writable_roots}."),
            )?;
        }
        WORKSPACE_APPLY_PATCH => {
            descriptor.description = Some("Apply a precise single-file string replacement. old_string must come from text you already read with workspace_read_file or from a file you created/replaced in this run. old_string must match exactly and uniquely unless replace_all is true. If a patch fails, fully read the file before retrying.".to_string());
            set_property_description(
                descriptor,
                "path",
                &format!("Relative writable workspace file path under {writable_roots}."),
            )?;
        }
        WORKSPACE_COMMIT => {
            descriptor.description = Some(format!(
                "Commit a workspace text file to this run's single chat message. With no arguments, replace the current run message with {final_path}. mode append appends the file text to the same message, creating it when this run has not committed yet."
            ));
            set_property_description(
                descriptor,
                "path",
                &format!(
                    "Relative visible workspace file path to publish. Defaults to {final_path}."
                ),
            )?;
        }
        WORKSPACE_FINISH => {
            descriptor.description = Some(
                "Finish the Agent run after required chat commits and workspace changes are complete."
                    .to_string(),
            );
        }
        SKILL_READ => {
            let per_call = profile.skills.max_read_chars_per_call;
            let per_run = profile.skills.max_read_chars_per_run;
            set_property_description(
                descriptor,
                "max_chars",
                &format!(
                    "Maximum characters to return in this skill_read call. Current policy allows up to {per_call} characters per call and {per_run} total Skill characters per run; the remaining run budget also applies. Omit to use the available per-call budget."
                ),
            )?;
            set_integer_property_bounds(descriptor, "max_chars", 1, per_call)?;
        }
        _ => {}
    }

    Ok(())
}

fn profile_tool_visible(profile: &ResolvedAgentProfile, name: &str) -> bool {
    profile.tools.allow.iter().any(|allowed| allowed == name)
        && !profile.tools.deny.iter().any(|denied| denied == name)
}

fn apply_description_override(
    descriptor: &mut ToolDescriptor,
    override_: &AgentToolDescriptionOverride,
) -> Result<(), ApplicationError> {
    if let Some(description) = override_.description.as_ref() {
        descriptor.description = Some(description.trim().to_string());
    }

    if override_.properties.is_empty() {
        return Ok(());
    }

    for (property, description) in &override_.properties {
        set_property_description(descriptor, property, description.trim())?;
    }
    Ok(())
}

fn set_integer_property_bounds(
    descriptor: &mut ToolDescriptor,
    property: &str,
    minimum: usize,
    maximum: usize,
) -> Result<(), ApplicationError> {
    let object = property_schema_object_mut(descriptor, property)?;
    object.insert("minimum".to_string(), serde_json::json!(minimum));
    object.insert("maximum".to_string(), serde_json::json!(maximum));
    Ok(())
}

fn set_property_description(
    descriptor: &mut ToolDescriptor,
    property: &str,
    description: &str,
) -> Result<(), ApplicationError> {
    let object = property_schema_object_mut(descriptor, property)?;
    object.insert(
        "description".to_string(),
        serde_json::Value::String(description.to_string()),
    );
    Ok(())
}

fn property_schema_object_mut<'a>(
    descriptor: &'a mut ToolDescriptor,
    property: &str,
) -> Result<&'a mut serde_json::Map<String, serde_json::Value>, ApplicationError> {
    let properties = descriptor
        .input_schema
        .get_mut("properties")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| {
            ApplicationError::ValidationError(format!(
                "agent.profile_tool_properties_invalid: `{}` has no object properties",
                descriptor.id.native_name()
            ))
        })?;
    let schema = properties.get_mut(property).ok_or_else(|| {
        ApplicationError::ValidationError(format!(
            "agent.profile_unknown_tool_property: `{}` has no property `{property}`",
            descriptor.id.native_name()
        ))
    })?;
    let object = schema.as_object_mut().ok_or_else(|| {
        ApplicationError::ValidationError(format!(
            "agent.profile_tool_property_schema_invalid: `{}` property `{property}` is not an object",
            descriptor.id.native_name()
        ))
    })?;
    Ok(object)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::super::agent::{
        AGENT_AWAIT, AGENT_DELEGATE, AGENT_HANDOFF, AGENT_LIST, TASK_RETURN,
    };
    use super::super::policy::{compile_invocation_tool_snapshot, project_agent_model_tools};
    use super::super::workspace::{WORKSPACE_FINISH, WORKSPACE_READ_FILE};
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
    use tt_domain::models::tool::{ToolChoice, ToolId, ToolSnapshotId, ToolTurnContract};

    #[test]
    fn snapshot_compiler_derives_builtin_model_aliases() {
        let registry = BuiltinAgentToolRegistry::all();
        let mut profile = profile_with_skill_budget(100_000, 100_000);
        profile.tools.allow = registry
            .catalog()
            .iter()
            .map(|descriptor| descriptor.id.native_name().to_string())
            .filter(|name| name != TASK_RETURN)
            .collect();
        let snapshot = compile_invocation_tool_snapshot(
            &registry,
            &profile,
            AgentInvocationExitPolicy::RunFinishAllowed,
            ToolSnapshotId::parse("aliases").unwrap(),
        )
        .unwrap();

        for binding in snapshot.bindings() {
            assert_eq!(
                binding.model_alias(),
                binding.tool_id().native_name().replace('.', "_")
            );
        }
    }

    #[test]
    fn registry_declares_canonical_builtin_descriptors() {
        let registry = BuiltinAgentToolRegistry::all();

        assert_eq!(registry.catalog().len(), 19);
        for descriptor in registry.catalog().iter() {
            assert!(descriptor.id.is_builtin());
            assert!(
                descriptor
                    .title
                    .as_deref()
                    .is_some_and(|value| !value.is_empty())
            );
            assert!(
                descriptor
                    .description
                    .as_deref()
                    .is_some_and(|value| !value.is_empty())
            );
        }
    }

    #[test]
    fn agent_delegate_requires_objective_but_not_title() {
        let registry = BuiltinAgentToolRegistry::all();
        let delegate = registry
            .catalog()
            .get(&ToolId::builtin(AGENT_DELEGATE).unwrap())
            .expect("agent.delegate descriptor");

        assert_eq!(
            delegate
                .input_schema
                .pointer("/properties/task/required")
                .expect("task required fields"),
            &serde_json::json!(["objective"])
        );
        assert!(
            delegate
                .input_schema
                .pointer("/properties/task/properties/title")
                .is_some()
        );
        assert!(
            delegate
                .input_schema
                .pointer("/properties/budget")
                .is_none()
        );
    }

    #[test]
    fn visible_model_tools_expose_profile_skill_read_budget() {
        let registry = BuiltinAgentToolRegistry::all();
        let profile = profile_with_skill_budget(100_000, 100_000);
        let snapshot = compile_invocation_tool_snapshot(
            &registry,
            &profile,
            AgentInvocationExitPolicy::RunFinishAllowed,
            ToolSnapshotId::parse("test").unwrap(),
        )
        .expect("snapshot");
        let turn = ToolTurnContract::all(&snapshot, ToolChoice::Auto).expect("turn");
        let tools = project_agent_model_tools(&snapshot, &turn).expect("visible model tools");
        let skill_read = tools
            .iter()
            .find(|tool| tool.tool_id.native_name() == SKILL_READ)
            .expect("skill.read model tool");
        let max_chars = skill_read
            .input_schema
            .pointer("/properties/max_chars")
            .expect("max_chars schema");

        assert_eq!(max_chars["maximum"], serde_json::json!(100_000));
        assert_eq!(max_chars["minimum"], serde_json::json!(1));
        assert!(
            max_chars["description"]
                .as_str()
                .expect("description")
                .contains("100000")
        );
        assert!(
            !max_chars["description"]
                .as_str()
                .expect("description")
                .contains("80000")
        );
        assert!(
            !max_chars["description"]
                .as_str()
                .expect("description")
                .contains("profile")
        );
    }

    #[test]
    fn invocation_policy_preserves_order_and_materializes_return_mode_without_profile_mutation() {
        let registry = BuiltinAgentToolRegistry::all();
        let mut profile = profile_with_skill_budget(100_000, 100_000);
        profile.tools.allow = vec![
            WORKSPACE_READ_FILE.to_string(),
            SKILL_READ.to_string(),
            WORKSPACE_FINISH.to_string(),
            AGENT_LIST.to_string(),
        ];
        profile.tools.deny = vec![SKILL_READ.to_string()];
        profile
            .tools
            .max_calls_per_tool
            .insert(WORKSPACE_READ_FILE.to_string(), 2);

        let root = compile_invocation_tool_snapshot(
            &registry,
            &profile,
            AgentInvocationExitPolicy::RunFinishAllowed,
            ToolSnapshotId::parse("root").unwrap(),
        )
        .unwrap();
        assert_eq!(
            root.bindings()
                .iter()
                .map(|binding| binding.tool_id().native_name())
                .collect::<Vec<_>>(),
            vec![WORKSPACE_READ_FILE, WORKSPACE_FINISH, AGENT_LIST]
        );
        assert_eq!(root.bindings()[0].max_calls(), Some(2));

        let child = compile_invocation_tool_snapshot(
            &registry,
            &profile,
            AgentInvocationExitPolicy::TaskReturnRequired,
            ToolSnapshotId::parse("child").unwrap(),
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
        assert!(
            child.bindings()[0]
                .descriptor()
                .description
                .as_deref()
                .is_some_and(|description| description.contains("task workspace file"))
        );
        assert_eq!(
            profile.tools.allow,
            vec![
                WORKSPACE_READ_FILE.to_string(),
                SKILL_READ.to_string(),
                WORKSPACE_FINISH.to_string(),
                AGENT_LIST.to_string(),
            ]
        );
    }

    #[test]
    fn agent_tool_descriptors_keep_runtime_terms_out_of_model_descriptions() {
        let registry = BuiltinAgentToolRegistry::all();
        let agent_tools = registry
            .catalog()
            .iter()
            .filter(|tool| {
                matches!(
                    tool.id.native_name(),
                    AGENT_LIST | AGENT_DELEGATE | AGENT_HANDOFF | AGENT_AWAIT | TASK_RETURN
                )
            })
            .collect::<Vec<_>>();

        for tool in agent_tools {
            let text = format!(
                "{} {}",
                tool.description.as_deref().expect("builtin description"),
                serde_json::to_string(&tool.input_schema).expect("schema JSON")
            );
            assert!(!text.contains("invocation"), "{}", tool.id);
            assert!(!text.contains("parent Agent"), "{}", tool.id);
            assert!(!text.contains("child Agent"), "{}", tool.id);
            assert!(!text.contains("This Agent"), "{}", tool.id);
            assert!(!text.contains("active control"), "{}", tool.id);
            assert!(!text.contains("active owner"), "{}", tool.id);
            assert!(!text.contains("delegated result to you"), "{}", tool.id);
            assert!(!text.contains("workspace_finish"), "{}", tool.id);
            assert!(!text.contains("to collect it"), "{}", tool.id);
            assert!(!text.contains("before finalizing"), "{}", tool.id);
            assert!(!text.contains("first version"), "{}", tool.id);
        }
    }

    fn profile_with_skill_budget(per_call: usize, per_run: usize) -> ResolvedAgentProfile {
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
            },
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
                allow: vec![SKILL_READ.to_string()],
                deny: Vec::new(),
                tool_descriptions: BTreeMap::new(),
                max_rounds: 1,
                max_calls_per_run: 1,
                max_calls_per_tool: BTreeMap::new(),
            },
            skills: AgentSkillPolicy {
                visible: vec!["*".to_string()],
                deny: Vec::new(),
                max_read_chars_per_call: per_call,
                max_read_chars_per_run: per_run,
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
}
