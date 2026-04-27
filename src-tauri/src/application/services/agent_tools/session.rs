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
}
