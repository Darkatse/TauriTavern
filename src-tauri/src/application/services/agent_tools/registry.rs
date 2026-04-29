use serde_json::{Value, json};

use super::chat::{chat_read_messages_spec, chat_search_spec};
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

    pub fn openai_tools(&self) -> Vec<Value> {
        self.specs
            .iter()
            .map(|spec| {
                json!({
                    "type": "function",
                    "function": {
                        "name": spec.model_name.as_str(),
                        "description": spec.description.as_str(),
                        "parameters": &spec.input_schema,
                    }
                })
            })
            .collect()
    }

    pub fn canonical_name<'a>(&'a self, raw: &'a str) -> Option<&'a str> {
        self.specs
            .iter()
            .find(|spec| spec.model_name == raw || spec.name == raw)
            .map(|spec| spec.name.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::super::workspace::{WORKSPACE_FINISH, WORKSPACE_READ_FILE, WORKSPACE_WRITE_FILE};
    use super::*;

    #[test]
    fn registry_uses_openai_safe_model_names() {
        let registry = BuiltinAgentToolRegistry::phase2c();
        let tools = registry.openai_tools();

        assert_eq!(tools[0]["function"]["name"], "chat_search");
        assert_eq!(
            registry.canonical_name("workspace_write_file"),
            Some(WORKSPACE_WRITE_FILE)
        );
        assert_eq!(
            registry.canonical_name("workspace_read_file"),
            Some(WORKSPACE_READ_FILE)
        );
        assert_eq!(
            registry.canonical_name("workspace.finish"),
            Some(WORKSPACE_FINISH)
        );
    }
}
