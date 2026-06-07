use std::collections::HashSet;

use crate::application::dto::agent_dto::{
    AgentRunTimelineDelegationEdgeDto, AgentRunTimelineInvocationDto, AgentRunTimelineProjectionDto,
};
use crate::application::errors::ApplicationError;
use crate::domain::models::agent::{
    AgentDelegationContinuation, AgentInvocation, AgentInvocationKind, AgentTaskRecord,
    ROOT_AGENT_INVOCATION_ID,
};

pub(super) fn build_run_timeline_projection(
    invocations: &[AgentInvocation],
    tasks: &[AgentTaskRecord],
) -> Result<AgentRunTimelineProjectionDto, ApplicationError> {
    validate_projection_graph(invocations, tasks)?;
    Ok(AgentRunTimelineProjectionDto {
        foreground_invocation_ids: foreground_invocation_ids(invocations, tasks),
        invocations: invocation_nodes(invocations),
        delegation_edges: delegation_edges(tasks),
    })
}

fn validate_projection_graph(
    invocations: &[AgentInvocation],
    tasks: &[AgentTaskRecord],
) -> Result<(), ApplicationError> {
    let invocation_ids = invocations
        .iter()
        .map(|invocation| invocation.id.as_str())
        .collect::<HashSet<_>>();

    for task in tasks {
        if !invocation_ids.contains(task.parent_invocation_id.as_str()) {
            return Err(ApplicationError::ValidationError(format!(
                "agent.timeline_projection_invalid: task `{}` references missing parent invocation `{}`",
                task.id, task.parent_invocation_id
            )));
        }
        if !invocation_ids.contains(task.child_invocation_id.as_str()) {
            return Err(ApplicationError::ValidationError(format!(
                "agent.timeline_projection_invalid: task `{}` references missing target invocation `{}`",
                task.id, task.child_invocation_id
            )));
        }
    }
    Ok(())
}

fn foreground_invocation_ids(
    invocations: &[AgentInvocation],
    tasks: &[AgentTaskRecord],
) -> Vec<String> {
    let mut candidates = Vec::new();
    for task in tasks {
        if task.continuation == AgentDelegationContinuation::TransferControl
            && task.child_invocation_id != ROOT_AGENT_INVOCATION_ID
        {
            candidates.push((task.child_invocation_id.clone(), task.created_at));
        }
    }
    for invocation in invocations {
        if invocation.kind == AgentInvocationKind::Handoff
            && invocation.id != ROOT_AGENT_INVOCATION_ID
        {
            candidates.push((invocation.id.clone(), invocation.created_at));
        }
    }
    candidates.sort_by(|left, right| left.1.cmp(&right.1).then_with(|| left.0.cmp(&right.0)));

    let mut ids = vec![ROOT_AGENT_INVOCATION_ID.to_string()];
    for (invocation_id, _) in candidates {
        if !ids.iter().any(|existing| existing == &invocation_id) {
            ids.push(invocation_id);
        }
    }
    ids
}

fn invocation_nodes(invocations: &[AgentInvocation]) -> Vec<AgentRunTimelineInvocationDto> {
    let mut nodes = invocations.iter().collect::<Vec<_>>();
    nodes.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then_with(|| left.id.cmp(&right.id))
    });

    nodes
        .into_iter()
        .map(|invocation| AgentRunTimelineInvocationDto {
            invocation_id: invocation.id.clone(),
            parent_invocation_id: invocation.parent_invocation_id.clone(),
            profile_id: invocation.profile_id.clone(),
            kind: invocation.kind,
            status: invocation.status,
            exit_policy: invocation.exit_policy,
            created_at: invocation.created_at,
            updated_at: invocation.updated_at,
        })
        .collect()
}

fn delegation_edges(tasks: &[AgentTaskRecord]) -> Vec<AgentRunTimelineDelegationEdgeDto> {
    let mut edges = tasks.iter().collect::<Vec<_>>();
    edges.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then_with(|| left.id.cmp(&right.id))
    });

    edges
        .into_iter()
        .map(|task| AgentRunTimelineDelegationEdgeDto {
            task_id: task.id.clone(),
            source_invocation_id: task.parent_invocation_id.clone(),
            target_invocation_id: task.child_invocation_id.clone(),
            target_profile_id: task.target_profile_id.clone(),
            workspace_key: task.workspace_key.clone(),
            continuation: task.continuation,
            status: task.status,
            result_ref: task.result_ref.clone(),
            error: task.error.clone(),
            created_at: task.created_at,
            updated_at: task.updated_at,
        })
        .collect()
}
