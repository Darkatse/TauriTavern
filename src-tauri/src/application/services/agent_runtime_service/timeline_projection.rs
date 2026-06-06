use crate::application::dto::agent_dto::{
    AgentRunTimelineHandoffEdgeDto, AgentRunTimelineProjectionDto,
};
use crate::domain::models::agent::{
    AgentDelegationContinuation, AgentInvocation, AgentInvocationKind, AgentTaskRecord,
    ROOT_AGENT_INVOCATION_ID,
};

pub(super) fn build_run_timeline_projection(
    invocations: &[AgentInvocation],
    tasks: &[AgentTaskRecord],
) -> AgentRunTimelineProjectionDto {
    let handoff_edges = handoff_edges(tasks);
    AgentRunTimelineProjectionDto {
        foreground_invocation_ids: foreground_invocation_ids(invocations, tasks),
        handoff_edges,
    }
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

fn handoff_edges(tasks: &[AgentTaskRecord]) -> Vec<AgentRunTimelineHandoffEdgeDto> {
    let mut handoffs = tasks
        .iter()
        .filter(|task| task.continuation == AgentDelegationContinuation::TransferControl)
        .collect::<Vec<_>>();
    handoffs.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then_with(|| left.id.cmp(&right.id))
    });

    handoffs
        .into_iter()
        .map(|task| AgentRunTimelineHandoffEdgeDto {
            task_id: task.id.clone(),
            source_invocation_id: task.parent_invocation_id.clone(),
            new_invocation_id: task.child_invocation_id.clone(),
            target_profile_id: task.target_profile_id.clone(),
            workspace_key: task.workspace_key.clone(),
            status: task.status,
        })
        .collect()
}
