mod descriptors;

pub(super) use descriptors::{
    agent_await_descriptor, agent_delegate_descriptor, agent_handoff_descriptor,
    agent_list_descriptor, task_return_descriptor,
};

pub(crate) const AGENT_AWAIT: &str = "agent.await";
pub(crate) const AGENT_DELEGATE: &str = "agent.delegate";
pub(crate) const AGENT_HANDOFF: &str = "agent.handoff";
pub(crate) const AGENT_LIST: &str = "agent.list";
pub(crate) const TASK_RETURN: &str = "task.return";
