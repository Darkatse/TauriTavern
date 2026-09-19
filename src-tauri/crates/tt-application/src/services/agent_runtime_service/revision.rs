use chrono::Utc;
use serde_json::{Value, json};
use uuid::Uuid;

use super::AgentRuntimeService;
use super::continuation::{InvocationFrame, RunExecutionState};
use super::invocation::model_session_id;
use crate::errors::ApplicationError;
use tt_domain::models::agent::{
    AgentInvocation, AgentInvocationKind, AgentInvocationStatus, AgentModelContentPart,
    AgentModelMessage, AgentModelRole, AgentRunEventLevel,
};

pub(super) const PREVIOUS_OUTPUT_PATH: &str = "output/previous_output.md";

impl RunExecutionState {
    pub(super) fn begin_output_revision(&mut self) -> Result<(), ApplicationError> {
        let mut prepared = self
            .foreground
            .take()
            .ok_or_else(|| {
                ApplicationError::ValidationError(
                    "agent.revision_unavailable: foreground invocation is missing".to_string(),
                )
            })?
            .prepared;
        let now = Utc::now();
        prepared.invocation = AgentInvocation {
            id: format!("inv_{}", Uuid::new_v4().simple()),
            parent_invocation_id: Some(prepared.invocation.id.clone()),
            kind: AgentInvocationKind::Revision,
            status: AgentInvocationStatus::Created,
            created_at: now,
            updated_at: now,
            ..prepared.invocation
        };
        prepared.delegation_task_id = None;
        prepared.request.provider_state["sessionId"] = json!(model_session_id(
            &prepared.invocation.run_id,
            &prepared.invocation.id,
        ));
        prepared.request.provider_state["invocationId"] = json!(prepared.invocation.id);
        prepared.request.messages.push(AgentModelMessage {
            role: AgentModelRole::User,
            parts: vec![AgentModelContentPart::Text { text: format!(
                "The original task is complete. The user would now like you to revise the output.\n\nThe current text is saved in `{PREVIOUS_OUTPUT_PATH}`. Read it first, then make the requested changes while preserving the rest."
            ) }],
            provider_metadata: Value::Null,
        });
        self.foreground = Some(InvocationFrame::new(prepared));
        self.children.clear();
        self.previous_published_state_id = self.published_state.take().map(|state| state.state_id);
        Ok(())
    }
}

impl AgentRuntimeService {
    pub(super) async fn save_revision_invocation(
        &self,
        frame: &InvocationFrame,
    ) -> Result<(), ApplicationError> {
        let invocation = &frame.prepared.invocation;
        self.invocation_repository
            .save_invocation(invocation)
            .await?;
        self.event(
            &invocation.run_id,
            AgentRunEventLevel::Info,
            "agent_invocation_created",
            json!({
                "invocationId": invocation.id,
                "parentInvocationId": invocation.parent_invocation_id,
                "profileId": invocation.profile_id,
                "kind": invocation.kind,
                "status": invocation.status,
                "exitPolicy": invocation.exit_policy,
            }),
        )
        .await?;
        Ok(())
    }
}
