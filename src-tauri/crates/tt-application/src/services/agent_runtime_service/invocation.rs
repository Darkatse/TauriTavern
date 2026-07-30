use std::collections::HashSet;

use chrono::Utc;
use serde_json::json;
use uuid::Uuid;

use super::AgentRuntimeService;
use crate::errors::ApplicationError;
use tt_domain::models::agent::profile::ResolvedAgentProfile;
use tt_domain::models::agent::{
    AgentDelegationContinuation, AgentInvocation, AgentInvocationExitPolicy, AgentInvocationKind,
    AgentInvocationStatus, AgentRunEventLevel, AgentTaskRecord, AgentTaskStatus,
    ROOT_AGENT_INVOCATION_ID,
};

pub(super) struct AgentTaskTransition {
    pub(super) task: AgentTaskRecord,
    pub(super) changed: bool,
}

pub(super) fn model_session_id(run_id: &str, invocation_id: &str) -> String {
    format!("{run_id}:{invocation_id}")
}

impl AgentRuntimeService {
    pub(super) async fn ensure_root_invocation(
        &self,
        run_id: &str,
        profile: &ResolvedAgentProfile,
    ) -> Result<AgentInvocation, ApplicationError> {
        if let Some(invocation) = self
            .invocation_repository
            .try_load_invocation(run_id, ROOT_AGENT_INVOCATION_ID)
            .await?
        {
            return Ok(invocation);
        }

        let now = Utc::now();
        let invocation = AgentInvocation {
            id: ROOT_AGENT_INVOCATION_ID.to_string(),
            run_id: run_id.to_string(),
            parent_invocation_id: None,
            profile_id: profile.id.as_str().to_string(),
            kind: AgentInvocationKind::Root,
            status: AgentInvocationStatus::Created,
            exit_policy: AgentInvocationExitPolicy::RunFinishAllowed,
            created_at: now,
            updated_at: now,
        };
        self.invocation_repository
            .save_invocation(&invocation)
            .await?;
        self.event(
            run_id,
            AgentRunEventLevel::Info,
            "agent_invocation_created",
            json!({
                "invocationId": invocation.id.as_str(),
                "parentInvocationId": invocation.parent_invocation_id.as_deref(),
                "profileId": invocation.profile_id.as_str(),
                "kind": invocation.kind,
                "status": invocation.status,
                "exitPolicy": invocation.exit_policy,
            }),
        )
        .await?;
        Ok(invocation)
    }

    pub(super) async fn start_root_invocation(
        &self,
        run_id: &str,
    ) -> Result<AgentInvocation, ApplicationError> {
        let mut invocation = self
            .invocation_repository
            .load_invocation(run_id, ROOT_AGENT_INVOCATION_ID)
            .await?;
        if invocation.status == AgentInvocationStatus::Running {
            return Ok(invocation);
        }
        invocation.status = AgentInvocationStatus::Running;
        invocation.updated_at = Utc::now();
        self.invocation_repository
            .save_invocation(&invocation)
            .await?;
        self.event(
            run_id,
            AgentRunEventLevel::Info,
            "agent_invocation_started",
            json!({
                "invocationId": invocation.id.as_str(),
                "profileId": invocation.profile_id.as_str(),
                "kind": invocation.kind,
                "status": invocation.status,
                "exitPolicy": invocation.exit_policy,
            }),
        )
        .await?;
        Ok(invocation)
    }

    pub(super) async fn create_child_task(
        &self,
        run_id: &str,
        parent_invocation_id: &str,
        target_profile_id: &str,
        created_by_tool_call_id: &str,
        task: serde_json::Value,
    ) -> Result<AgentTaskRecord, ApplicationError> {
        self.create_delegation_task(
            run_id,
            parent_invocation_id,
            target_profile_id,
            created_by_tool_call_id,
            task,
            AgentDelegationContinuation::ReturnToParent,
        )
        .await
    }

    pub(super) async fn create_handoff_task(
        &self,
        run_id: &str,
        parent_invocation_id: &str,
        target_profile_id: &str,
        created_by_tool_call_id: &str,
        task: serde_json::Value,
    ) -> Result<AgentTaskRecord, ApplicationError> {
        self.create_delegation_task(
            run_id,
            parent_invocation_id,
            target_profile_id,
            created_by_tool_call_id,
            task,
            AgentDelegationContinuation::TransferControl,
        )
        .await
    }

