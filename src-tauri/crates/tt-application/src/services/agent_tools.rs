mod agent;
mod chat;
mod common;
mod dice;
mod dispatcher;
mod policy;
mod registry;
mod runtime_context;
mod session;
mod structured;
mod workspace;
mod world_info;

pub use registry::BuiltinAgentToolRegistry;

pub(crate) use dispatcher::{AgentToolDispatchOutcome, AgentToolDispatcher, AgentToolEffect};
pub(crate) use runtime_context::build_script_context_json;
pub(crate) use session::AgentToolSession;

pub(crate) use agent::{AGENT_AWAIT, AGENT_DELEGATE, AGENT_HANDOFF, TASK_RETURN};
pub(crate) use policy::{
    ExternalAgentTool, builtin_available_in_scope, compile_invocation_tool_snapshot,
    mcp_model_name, prepare_tool_bindings, project_agent_model_tools,
};
pub(crate) use workspace::{
    WORKSPACE_APPLY_PATCH, WORKSPACE_FINISH, WORKSPACE_SHELL, WORKSPACE_WRITE_FILE,
    classify_workspace_io_error,
};
