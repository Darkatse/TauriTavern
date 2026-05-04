use std::collections::HashMap;

use crate::domain::repositories::workspace_repository::WorkspaceFile;

#[derive(Debug, Clone)]
pub struct WorkspaceReadState {
    pub sha256: String,
    pub full_read: bool,
}

#[derive(Debug, Default)]
pub struct AgentToolSession {
    read_state: HashMap<String, WorkspaceReadState>,
    total_calls: usize,
    calls_per_tool: HashMap<String, usize>,
    skill_read_chars: usize,
}

impl AgentToolSession {
    pub fn remember_file(&mut self, file: &WorkspaceFile, full_read: bool) {
        self.read_state.insert(
            file.path.as_str().to_string(),
            WorkspaceReadState {
                sha256: file.sha256.clone(),
                full_read,
            },
        );
    }

    pub fn read_state(&self, path: &str) -> Option<&WorkspaceReadState> {
        self.read_state.get(path)
    }

    pub fn total_calls(&self) -> usize {
        self.total_calls
    }

    pub fn calls_for_tool(&self, name: &str) -> usize {
        self.calls_per_tool.get(name).copied().unwrap_or(0)
    }

    pub fn remember_tool_call(&mut self, name: &str) {
        self.total_calls += 1;
        *self.calls_per_tool.entry(name.to_string()).or_insert(0) += 1;
    }

    pub fn skill_read_chars(&self) -> usize {
        self.skill_read_chars
    }

    pub fn remember_skill_read_chars(&mut self, chars: usize) {
        self.skill_read_chars += chars;
    }
}
