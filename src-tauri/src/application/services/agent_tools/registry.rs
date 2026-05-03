use super::chat::{chat_read_messages_spec, chat_search_spec};
use super::skill::{skill_list_spec, skill_read_spec};
use super::workspace::{
    workspace_apply_patch_spec, workspace_finish_spec, workspace_list_files_spec,
    workspace_read_file_spec, workspace_write_file_spec,
};
use super::world_info::worldinfo_read_activated_spec;
use crate::domain::models::agent::AgentToolSpec;

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
