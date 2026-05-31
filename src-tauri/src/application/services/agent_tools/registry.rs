use super::agent::{agent_await_spec, agent_delegate_spec, agent_list_spec, task_return_spec};
use super::chat::{chat_read_messages_spec, chat_search_spec};
use super::dice::dice_roll_spec;
use super::skill::{SKILL_READ, skill_list_spec, skill_read_spec, skill_search_spec};
use super::workspace::{
    WORKSPACE_APPLY_PATCH, WORKSPACE_COMMIT, WORKSPACE_FINISH, WORKSPACE_LIST_FILES,
    WORKSPACE_READ_FILE, WORKSPACE_SEARCH_FILES, WORKSPACE_WRITE_FILE, workspace_apply_patch_spec,
    workspace_commit_spec, workspace_finish_spec, workspace_list_files_spec,
    workspace_read_file_spec, workspace_search_files_spec, workspace_write_file_spec,
};
use super::world_info::worldinfo_read_activated_spec;
use crate::application::errors::ApplicationError;
use crate::application::services::agent_workspace_scope::format_model_workspace_roots;
use crate::domain::models::agent::AgentToolSpec;
use crate::domain::models::agent::profile::{AgentToolDescriptionOverride, ResolvedAgentProfile};

#[derive(Debug, Clone)]
pub struct BuiltinAgentToolRegistry {
    specs: Vec<AgentToolSpec>,
}

impl BuiltinAgentToolRegistry {
    pub fn phase2c() -> Self {
        Self {
            specs: vec![
                agent_list_spec(),
                agent_delegate_spec(),
                agent_await_spec(),
                task_return_spec(),
                chat_search_spec(),
                chat_read_messages_spec(),
                worldinfo_read_activated_spec(),
                dice_roll_spec(),
                skill_list_spec(),
                skill_search_spec(),
                skill_read_spec(),
                workspace_list_files_spec(),
                workspace_search_files_spec(),
                workspace_read_file_spec(),
                workspace_write_file_spec(),
                workspace_apply_patch_spec(),
                workspace_commit_spec(),
                workspace_finish_spec(),
            ],
        }
    }

    pub fn specs(&self) -> &[AgentToolSpec] {
        &self.specs
    }

    pub fn spec_by_name(&self, name: &str) -> Option<&AgentToolSpec> {
        self.specs.iter().find(|spec| spec.name == name)
    }

    pub fn spec_by_name_or_model_name(&self, name: &str) -> Option<&AgentToolSpec> {
        self.specs
            .iter()
            .find(|spec| spec.name == name || spec.model_name == name)
    }

    pub(crate) fn apply_return_mode_context(
        &self,
        specs: &mut [AgentToolSpec],
        profile: &ResolvedAgentProfile,
    ) -> Result<(), ApplicationError> {
        for spec in specs {
            apply_return_mode_context(spec, profile)?;
        }
        Ok(())
    }

    pub fn visible_specs(
        &self,
        profile: &ResolvedAgentProfile,
    ) -> Result<Vec<AgentToolSpec>, ApplicationError> {
        let mut specs = Vec::new();
        for name in &profile.tools.allow {
            if profile.tools.deny.iter().any(|denied| denied == name) {
                continue;
            }
            let mut spec = self
                .spec_by_name(name)
                .ok_or_else(|| {
                    ApplicationError::ValidationError(format!(
                        "agent.profile_unknown_tool: unknown tool `{name}`"
                    ))
                })?
                .clone();
            apply_profile_context(&mut spec, profile)?;
            if let Some(override_) = profile.tools.tool_descriptions.get(name) {
                apply_description_override(&mut spec, override_)?;
            }
            specs.push(spec);
        }
        Ok(specs)
    }
}

