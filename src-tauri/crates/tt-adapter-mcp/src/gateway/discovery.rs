use std::collections::{BTreeMap, HashSet};

use rmcp::model::{PaginatedRequestParams, Tool};
use serde_json::{Value, json};

use tt_domain::{errors::DomainError, models::mcp::validate_native_tool_name};
use tt_ports::mcp::{McpDiscoveredTool, McpToolDiagnostic};

const MAX_DISCOVERY_PAGES: usize = 32;
const MAX_DISCOVERED_TOOLS: usize = 512;
pub(super) const MAX_TOOL_BYTES: usize = 256 * 1024;
const MAX_CATALOG_BYTES: usize = 8 * 1024 * 1024;

pub(super) async fn list_tools(
    peer: &rmcp::service::Peer<rmcp::RoleClient>,
    stop_after: Option<&str>,
) -> Result<Vec<Tool>, DomainError> {
    let mut tools = Vec::new();
    let mut catalog_bytes = 0usize;
    let mut cursor = None;
    let mut seen_cursors = HashSet::new();

    for page_index in 0..MAX_DISCOVERY_PAGES {
        let params = cursor
            .clone()
            .map(|cursor| PaginatedRequestParams::default().with_cursor(Some(cursor)));
        let page = peer.list_tools(params).await.map_err(|error| {
            DomainError::transient(format!("mcp.discovery_list_failed: {error}"))
        })?;
        if page
            .result_type
            .as_ref()
            .is_some_and(|result_type| !result_type.is_complete())
        {
            return Err(DomainError::InvalidData(
                "mcp.discovery_result_type_unsupported: tools/list did not return a complete result"
                    .to_string(),
            ));
        }
        if tools.len().saturating_add(page.tools.len()) > MAX_DISCOVERED_TOOLS {
            return Err(DomainError::InvalidData(format!(
                "mcp.discovery_tool_limit: catalog exceeds {MAX_DISCOVERED_TOOLS} tools"
            )));
        }
        for tool in &page.tools {
            catalog_bytes = catalog_bytes.saturating_add(
                serde_json::to_vec(tool)
                    .map_err(|error| {
                        DomainError::InvalidData(format!("mcp.tool_serialize_failed: {error}"))
                    })?
                    .len(),
            );
            if catalog_bytes > MAX_CATALOG_BYTES {
                return Err(DomainError::InvalidData(format!(
                    "mcp.discovery_catalog_size_limit: catalog exceeds {MAX_CATALOG_BYTES} bytes"
                )));
            }
        }
        let target_found = stop_after.is_some_and(|native_name| {
            page.tools
                .iter()
                .any(|tool| tool.name.as_ref() == native_name)
        });
        tools.extend(page.tools);
        if target_found {
            return Ok(tools);
        }

        let Some(next_cursor) = page.next_cursor else {
            return Ok(tools);
        };
        if !seen_cursors.insert(next_cursor.clone()) {
            return Err(DomainError::InvalidData(format!(
                "mcp.discovery_cursor_cycle: server repeated cursor `{next_cursor}`"
            )));
        }
        if page_index + 1 == MAX_DISCOVERY_PAGES {
            return Err(DomainError::InvalidData(format!(
                "mcp.discovery_page_limit: catalog exceeds {MAX_DISCOVERY_PAGES} pages"
            )));
        }
        cursor = Some(next_cursor);
    }

    unreachable!("page limit exits inside the loop")
}

pub(super) fn validate_tools(tools: Vec<Tool>) -> (Vec<McpDiscoveredTool>, Vec<McpToolDiagnostic>) {
    let mut groups = BTreeMap::<String, Vec<Tool>>::new();
    for tool in tools {
        groups.entry(tool.name.to_string()).or_default().push(tool);
    }

    let mut discovered = Vec::with_capacity(groups.len());
    let mut diagnostics = Vec::new();
    for (native_name, mut group) in groups {
        if group.len() > 1 {
            diagnostics.push(McpToolDiagnostic {
                code: "mcp.tool_duplicate_name".to_string(),
                native_name: Some(native_name.clone()),
                message: format!(
                    "Server returned {} tools named `{native_name}`; the whole name group was isolated",
                    group.len()
                ),
            });
            continue;
        }
        let tool = group.pop().expect("one-element tool group");
        match validate_tool(tool) {
            Ok((tool, warning)) => {
                discovered.push(tool);
                if let Some(warning) = warning {
                    diagnostics.push(McpToolDiagnostic {
                        code: warning.code.to_string(),
                        native_name: Some(native_name),
                        message: warning.message,
                    });
                }
            }
            Err(error) => diagnostics.push(McpToolDiagnostic {
                code: error.code.to_string(),
                native_name: Some(native_name),
                message: error.message,
            }),
        }
    }
    (discovered, diagnostics)
}

#[derive(Debug)]
pub(super) struct ToolValidationError {
    code: &'static str,
    message: String,
}

pub(super) fn validate_tool(
    tool: Tool,
) -> Result<(McpDiscoveredTool, Option<ToolValidationError>), ToolValidationError> {
    let native_name = tool.name.to_string();
    validate_native_tool_name(&native_name).map_err(|error| ToolValidationError {
        code: "mcp.tool_name_invalid",
        message: error.to_string(),
    })?;
    let encoded_size = serde_json::to_vec(&tool)
        .map_err(|error| ToolValidationError {
            code: "mcp.tool_serialize_failed",
            message: error.to_string(),
        })?
        .len();
    if encoded_size > MAX_TOOL_BYTES {
        return Err(ToolValidationError {
            code: "mcp.tool_size_limit",
            message: format!("Tool `{native_name}` exceeds {MAX_TOOL_BYTES} bytes"),
        });
    }

    let input_schema = Value::Object(tool.input_schema.as_ref().clone());
    validate_schema(&input_schema).map_err(|message| ToolValidationError {
        code: "mcp.tool_input_schema_invalid",
        message: format!("Tool `{native_name}` input schema is invalid: {message}"),
    })?;
    let mut output_warning = None;
    let output_schema = tool
        .output_schema
        .as_ref()
        .map(|schema| Value::Object(schema.as_ref().clone()))
        .and_then(|schema| match validate_schema(&schema) {
            Ok(()) => Some(schema),
            Err(message) => {
                output_warning = Some(ToolValidationError {
                    code: "mcp.tool_output_schema_invalid",
                    message: format!("Tool `{native_name}` output schema is invalid: {message}"),
                });
                None
            }
        });
    let annotations = tool
        .annotations
        .map(serde_json::to_value)
        .transpose()
        .map_err(|error| ToolValidationError {
            code: "mcp.tool_annotations_invalid",
            message: format!("Tool `{native_name}` annotations are invalid: {error}"),
        })?
        .unwrap_or_else(|| json!({}));

    Ok((
        McpDiscoveredTool {
            native_name,
            title: tool.title,
            description: tool.description.map(|value| value.into_owned()),
            input_schema,
            output_schema,
            annotations,
        },
        output_warning,
    ))
}

pub(super) fn validate_schema(schema: &Value) -> Result<(), String> {
    jsonschema::draft202012::options()
        .build(schema)
        .map(|_| ())
        .map_err(|error| error.to_string())
}
