use super::chat::{chat_read_messages_spec, chat_search_spec};
use super::skill::{skill_list_spec, skill_read_spec};
use super::workspace::{
    WORKSPACE_APPLY_PATCH, WORKSPACE_FINISH, WORKSPACE_LIST_FILES, WORKSPACE_READ_FILE,
    WORKSPACE_WRITE_FILE, workspace_apply_patch_spec, workspace_finish_spec,
    workspace_list_files_spec, workspace_read_file_spec, workspace_write_file_spec,
};
use super::world_info::worldinfo_read_activated_spec;
use crate::application::errors::ApplicationError;
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
                chat_search_spec(),
                chat_read_messages_spec(),
                worldinfo_read_activated_spec(),
                skill_list_spec(),
                skill_read_spec(),
                workspace_list_files_spec(),
                workspace_read_file_spec(),
                workspace_write_file_spec(),
                workspace_apply_patch_spec(),
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

fn apply_profile_context(
    spec: &mut AgentToolSpec,
    profile: &ResolvedAgentProfile,
) -> Result<(), ApplicationError> {
    let visible_roots = model_roots(&profile.workspace.visible_roots);
    let writable_roots = model_roots(&profile.workspace.writable_roots);
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
                " Fully read a file before using workspace_apply_patch on it; partial reads are only for inspection."
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
        WORKSPACE_WRITE_FILE => {
            spec.description = format!(
                "Write complete UTF-8 text to a writable Agent workspace file. Use {final_path} for the final chat message body, then call workspace_finish."
            );
            set_property_description(
                spec,
                "path",
                &format!("Relative workspace path. Writable prefixes are {writable_roots}."),
            )?;
        }
        WORKSPACE_APPLY_PATCH => {
            spec.description = "Apply a precise single-file string replacement. The file must have been fully read with workspace_read_file or created by workspace_write_file in this run. old_string must match exactly and uniquely unless replace_all is true.".to_string();
            set_property_description(
                spec,
                "path",
                &format!("Relative writable workspace file path under {writable_roots}."),
            )?;
        }
        WORKSPACE_FINISH => {
            spec.description = format!(
                "Finish the Agent loop after the final artifact has been written. The default final_path is {final_path}."
            );
            set_property_description(
                spec,
                "final_path",
                &format!(
                    "Relative workspace path for the final artifact. Defaults to {final_path}."
                ),
            )?;
        }
        _ => {}
    }

    Ok(())
}

fn model_roots(roots: &[String]) -> String {
    roots
        .iter()
        .map(|root| format!("{root}/"))
        .collect::<Vec<_>>()
        .join(", ")
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

fn set_property_description(
    spec: &mut AgentToolSpec,
    property: &str,
    description: &str,
) -> Result<(), ApplicationError> {
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
    object.insert(
        "description".to_string(),
        serde_json::Value::String(description.to_string()),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::workspace::{WORKSPACE_FINISH, WORKSPACE_READ_FILE, WORKSPACE_WRITE_FILE};
    use super::*;

    #[test]
    fn registry_uses_openai_safe_model_names() {
        let registry = BuiltinAgentToolRegistry::phase2c();
        let tools = registry.specs();

        assert_eq!(tools[0].model_name, "chat_search");
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
}
