use serde_json::json;

use super::AgentRuntimeService;
use crate::application::errors::ApplicationError;
use crate::domain::models::agent::{AgentModelResponse, AgentRunEventLevel, WorkspacePath};

impl AgentRuntimeService {
    pub(super) async fn store_model_response(
        &self,
        run_id: &str,
        round: usize,
        response: &AgentModelResponse,
    ) -> Result<WorkspacePath, ApplicationError> {
        let path = WorkspacePath::parse(format!("model-responses/round-{round:03}.json"))?;
        let document = json!({
            "round": round,
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
                "path": path.as_str(),
                "responseId": response.provider_metadata.get("id"),
                "model": response.provider_metadata.get("model"),
            }),
        )
        .await?;

        Ok(path)
    }
}
