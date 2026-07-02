use std::collections::{HashMap, hash_map::Entry};

use tt_domain::models::skill::{SkillIndexEntry, SkillScope};
use tt_ports::repositories::workspace_repository::WorkspaceFile;

#[derive(Debug, Clone)]
pub struct WorkspaceReadState {
    pub sha256: String,
    pub full_read: bool,
    observed_texts: Vec<String>,
    patch_requires_full_read: bool,
}

impl WorkspaceReadState {
    fn new(sha256: String, full_read: bool) -> Self {
        Self {
            sha256,
            full_read,
            observed_texts: Vec::new(),
            patch_requires_full_read: false,
        }
    }

    pub fn old_string_was_observed(&self, old_string: &str) -> bool {
        self.full_read
            || self
                .observed_texts
                .iter()
                .any(|text| text.contains(old_string))
    }

    pub fn patch_requires_full_read(&self) -> bool {
        self.patch_requires_full_read
    }
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
        let path = file.path.as_str().to_string();
        if full_read {
            self.read_state
                .insert(path, WorkspaceReadState::new(file.sha256.clone(), true));
            return;
        }

        let previous = self.read_state.get(&path);
        let full_read =
            previous.is_some_and(|state| state.sha256 == file.sha256 && state.full_read);
        let patch_requires_full_read =
            previous.is_some_and(WorkspaceReadState::patch_requires_full_read);
        self.read_state.insert(
            path,
            WorkspaceReadState {
                sha256: file.sha256.clone(),
                full_read,
                observed_texts: Vec::new(),
                patch_requires_full_read,
            },
        );
    }

    pub fn remember_file_read(&mut self, file: &WorkspaceFile, full_read: bool, text: &str) {
        if full_read {
            self.remember_file(file, true);
            return;
        }

        let path = file.path.as_str().to_string();
        match self.read_state.entry(path) {
            Entry::Occupied(mut entry) => {
                let state = entry.get_mut();
                if state.sha256 != file.sha256 {
                    let patch_requires_full_read = state.patch_requires_full_read;
                    *state = WorkspaceReadState {
                        sha256: file.sha256.clone(),
                        full_read: false,
                        observed_texts: Vec::new(),
                        patch_requires_full_read,
                    };
                }
                if !state.full_read && !text.is_empty() {
                    state.observed_texts.push(text.to_string());
                }
            }
            Entry::Vacant(entry) => {
                let mut state = WorkspaceReadState::new(file.sha256.clone(), false);
                if !text.is_empty() {
                    state.observed_texts.push(text.to_string());
                }
                entry.insert(state);
            }
        }
    }

    pub fn remember_partial_patch(
        &mut self,
        file: &WorkspaceFile,
        old_string: &str,
        new_string: &str,
    ) {
        let path = file.path.as_str().to_string();
        let observed_texts = self
            .read_state
            .get(&path)
            .map(|state| {
                state
                    .observed_texts
                    .iter()
                    .map(|text| text.replacen(old_string, new_string, 1))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        self.read_state.insert(
            path,
            WorkspaceReadState {
                sha256: file.sha256.clone(),
                full_read: false,
                observed_texts,
                patch_requires_full_read: false,
            },
        );
    }

    pub fn require_full_read_before_patch(&mut self, path: &str) {
        if let Some(state) = self.read_state.get_mut(path) {
            state.patch_requires_full_read = true;
        }
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
