use serde_json::{Value, json};

use super::AgentRuntimeService;
use crate::errors::ApplicationError;
use tt_domain::models::agent::WorkspacePath;
use tt_domain::models::tool::InvocationToolSnapshot;
use tt_ports::workspace_fs::WorkspaceWriteGuard;

impl AgentRuntimeService {
    pub(super) async fn persist_tool_snapshot(
        &self,
        run_id: &str,
        snapshot: &InvocationToolSnapshot,
    ) -> Result<WorkspacePath, ApplicationError> {
        let path = WorkspacePath::parse(format!(
            "input/invocations/{}/tool_snapshot.json",
            snapshot.id().as_str()
        ))?;
        let text = serde_json::to_string_pretty(snapshot).map_err(|error| {
            ApplicationError::ValidationError(format!(
                "agent.tool_snapshot_serialize_failed: {error}"
            ))
        })?;
        self.workspace_files(run_id)
            .await?
            .write_text(&path, &text, WorkspaceWriteGuard::MustNotExist)
            .await?;
        Ok(path)
    }
}

pub(super) fn tool_snapshot_summary(snapshot: &InvocationToolSnapshot) -> Value {
    json!({
        "id": snapshot.id(),
        "maxCallsPerInvocation": snapshot.max_calls_per_invocation(),
        "tools": snapshot.bindings().iter().map(|binding| json!({
            "toolId": binding.tool_id(),
            "alias": binding.model_alias(),
            "maxCalls": binding.max_calls(),
        })).collect::<Vec<_>>(),
    })
}
