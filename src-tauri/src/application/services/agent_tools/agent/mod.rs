mod specs;

pub(super) use specs::{agent_await_spec, agent_delegate_spec, agent_list_spec, task_return_spec};

pub(crate) const AGENT_AWAIT: &str = "agent.await";
pub(crate) const AGENT_DELEGATE: &str = "agent.delegate";
pub(crate) const AGENT_LIST: &str = "agent.list";
pub(crate) const TASK_RETURN: &str = "task.return";