    async fn create_delegation_task(
        &self,
        run_id: &str,
        parent_invocation_id: &str,
        target_profile_id: &str,
        created_by_tool_call_id: &str,
        task: serde_json::Value,
        continuation: AgentDelegationContinuation,
    ) -> Result<AgentTaskRecord, ApplicationError> {
        let (task_id_prefix, invocation_kind, exit_policy) = match continuation {
            AgentDelegationContinuation::ReturnToParent => (
                "task",
                AgentInvocationKind::Subagent,
                AgentInvocationExitPolicy::TaskReturnRequired,
            ),
            AgentDelegationContinuation::TransferControl => (
                "handoff",
                AgentInvocationKind::Handoff,
                AgentInvocationExitPolicy::RunFinishAllowed,
            ),
        };
        let task_id = format!("{task_id_prefix}_{}", Uuid::new_v4().simple());
        let child_invocation_id = format!("inv_{}", Uuid::new_v4().simple());
        let workspace_key = self
            .allocate_child_workspace_key(run_id, target_profile_id)
            .await?;
        let now = Utc::now();
        let invocation = AgentInvocation {
            id: child_invocation_id.clone(),
            run_id: run_id.to_string(),
            parent_invocation_id: Some(parent_invocation_id.to_string()),
            profile_id: target_profile_id.to_string(),
            kind: invocation_kind,
            status: AgentInvocationStatus::Created,
            exit_policy,
            created_at: now,
            updated_at: now,
        };
        let task = AgentTaskRecord {
            id: task_id,
            run_id: run_id.to_string(),
            parent_invocation_id: parent_invocation_id.to_string(),
            child_invocation_id,
            target_profile_id: target_profile_id.to_string(),
            workspace_key,
            continuation,
            status: AgentTaskStatus::Queued,
            task,
            created_by_tool_call_id: Some(created_by_tool_call_id.to_string()),
            result_ref: None,
            error: None,
            created_at: now,
            updated_at: now,
        };

        self.invocation_repository
            .save_invocation(&invocation)
            .await?;
        self.invocation_repository.save_task(&task).await?;
        self.event(
            run_id,
            AgentRunEventLevel::Info,
            "agent_invocation_created",
            json!({
                "invocationId": invocation.id.as_str(),
                "parentInvocationId": invocation.parent_invocation_id.as_deref(),
                "profileId": invocation.profile_id.as_str(),
                "kind": invocation.kind,
                "status": invocation.status,
                "exitPolicy": invocation.exit_policy,
                "taskId": task.id.as_str(),
            }),
        )
        .await?;
        self.event(
            run_id,
            AgentRunEventLevel::Info,
            "agent_task_registered",
            json!({
                "taskId": task.id.as_str(),
                "parentInvocationId": task.parent_invocation_id.as_str(),
                "childInvocationId": task.child_invocation_id.as_str(),
                "targetProfileId": task.target_profile_id.as_str(),
                "workspaceKey": task.workspace_key.as_str(),
                "continuation": task.continuation,
                "status": task.status,
            }),
        )
        .await?;

        Ok(task)
    }

    async fn allocate_child_workspace_key(
        &self,
        run_id: &str,
        target_profile_id: &str,
    ) -> Result<String, ApplicationError> {
        let tasks = self.invocation_repository.list_tasks(run_id).await?;
        Ok(next_child_workspace_key(
            target_profile_id,
            tasks.iter().map(|task| task.workspace_key.as_str()),
        ))
    }

    pub(super) async fn start_child_invocation(
        &self,
        run_id: &str,
        invocation_id: &str,
    ) -> Result<AgentInvocation, ApplicationError> {
        self.start_invocation(run_id, invocation_id).await
    }

    pub(super) async fn start_invocation(
        &self,
        run_id: &str,
        invocation_id: &str,
    ) -> Result<AgentInvocation, ApplicationError> {
        let mut invocation = self
            .invocation_repository
            .load_invocation(run_id, invocation_id)
            .await?;
        if invocation_is_terminal(invocation.status) {
            return Ok(invocation);
        }
        if invocation.status == AgentInvocationStatus::Running {
            return Ok(invocation);
        }
        invocation.status = AgentInvocationStatus::Running;
        invocation.updated_at = Utc::now();
        self.invocation_repository
            .save_invocation(&invocation)
            .await?;
        self.event(
            run_id,
            AgentRunEventLevel::Info,
            "agent_invocation_started",
            json!({
                "invocationId": invocation.id.as_str(),
                "parentInvocationId": invocation.parent_invocation_id.as_deref(),
                "profileId": invocation.profile_id.as_str(),
                "kind": invocation.kind,
                "status": invocation.status,
                "exitPolicy": invocation.exit_policy,
            }),
        )
        .await?;
        Ok(invocation)
    }

    pub(super) async fn transition_child_task(
        &self,
        run_id: &str,
        task_id: &str,
        status: AgentTaskStatus,
        result_ref: Option<String>,
        error: Option<String>,
    ) -> Result<AgentTaskRecord, ApplicationError> {
        Ok(self
            .transition_child_task_with_change(run_id, task_id, status, result_ref, error)
            .await?
            .task)
    }

