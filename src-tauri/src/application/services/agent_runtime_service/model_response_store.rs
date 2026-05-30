use serde_json::json;

use super::AgentRuntimeService;
use super::model_turn_display::model_response_path_for_invocation;
use crate::application::errors::ApplicationError;
use crate::domain::models::agent::{AgentModelResponse, AgentRunEventLevel, WorkspacePath};

impl AgentRuntimeService {
    pub(super) async fn store_model_response(
        &self,
        run_id: &str,
        invocation_id: &str,
        round: usize,
        response: &AgentModelResponse,
    ) -> Result<WorkspacePath, ApplicationError> {
        let path = model_response_path_for_invocation(invocation_id, round)?;
        let document = json!({
            "round": round,
            "invocationId": invocation_id,
            "response": response,
        });
        let text = serde_json::to_string_pretty(&document).map_err(|error| {
            ApplicationError::ValidationError(format!(
                "agent.model_response_serialize_failed: {error}"
            ))
        })?;

        self.workspace_repository
            .write_text(run_id, &path, &text)
            .await?;
        self.event(
            run_id,
            AgentRunEventLevel::Debug,
            "model_response_stored",
            json!({
                "round": round,
                "invocationId": invocation_id,
                "path": path.as_str(),
                "responseId": response.provider_metadata.get("id"),
                "model": response.provider_metadata.get("model"),
            }),
        )
        .await?;

        Ok(path)
    }
}
