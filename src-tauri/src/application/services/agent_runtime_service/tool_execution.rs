use serde_json::json;

use super::AgentRuntimeService;
use super::ids::safe_workspace_file_stem;
use crate::application::errors::ApplicationError;
use crate::application::services::agent_tools::{AgentToolDispatchOutcome, AgentToolSession};
use crate::domain::models::agent::{
    AgentRunEventLevel, AgentRunStatus, AgentToolCall, AgentToolResult, WorkspacePath,
};

impl AgentRuntimeService {
    pub(super) async fn dispatch_tool_call(
        &self,
        run_id: &str,
        call: &AgentToolCall,
        session: &mut AgentToolSession,
    ) -> Result<AgentToolDispatchOutcome, ApplicationError> {
        let arguments_ref = self.store_tool_arguments(run_id, call).await?;
        self.event(
            run_id,
            AgentRunEventLevel::Info,
            "tool_call_requested",
            json!({
                "callId": call.id.as_str(),
                "name": call.name.as_str(),
                "argumentsRef": arguments_ref.as_str(),
                "providerMetadata": &call.provider_metadata,
            }),
        )
        .await?;
        self.transition_status(run_id, AgentRunStatus::DispatchingTool)
            .await?;
        self.event(
            run_id,
            AgentRunEventLevel::Info,
            "tool_call_started",
            json!({
                "callId": call.id.as_str(),
                "name": call.name.as_str(),
            }),
        )
        .await?;

        match self.tool_dispatcher.dispatch(run_id, call, session).await {
            Ok(outcome) => {
                self.store_tool_result(run_id, &outcome.result).await?;
                self.event(
                    run_id,
                    if outcome.result.is_error {
                        AgentRunEventLevel::Warn
                    } else {
                        AgentRunEventLevel::Info
                    },
                    if outcome.result.is_error {
                        "tool_call_failed"
                    } else {
                        "tool_call_completed"
                    },
                    json!({
                        "callId": outcome.result.call_id.as_str(),
                        "name": outcome.result.name.as_str(),
                        "isError": outcome.result.is_error,
                        "errorCode": outcome.result.error_code.as_deref(),
                        "message": outcome.result.is_error.then_some(outcome.result.content.as_str()),
                        "elapsedMs": outcome.elapsed_ms,
                        "resourceRefs": &outcome.result.resource_refs,
                    }),
                )
                .await?;
                Ok(outcome)
            }
            Err(error) => {
                self.event(
                    run_id,
                    AgentRunEventLevel::Error,
                    "tool_call_failed",
                    json!({
                        "callId": call.id.as_str(),
                        "name": call.name.as_str(),
                        "message": error.to_string(),
                    }),
                )
                .await?;
                Err(error)
            }
        }
    }

    async fn store_tool_result(
        &self,
        run_id: &str,
        result: &AgentToolResult,
    ) -> Result<(), ApplicationError> {
        let path = WorkspacePath::parse(format!(
            "tool-results/{}.json",
            safe_workspace_file_stem(&result.call_id)
        ))?;
        let text = serde_json::to_string_pretty(result).map_err(|error| {
            ApplicationError::ValidationError(format!(
                "agent.tool_result_serialize_failed: {error}"
            ))
        })?;
        self.workspace_repository
            .write_text(run_id, &path, &text)
            .await?;
        self.event(
            run_id,
            AgentRunEventLevel::Debug,
            "tool_result_stored",
            json!({
                "callId": result.call_id.as_str(),
                "path": path.as_str(),
            }),
        )
        .await?;
        Ok(())
    }

    async fn store_tool_arguments(
        &self,
        run_id: &str,
        call: &AgentToolCall,
    ) -> Result<WorkspacePath, ApplicationError> {
        let path = WorkspacePath::parse(format!(
            "tool-args/{}.json",
            safe_workspace_file_stem(&call.id)
        ))?;
        let text = serde_json::to_string_pretty(&call.arguments).map_err(|error| {
            ApplicationError::ValidationError(format!(
                "agent.tool_arguments_serialize_failed: {error}"
            ))
        })?;
        self.workspace_repository
            .write_text(run_id, &path, &text)
            .await?;
        Ok(path)
    }
}