    pub(super) async fn transition_child_task_with_change(
        &self,
        run_id: &str,
        task_id: &str,
        status: AgentTaskStatus,
        result_ref: Option<String>,
        error: Option<String>,
    ) -> Result<AgentTaskTransition, ApplicationError> {
        let mut task = self
            .invocation_repository
            .load_task(run_id, task_id)
            .await?;
        if task_is_terminal_status(task.status) {
            return Ok(AgentTaskTransition {
                task,
                changed: false,
            });
        }
        task.status = status;
        task.result_ref = result_ref;
        task.error = error;
        task.updated_at = Utc::now();
        self.invocation_repository.save_task(&task).await?;
        self.event(
            run_id,
            task_event_level(status),
            task_event_type(status),
            json!({
                "taskId": task.id.as_str(),
                "parentInvocationId": task.parent_invocation_id.as_str(),
                "childInvocationId": task.child_invocation_id.as_str(),
                "targetProfileId": task.target_profile_id.as_str(),
                "status": task.status,
                "resultRef": task.result_ref.as_deref(),
                "error": task.error.as_deref(),
            }),
        )
        .await?;
        Ok(AgentTaskTransition {
            task,
            changed: true,
        })
    }

    pub(super) async fn finish_child_invocation(
        &self,
        run_id: &str,
        invocation_id: &str,
        status: AgentInvocationStatus,
    ) -> Result<(), ApplicationError> {
        self.finish_invocation(run_id, invocation_id, status).await
    }

    pub(super) async fn finish_invocation(
        &self,
        run_id: &str,
        invocation_id: &str,
        status: AgentInvocationStatus,
    ) -> Result<(), ApplicationError> {
        let mut invocation = self
            .invocation_repository
            .load_invocation(run_id, invocation_id)
            .await?;
        if invocation_is_terminal(invocation.status) {
            return Ok(());
        }
        invocation.status = status;
        invocation.updated_at = Utc::now();
        self.invocation_repository
            .save_invocation(&invocation)
            .await?;
        self.event(
            run_id,
            AgentRunEventLevel::Info,
            terminal_invocation_event_type(status),
            json!({
                "invocationId": invocation.id.as_str(),
                "parentInvocationId": invocation.parent_invocation_id.as_deref(),
                "profileId": invocation.profile_id.as_str(),
                "kind": invocation.kind,
                "status": invocation.status,
            }),
        )
        .await?;
        Ok(())
    }
}

fn terminal_invocation_event_type(status: AgentInvocationStatus) -> &'static str {
    match status {
        AgentInvocationStatus::Completed => "agent_invocation_completed",
        AgentInvocationStatus::Failed => "agent_invocation_failed",
        AgentInvocationStatus::Cancelled => "agent_invocation_cancelled",
        AgentInvocationStatus::Transferred => "agent_invocation_transferred",
        AgentInvocationStatus::Created | AgentInvocationStatus::Running => {
            "agent_invocation_status_changed"
        }
    }
}

fn invocation_is_terminal(status: AgentInvocationStatus) -> bool {
    matches!(
        status,
        AgentInvocationStatus::Completed
            | AgentInvocationStatus::Failed
            | AgentInvocationStatus::Cancelled
            | AgentInvocationStatus::Transferred
    )
}

fn task_event_level(status: AgentTaskStatus) -> AgentRunEventLevel {
    match status {
        AgentTaskStatus::Failed | AgentTaskStatus::Cancelled => AgentRunEventLevel::Warn,
        _ => AgentRunEventLevel::Info,
    }
}

fn task_event_type(status: AgentTaskStatus) -> &'static str {
    match status {
        AgentTaskStatus::Queued => "agent_task_queued",
        AgentTaskStatus::Running => "agent_task_started",
        AgentTaskStatus::Completed => "agent_task_completed",
        AgentTaskStatus::Failed => "agent_task_failed",
        AgentTaskStatus::Cancelled => "agent_task_cancelled",
    }
}

fn task_is_terminal_status(status: AgentTaskStatus) -> bool {
    matches!(
        status,
        AgentTaskStatus::Completed | AgentTaskStatus::Failed | AgentTaskStatus::Cancelled
    )
}

fn next_child_workspace_key<'a>(
    target_profile_id: &str,
    existing_keys: impl IntoIterator<Item = &'a str>,
) -> String {
    let existing_keys = existing_keys.into_iter().collect::<HashSet<_>>();
    if !existing_keys.contains(target_profile_id) {
        return target_profile_id.to_string();
    }

    for index in 2.. {
        let candidate = format!("{target_profile_id}-{index:03}");
        if !existing_keys.contains(candidate.as_str()) {
            return candidate;
        }
    }
    unreachable!("unbounded workspace key allocation exhausted")
}

#[cfg(test)]
mod tests {
    use super::next_child_workspace_key;

    #[test]
    fn child_workspace_key_uses_agent_id_then_numbered_suffixes() {
        assert_eq!(next_child_workspace_key("scene-critic", []), "scene-critic");
        assert_eq!(
            next_child_workspace_key("scene-critic", ["scene-critic"]),
            "scene-critic-002"
        );
        assert_eq!(
            next_child_workspace_key("scene-critic", ["scene-critic", "scene-critic-002"]),
            "scene-critic-003"
        );
    }
}
