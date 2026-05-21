use std::collections::HashMap;

use crate::domain::models::skill::{SkillIndexEntry, SkillScope};
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
    effective_skills: Vec<SkillIndexEntry>,
}

impl AgentToolSession {
    pub fn new(effective_skills: Vec<SkillIndexEntry>) -> Self {
        Self {
            effective_skills,
            ..Self::default()
        }
    }

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

    pub fn effective_skills(&self) -> &[SkillIndexEntry] {
        &self.effective_skills
    }

    pub fn effective_skill_scope(&self, name: &str) -> Option<SkillScope> {
        self.effective_skills
            .iter()
            .find(|skill| skill.name == name)
            .map(|skill| skill.scope.clone())
    }
}
