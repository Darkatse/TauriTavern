use std::sync::Arc;

use tauri::{State, Webview, ipc::Channel};
use tt_contracts::extension_tools::{
    ExtensionToolDefinition, ExtensionToolEvent, ResolveExtensionToolCallDto,
    SetExtensionToolEnabledDto,
};

use crate::infrastructure::agent_extension_tools::AgentExtensionTools;
use crate::presentation::errors::CommandError;

#[tauri::command]
pub(crate) fn register_agent_extension_tool(
    definition: ExtensionToolDefinition,
    channel: Channel<ExtensionToolEvent>,
    webview: Webview,
    tools: State<'_, Arc<AgentExtensionTools>>,
) -> Result<(), CommandError> {
    tools.register(webview.label(), definition, channel)?;
    Ok(())
}

#[tauri::command]
pub(crate) fn set_agent_extension_tool_enabled(
    dto: SetExtensionToolEnabledDto,
    webview: Webview,
    tools: State<'_, Arc<AgentExtensionTools>>,
) -> Result<(), CommandError> {
    tools.set_enabled(webview.label(), &dto.tool_id, dto.enabled)?;
    Ok(())
}

#[tauri::command]
pub(crate) fn resolve_agent_extension_tool_call(
    dto: ResolveExtensionToolCallDto,
    webview: Webview,
    tools: State<'_, Arc<AgentExtensionTools>>,
) -> bool {
    tools.resolve(webview.label(), &dto.request_id, dto.result)
}
