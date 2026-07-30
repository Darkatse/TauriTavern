use tt_domain::models::agent::ArtifactTarget;

pub(super) const WORKSPACE_ROOT_UNIVERSE: [&str; 5] =
    ["output", "scratch", "plan", "summaries", "persist"];
pub(super) const MESSAGE_BODY_ARTIFACT_TARGET: ArtifactTarget = ArtifactTarget::MessageBody;
pub(super) const AGENT_AWAIT_TOOL: &str = "agent.await";
pub(super) const AGENT_DELEGATE_TOOL: &str = "agent.delegate";
pub(super) const AGENT_HANDOFF_TOOL: &str = "agent.handoff";
pub(super) const AGENT_LIST_TOOL: &str = "agent.list";
pub(super) const TASK_RETURN_TOOL: &str = "task.return";