fn apply_return_mode_context(
    spec: &mut AgentToolSpec,
    profile: &ResolvedAgentProfile,
) -> Result<(), ApplicationError> {
    let visible_roots = format_model_workspace_roots(&profile.workspace.visible_roots);
    let writable_roots = format_model_workspace_roots(&profile.workspace.writable_roots);
    match spec.name.as_str() {
        WORKSPACE_LIST_FILES => {
            spec.description = format!(
                "List files visible to this delegated task under {visible_roots}. This is the same logical workspace used by the requesting Agent; use the paths named in the task brief."
            );
            set_property_description(
                spec,
                "path",
                &format!(
                    "Optional task workspace path under {visible_roots}. Omit to list visible roots."
                ),
            )?;
        }
        WORKSPACE_READ_FILE => {
            spec.description = format!(
                "Read a visible UTF-8 task workspace file with line numbers. Visible roots are {visible_roots}. Use ordinary workspace paths exactly as they appear in the task brief or file list."
            );
            set_property_description(
                spec,
                "path",
                &format!("Visible task workspace file path under {visible_roots}."),
            )?;
        }
        WORKSPACE_SEARCH_FILES => {
            spec.description = format!(
                "Search visible UTF-8 task workspace files under {visible_roots}. Use this before reading exact ranges."
            );
            set_property_description(
                spec,
                "path",
                "Optional visible task workspace file or directory path. Omit to search all visible task paths.",
            )?;
        }
        WORKSPACE_WRITE_FILE => {
            spec.description = format!(
                "Write UTF-8 text to a writable workspace file for this delegated task. mode replace writes the complete file; mode append adds content exactly to the end and creates the file when missing. Writable prefixes are {writable_roots}. Use the path requested in the task brief when one is provided."
            );
            set_property_description(
                spec,
                "path",
                &format!(
                    "Writable task path under {writable_roots}. Use the path requested in the task when one is provided."
                ),
            )?;
        }
        WORKSPACE_APPLY_PATCH => {
            spec.description = format!(
                "Apply a precise single-file string replacement to a writable delegated-task workspace file. Writable prefixes are {writable_roots}. Fully read an existing file before editing it; if the tool reports that it changed, read it again and retry."
            );
            set_property_description(
                spec,
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
    spec: &mut AgentToolSpec,
    profile: &ResolvedAgentProfile,
) -> Result<(), ApplicationError> {
    let visible_roots = format_model_workspace_roots(&profile.workspace.visible_roots);
    let writable_roots = format_model_workspace_roots(&profile.workspace.writable_roots);
    let final_path = profile.output.message_body_path.as_str();

    match spec.name.as_str() {
        WORKSPACE_LIST_FILES => {
            spec.description = format!(
                "List visible Agent workspace files under {visible_roots}. Use this before reading when you need to inspect available artifacts."
            );
            set_property_description(
                spec,
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
            spec.description =
                format!("Read a visible UTF-8 Agent workspace file with line numbers.{patch_hint}");
            set_property_description(
                spec,
                "path",
                &format!("Relative workspace file path under {visible_roots}."),
            )?;
        }
        WORKSPACE_SEARCH_FILES => {
            spec.description = format!(
                "Search visible UTF-8 Agent workspace files under {visible_roots}. Results return snippets and refs; use workspace_read_file to read exact ranges."
            );
            set_property_description(
                spec,
                "path",
                &format!(
                    "Optional visible workspace file or directory path under {visible_roots}. Omit to search all visible roots."
                ),
            )?;
        }
        WORKSPACE_WRITE_FILE => {
            spec.description = format!(
                "Write UTF-8 text to a writable Agent workspace file. mode replace writes the complete file; mode append adds content exactly to the end and creates the file when missing. Use {final_path} for the default chat message body."
            );
            set_property_description(
                spec,
                "path",
                &format!("Relative workspace path. Writable prefixes are {writable_roots}."),
            )?;
        }
        WORKSPACE_APPLY_PATCH => {
            spec.description = "Apply a precise single-file string replacement. old_string must come from text you already read with workspace_read_file or from a file you created/replaced in this run. old_string must match exactly and uniquely unless replace_all is true. If a patch fails, fully read the file before retrying.".to_string();
            set_property_description(
                spec,
                "path",
                &format!("Relative writable workspace file path under {writable_roots}."),
            )?;
        }
        WORKSPACE_COMMIT => {
            spec.description = format!(
                "Commit a workspace text file to this run's single chat message. With no arguments, replace the current run message with {final_path}. mode append appends the file text to the same message, creating it when this run has not committed yet."
            );
            set_property_description(
                spec,
                "path",
                &format!(
                    "Relative visible workspace file path to publish. Defaults to {final_path}."
                ),
            )?;
        }
        WORKSPACE_FINISH => {
            spec.description =
                "Finish the Agent run after required chat commits and workspace changes are complete."
                    .to_string();
        }
        SKILL_READ => {
            let per_call = profile.skills.max_read_chars_per_call;
            let per_run = profile.skills.max_read_chars_per_run;
            set_property_description(
                spec,
                "max_chars",
                &format!(
                    "Maximum characters to return in this skill_read call. Current policy allows up to {per_call} characters per call and {per_run} total Skill characters per run; the remaining run budget also applies. Omit to use the available per-call budget."
                ),
            )?;
            set_integer_property_bounds(spec, "max_chars", 1, per_call)?;
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
    spec: &mut AgentToolSpec,
    override_: &AgentToolDescriptionOverride,
) -> Result<(), ApplicationError> {
    if let Some(description) = override_.description.as_ref() {
        spec.description = description.trim().to_string();
    }

    if override_.properties.is_empty() {
        return Ok(());
    }

    for (property, description) in &override_.properties {
        set_property_description(spec, property, description.trim())?;
    }
    Ok(())
}

fn set_integer_property_bounds(
    spec: &mut AgentToolSpec,
    property: &str,
    minimum: usize,
    maximum: usize,
) -> Result<(), ApplicationError> {
    let object = property_schema_object_mut(spec, property)?;
    object.insert("minimum".to_string(), serde_json::json!(minimum));
    object.insert("maximum".to_string(), serde_json::json!(maximum));
    Ok(())
}

fn set_property_description(
    spec: &mut AgentToolSpec,
    property: &str,
    description: &str,
) -> Result<(), ApplicationError> {
    let object = property_schema_object_mut(spec, property)?;
    object.insert(
        "description".to_string(),
        serde_json::Value::String(description.to_string()),
    );
    Ok(())
}

fn property_schema_object_mut<'a>(
    spec: &'a mut AgentToolSpec,
    property: &str,
) -> Result<&'a mut serde_json::Map<String, serde_json::Value>, ApplicationError> {
    let properties = spec
        .input_schema
        .get_mut("properties")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| {
            ApplicationError::ValidationError(format!(
                "agent.profile_tool_properties_invalid: `{}` has no object properties",
                spec.name
            ))
        })?;
    let schema = properties.get_mut(property).ok_or_else(|| {
        ApplicationError::ValidationError(format!(
            "agent.profile_unknown_tool_property: `{}` has no property `{property}`",
            spec.name
        ))
    })?;
    let object = schema.as_object_mut().ok_or_else(|| {
        ApplicationError::ValidationError(format!(
            "agent.profile_tool_property_schema_invalid: `{}` property `{property}` is not an object",
            spec.name
        ))
    })?;
    Ok(object)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::super::agent::{AGENT_AWAIT, AGENT_DELEGATE, AGENT_LIST, TASK_RETURN};
    use super::super::dice::DICE_ROLL;
    use super::super::workspace::{WORKSPACE_FINISH, WORKSPACE_READ_FILE, WORKSPACE_WRITE_FILE};
    use super::*;
    use crate::domain::models::agent::plan::{AgentPlanMode, AgentPlanPolicy};
    use crate::domain::models::agent::profile::{
        AGENT_PROFILE_KIND, AGENT_PROFILE_SCHEMA_VERSION, AgentContextPolicy,
        AgentDelegationPolicy, AgentModelBinding, AgentModelBindingMode, AgentPresetBinding,
        AgentPresetBindingMode, AgentProfileId, AgentProfileInstructions, AgentProfileSourceTrace,
        AgentRunPolicy, AgentSkillPolicy, AgentToolPolicy, AgentWorkspacePolicy,
        ResolvedAgentOutputPolicy, ResolvedAgentProfile,
    };
    use crate::domain::models::agent::{AgentRunPresentation, ArtifactSpec, ArtifactTarget};

    #[test]
    fn registry_uses_openai_safe_model_names() {
        let registry = BuiltinAgentToolRegistry::phase2c();
        let tools = registry.specs();

        assert_eq!(tools[0].model_name, "agent_list");
        assert_eq!(tools[0].name, AGENT_LIST);
        assert_eq!(tools[1].model_name, "agent_delegate");
        assert_eq!(tools[1].name, AGENT_DELEGATE);
        assert_eq!(tools[2].model_name, "agent_await");
        assert_eq!(tools[2].name, AGENT_AWAIT);
        assert_eq!(tools[3].model_name, "task_return");
        assert_eq!(tools[3].name, TASK_RETURN);
        assert_eq!(tools[4].model_name, "chat_search");
        assert_eq!(
            tools
                .iter()
                .find(|spec| spec.model_name == "dice_roll")
                .map(|spec| spec.name.as_str()),
            Some(DICE_ROLL)
        );
        assert_eq!(
            tools
                .iter()
                .find(|spec| spec.model_name == "skill_read")
                .map(|spec| spec.name.as_str()),
            Some("skill.read")
        );
        assert_eq!(
            tools
                .iter()
                .find(|spec| spec.model_name == "workspace_write_file")
                .map(|spec| spec.name.as_str()),
            Some(WORKSPACE_WRITE_FILE)
        );
        assert_eq!(
            tools
                .iter()
                .find(|spec| spec.model_name == "workspace_read_file")
                .map(|spec| spec.name.as_str()),
            Some(WORKSPACE_READ_FILE)
        );
        assert_eq!(
            tools
                .iter()
                .find(|spec| spec.name == WORKSPACE_FINISH)
                .map(|spec| spec.name.as_str()),
            Some(WORKSPACE_FINISH)
        );
    }

    #[test]
    fn agent_delegate_requires_objective_but_not_title() {
        let registry = BuiltinAgentToolRegistry::phase2c();
        let delegate = registry
            .specs()
            .iter()
            .find(|spec| spec.name == AGENT_DELEGATE)
            .expect("agent.delegate spec");

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
    }

    #[test]
    fn visible_specs_expose_profile_skill_read_budget() {
        let registry = BuiltinAgentToolRegistry::phase2c();
        let profile = profile_with_skill_budget(100_000, 100_000);
        let tools = registry.visible_specs(&profile).expect("visible specs");
        let skill_read = tools
            .iter()
            .find(|tool| tool.name == SKILL_READ)
            .expect("skill.read spec");
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
    fn agent_tool_specs_keep_runtime_terms_out_of_model_descriptions() {
        let registry = BuiltinAgentToolRegistry::phase2c();
        let agent_tools = registry
            .specs()
            .iter()
            .filter(|tool| {
                matches!(
                    tool.name.as_str(),
                    AGENT_LIST | AGENT_DELEGATE | AGENT_AWAIT | TASK_RETURN
                )
            })
            .collect::<Vec<_>>();

        for tool in agent_tools {
            let text = format!(
                "{} {}",
                tool.description,
                serde_json::to_string(&tool.input_schema).expect("schema JSON")
            );
            assert!(!text.contains("invocation"), "{}", tool.name);
            assert!(!text.contains("parent Agent"), "{}", tool.name);
            assert!(!text.contains("child Agent"), "{}", tool.name);
            assert!(!text.contains("workspace_finish"), "{}", tool.name);
            assert!(!text.contains("to collect it"), "{}", tool.name);
            assert!(!text.contains("before finalizing"), "{}", tool.name);
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
