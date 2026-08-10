use std::sync::Arc;

use tauri::State;

use crate::{
    app::AppState,
    presentation::{
        commands::helpers::{log_command, map_command_error},
        errors::CommandError,
    },
};
use tt_application::dto::mcp_dto::{
    CallLegacyMcpToolDto, CreateMcpServerDto, ListLegacyMcpToolsResultDto, ListMcpServersResultDto,
    McpCallOutcomeDto, McpDiscoveryResultDto, McpExecutionCallIdDto, McpRegistrationIdDto,
    McpServerDto, McpTestCallIdDto, RenameMcpServerDto, SetMcpServerStateDto,
    SetMcpToolPermissionDto, TestMcpToolCallDto,
};

#[tauri::command]
pub async fn list_mcp_servers(
    app_state: State<'_, Arc<AppState>>,
) -> Result<ListMcpServersResultDto, CommandError> {
    log_command("list_mcp_servers");
    app_state
        .services
        .mcp_service
        .list_servers()
        .await
        .map_err(map_command_error("Failed to list MCP servers"))
}

#[tauri::command]
pub async fn create_mcp_server(
    dto: CreateMcpServerDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<McpServerDto, CommandError> {
    log_command("create_mcp_server");
    app_state
        .services
        .mcp_service
        .create_server(dto.display_name, dto.endpoint)
        .await
        .map_err(map_command_error("Failed to create MCP server"))
}

#[tauri::command]
pub async fn rename_mcp_server(
    dto: RenameMcpServerDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<McpServerDto, CommandError> {
    log_command("rename_mcp_server");
    app_state
        .services
        .mcp_service
        .rename_server(&dto.registration_id, dto.display_name)
        .await
        .map_err(map_command_error("Failed to rename MCP server"))
}

#[tauri::command]
pub async fn set_mcp_server_state(
    dto: SetMcpServerStateDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<McpServerDto, CommandError> {
    log_command("set_mcp_server_state");
    app_state
        .services
        .mcp_service
        .set_server_state(&dto.registration_id, dto.state)
        .await
        .map_err(map_command_error("Failed to update MCP server state"))
}

#[tauri::command]
pub async fn remove_mcp_server(
    dto: McpRegistrationIdDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command("remove_mcp_server");
    app_state
        .services
        .mcp_service
        .remove_server(&dto.registration_id)
        .await
        .map_err(map_command_error("Failed to remove MCP server"))
}

#[tauri::command]
pub async fn discover_mcp_tools(
    dto: McpRegistrationIdDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<McpDiscoveryResultDto, CommandError> {
    log_command("discover_mcp_tools");
    app_state
        .services
        .mcp_service
        .discover_tools(&dto.registration_id)
        .await
        .map_err(map_command_error("Failed to discover MCP tools"))
}

#[tauri::command]
pub async fn refresh_mcp_tools(
    dto: McpRegistrationIdDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<McpDiscoveryResultDto, CommandError> {
    log_command("refresh_mcp_tools");
    app_state
        .services
        .mcp_service
        .refresh_tools(&dto.registration_id)
        .await
        .map_err(map_command_error("Failed to refresh MCP tools"))
}

#[tauri::command]
pub async fn set_mcp_tool_permission(
    dto: SetMcpToolPermissionDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<McpServerDto, CommandError> {
    log_command("set_mcp_tool_permission");
    app_state
        .services
        .mcp_service
        .set_tool_permission(&dto.registration_id, dto.native_name, dto.permission)
        .await
        .map_err(map_command_error("Failed to update MCP tool permission"))
}

#[tauri::command]
pub async fn start_mcp_test_call(
    dto: McpTestCallIdDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command("start_mcp_test_call");
    app_state
        .services
        .mcp_service
        .start_call(&dto.call_id)
        .await
        .map_err(map_command_error("Failed to prepare MCP test call"))
}

#[tauri::command]
pub async fn test_mcp_tool_call(
    dto: TestMcpToolCallDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<McpCallOutcomeDto, CommandError> {
    log_command("test_mcp_tool_call");
    app_state
        .services
        .mcp_service
        .test_call(
            &dto.call_id,
            &dto.registration_id,
            dto.native_name,
            dto.arguments_json,
        )
        .await
        .map_err(map_command_error("Failed to test MCP tool call"))
}

#[tauri::command]
pub async fn cancel_mcp_test_call(
    dto: McpTestCallIdDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command("cancel_mcp_test_call");
    app_state
        .services
        .mcp_service
        .cancel_call(&dto.call_id)
        .await
        .map_err(map_command_error("Failed to cancel MCP test call"))
}

#[tauri::command]
pub async fn list_legacy_mcp_tools(
    app_state: State<'_, Arc<AppState>>,
) -> Result<ListLegacyMcpToolsResultDto, CommandError> {
    log_command("list_legacy_mcp_tools");
    app_state
        .services
        .mcp_service
        .list_legacy_tools_cached()
        .await
        .map_err(map_command_error("Failed to list Legacy MCP tools"))
}

#[tauri::command]
pub async fn start_legacy_mcp_tool_call(
    dto: McpExecutionCallIdDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command("start_legacy_mcp_tool_call");
    app_state
        .services
        .mcp_service
        .start_call(&dto.execution_call_id)
        .await
        .map_err(map_command_error("Failed to prepare Legacy MCP tool call"))
}

#[tauri::command]
pub async fn call_legacy_mcp_tool(
    dto: CallLegacyMcpToolDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<McpCallOutcomeDto, CommandError> {
    log_command("call_legacy_mcp_tool");
    app_state
        .services
        .mcp_service
        .call_legacy_tool(&dto.execution_call_id, &dto.tool_id, dto.arguments_json)
        .await
        .map_err(map_command_error("Failed to call Legacy MCP tool"))
}

#[tauri::command]
pub async fn cancel_legacy_mcp_tool_call(
    dto: McpExecutionCallIdDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command("cancel_legacy_mcp_tool_call");
    app_state
        .services
        .mcp_service
        .cancel_call(&dto.execution_call_id)
        .await
        .map_err(map_command_error("Failed to cancel Legacy MCP tool call"))
}
