#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentRunStorageClass {
    RunJournal,
    RunSummaryProjection,
    RunContext,
    RunWorkspaceProjection,
    RunToolIo,
    WorkspaceOutputs,
    WorkspaceScratch,
    Tasks,
    ModelResponses,
    Checkpoints,
    OtherRunArtifact,
}

impl AgentRunStorageClass {
    pub fn from_run_relative_path(relative_path: &str) -> Self {
        match relative_path {
            "run.json" | "events.jsonl" => return Self::RunJournal,
            "manifest.json" => return Self::RunContext,
            _ => {}
        }

        let component = relative_path
            .split_once('/')
            .map_or(relative_path, |(component, _)| component);

        match component {
            "input" | "invocations" => Self::RunContext,
            "persist" | "summaries" | "plan" => Self::RunWorkspaceProjection,
            "tool-args" | "tool-results" | "agent-results" => Self::RunToolIo,
            "output" => Self::WorkspaceOutputs,
            "scratch" => Self::WorkspaceScratch,
            "tasks" => Self::Tasks,
            "model-responses" => Self::ModelResponses,
            "checkpoints" => Self::Checkpoints,
            _ => Self::OtherRunArtifact,
        }
    }

    pub fn run_index() -> Self {
        Self::RunJournal
    }

    pub fn run_summary_projection() -> Self {
        Self::RunSummaryProjection
    }

    pub fn is_core_history(self) -> bool {
        matches!(self, Self::RunJournal | Self::RunSummaryProjection)
    }

    pub fn is_slim_artifact(self) -> bool {
        !self.is_core_history()
    }
}

#[cfg(test)]
mod tests {
    use super::AgentRunStorageClass;

    #[test]
    fn classifies_tt_sync_agent_run_components() {
        assert_eq!(
            AgentRunStorageClass::from_run_relative_path("run.json"),
            AgentRunStorageClass::RunJournal
        );
        assert_eq!(
            AgentRunStorageClass::from_run_relative_path("events.jsonl"),
            AgentRunStorageClass::RunJournal
        );
        assert_eq!(
            AgentRunStorageClass::from_run_relative_path("manifest.json"),
            AgentRunStorageClass::RunContext
        );
        assert_eq!(
            AgentRunStorageClass::from_run_relative_path("input/prompt_snapshot.json"),
            AgentRunStorageClass::RunContext
        );
        assert_eq!(
            AgentRunStorageClass::from_run_relative_path("invocations/inv_root.json"),
            AgentRunStorageClass::RunContext
        );
        assert_eq!(
            AgentRunStorageClass::from_run_relative_path("persist/state.json"),
            AgentRunStorageClass::RunWorkspaceProjection
        );
        assert_eq!(
            AgentRunStorageClass::from_run_relative_path("summaries/root.md"),
            AgentRunStorageClass::RunWorkspaceProjection
        );
        assert_eq!(
            AgentRunStorageClass::from_run_relative_path("plan/todos.json"),
            AgentRunStorageClass::RunWorkspaceProjection
        );
        assert_eq!(
            AgentRunStorageClass::from_run_relative_path("tool-args/call-1.json"),
            AgentRunStorageClass::RunToolIo
        );
        assert_eq!(
            AgentRunStorageClass::from_run_relative_path("tool-results/call-1.json"),
            AgentRunStorageClass::RunToolIo
        );
        assert_eq!(
            AgentRunStorageClass::from_run_relative_path("agent-results/task-1.json"),
            AgentRunStorageClass::RunToolIo
        );
        assert_eq!(
            AgentRunStorageClass::from_run_relative_path("output/main.md"),
            AgentRunStorageClass::WorkspaceOutputs
        );
        assert_eq!(
            AgentRunStorageClass::from_run_relative_path("output"),
            AgentRunStorageClass::WorkspaceOutputs
        );
        assert_eq!(
            AgentRunStorageClass::from_run_relative_path("scratch/notes.md"),
            AgentRunStorageClass::WorkspaceScratch
        );
        assert_eq!(
            AgentRunStorageClass::from_run_relative_path("tasks/task-1.json"),
            AgentRunStorageClass::Tasks
        );
        assert_eq!(
            AgentRunStorageClass::from_run_relative_path("model-responses/round-001.json"),
            AgentRunStorageClass::ModelResponses
        );
        assert_eq!(
            AgentRunStorageClass::from_run_relative_path("checkpoints/cp-1.json"),
            AgentRunStorageClass::Checkpoints
        );
    }

    #[test]
    fn preserves_only_journal_and_local_summary_projection_as_core_history() {
        assert!(AgentRunStorageClass::RunJournal.is_core_history());
        assert!(AgentRunStorageClass::RunSummaryProjection.is_core_history());
        assert!(!AgentRunStorageClass::RunContext.is_core_history());
        assert!(AgentRunStorageClass::RunContext.is_slim_artifact());
        assert!(AgentRunStorageClass::OtherRunArtifact.is_slim_artifact());
    }
}
