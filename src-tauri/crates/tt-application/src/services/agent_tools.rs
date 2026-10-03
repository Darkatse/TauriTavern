mod agent;
mod chat;
mod common;
mod dice;
mod dispatcher;
mod finish_policy;
mod policy;
mod registry;
mod runtime_context;
mod session;
mod structured;
mod workspace;
mod world_info;

pub use registry::BuiltinAgentToolRegistry;
pub(crate) use registry::{profile_can_finish_run, profile_reads_chat, profile_tool_visible};

pub(crate) use dispatcher::{AgentToolDispatchOutcome, AgentToolDispatcher, AgentToolEffect};
pub(crate) use finish_policy::{FinishPolicy, TextTurn};
pub(crate) use runtime_context::build_script_context_json;
pub(crate) use session::AgentToolSession;

pub(crate) use agent::{AGENT_AWAIT, AGENT_DELEGATE, AGENT_HANDOFF, TASK_RETURN};
#[cfg(test)]
pub(crate) use policy::builtin_model_alias;
pub(crate) use policy::{
    ExternalAgentTool, RENAMED_TOOL_PARAMETERS, TOOLS_WITH_CHANGED_MEANING,
    builtin_available_in_scope, compile_invocation_tool_snapshot, mcp_model_name,
    prepare_tool_bindings, project_agent_model_tools, stage_can_finish_run,
    unsupported_builtin_argument, visible_builtin_alias,
};
pub(crate) use workspace::{
    WORKSPACE_APPLY_PATCH, WORKSPACE_SHELL, WORKSPACE_WRITE_FILE, classify_workspace_io_error,
    offers_workspace_files, render_workspace_index,
};
