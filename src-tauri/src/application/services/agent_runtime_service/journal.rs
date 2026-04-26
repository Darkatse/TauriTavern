use chrono::Utc;
use serde_json::{Value, json};

use super::{AgentCancelReceiver, AgentRuntimeService};
use crate::application::errors::ApplicationError;
use crate::domain::errors::DomainError;
use crate::domain::models::agent::{
    AgentRun, AgentRunEvent, AgentRunEventLevel, AgentRunStatus, WorkspacePath,
};

impl AgentRuntimeService {
    pub(super) async fn transition_status(
        &self,
        run_id: &str,
        status: AgentRunStatus,
    ) -> Result<AgentRun, ApplicationError> {
        let mut run = self.run_repository.load_run(run_id).await?;
        run.status = status;
        run.updated_at = Utc::now();
        self.run_repository.save_run(&run).await?;
        self.event(
            run_id,
            AgentRunEventLevel::Info,
            "status_changed",
            json!({ "status": status }),
        )
        .await?;
        Ok(run)
    }

    pub(super) async fn event(
        &self,
        run_id: &str,
        level: AgentRunEventLevel,
        event_type: &str,
        payload: Value,
    ) -> Result<AgentRunEvent, ApplicationError> {
        self.run_repository
            .append_event(run_id, level, event_type, payload)
            .await
            .map_err(ApplicationError::from)
    }

    pub(super) fn ensure_not_cancelled(
        &self,
        cancel: &AgentCancelReceiver,
    ) -> Result<(), ApplicationError> {
        if *cancel.borrow() {
            return Err(DomainError::generation_cancelled_by_user().into());
        }
        Ok(())
    }

    pub(super) async fn checkpoint_workspace_file(
        &self,
        run_id: &str,
        reason: &str,
        path: WorkspacePath,
    ) -> Result<(), ApplicationError> {
        self.transition_status(run_id, AgentRunStatus::CreatingCheckpoint)
            .await?;
        let event = self
            .event(
                run_id,
                AgentRunEventLevel::Info,
                "workspace_file_written",
                json!({ "path": path.as_str() }),
            )
            .await?;
        let checkpoint = self
            .checkpoint_repository
            .create_checkpoint(run_id, reason, event.seq, &[path])
            .await?;
        self.event(
            run_id,
            AgentRunEventLevel::Info,
            "checkpoint_created",
            json!({ "checkpointId": checkpoint.id, "reason": reason }),
        )
        .await?;
        Ok(())
    }
}
