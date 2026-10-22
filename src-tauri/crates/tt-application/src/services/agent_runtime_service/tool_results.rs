use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tt_contracts::extension_tools::ExtensionToolReply;
use tt_domain::models::agent::{
    AgentRunEventLevel, AgentToolImage, AgentToolResult, WorkspacePath,
};
use tt_domain::models::tool::{InvocationToolSnapshot, ToolId, ToolInvocation};
use tt_domain::text_metrics::TextMetrics;
use tt_ports::mcp::McpKnownResponse;
use tt_ports::tool_images::{MAX_TOOL_IMAGE_EDGE, ToolImageRejection};
use tt_ports::workspace_fs::WorkspaceWriteGuard;

use super::markdown::render_markdown_value;
use super::{AgentRuntimeService, PreparedInvocation};
use crate::errors::ApplicationError;
use crate::services::agent_tools::{AgentToolDispatchOutcome, AgentToolEffect, IncomingToolImage};
use crate::services::hashing::hex_lower;

const TOOL_CALL_AUDIT_DIGEST_BYTES: usize = 8;
const RESULT_CONTENT_CHUNK_CHARS: usize = 3_000;

impl AgentRuntimeService {
    pub(super) async fn record_tool_outcome_for_model(
        &self,
        prepared: &PreparedInvocation,
        round: usize,
        outcome: &mut AgentToolDispatchOutcome,
    ) -> Result<(), ApplicationError> {
        let run_id = prepared.invocation.run_id.as_str();
        let invocation_id = prepared.invocation.id.as_str();
        let profile = &prepared.profile;
        let file_base = self
            .tool_call_file_base(run_id, invocation_id, round, &outcome.result.call_id)
            .await?;
        let image_notes = self.store_tool_images(run_id, &file_base, outcome).await?;
        let mcp_content = outcome
            .result
            .tool_id
            .provider_id()
            .starts_with("mcp/")
            .then(|| mcp_model_content(&outcome.result));
        outcome.result.content.push_str(&image_notes);
        let result_path = self
            .record_tool_outcome(prepared, round, &file_base, outcome)
            .await?;
        if !outcome.result.tool_id.is_builtin() {
            let readable_path = WorkspacePath::parse(format!("{file_base}.txt"))?;
            let mut projected = outcome.result.clone();
            if let Some(mut content) = mcp_content {
                content.push_str(&image_notes);
                projected.content = content;
            }
            if let Some(readable) = project_external_result_for_model(
                &mut projected,
                &result_path,
                &readable_path,
                &prepared.tool_snapshot,
                profile.tools.external_result_inline_char_limit,
            )? {
                self.workspace_files(run_id)
                    .await?
                    .write_text(&readable_path, &readable, WorkspaceWriteGuard::MustNotExist)
                    .await?;
                self.event(
                    run_id,
                    AgentRunEventLevel::Debug,
                    "tool_result_readable_view_stored",
                    json!({
                        "invocationId": invocation_id,
                        "round": round,
                        "callId": outcome.result.call_id.as_str(),
                        "toolId": outcome.result.tool_id.as_str(),
                        "path": readable_path.as_str(),
                        "auditPath": result_path.as_str(),
                    }),
                )
                .await?;
            }
            outcome.result = projected;
        }
        Ok(())
    }

    async fn store_tool_images(
        &self,
        run_id: &str,
        file_base: &str,
        outcome: &mut AgentToolDispatchOutcome,
    ) -> Result<String, ApplicationError> {
        if outcome.images.is_empty() {
            return Ok(String::new());
        }
        let workspace = self.workspace_files(run_id).await?;
        let mut notes = String::new();
        for (index, image) in std::mem::take(&mut outcome.images).into_iter().enumerate() {
            let normalized = match image {
                Ok(data) => {
                    let normalizer = self.tool_image_normalizer.clone();
                    tokio::task::spawn_blocking(move || normalizer.normalize(&data))
                        .await
                        .map_err(|error| {
                            ApplicationError::InternalError(format!(
                                "agent.tool_image_normalization_failed: {error}"
                            ))
                        })?
                }
                Err(rejection) => Err(rejection),
            };
            match normalized {
                Ok(image) => {
                    let (mime_type, extension) = image.format.properties();
                    let path =
                        WorkspacePath::parse(format!("{file_base}-{}.{extension}", index + 1))?;
                    workspace.create_text_parent(&path).await?;
                    workspace
                        .write_file(&path, &image.data, WorkspaceWriteGuard::MustNotExist)
                        .await?;
                    outcome.result.images.push(AgentToolImage {
                        path: path.as_str().to_owned(),
                        mime_type: mime_type.to_owned(),
                    });
                }
                Err(ToolImageRejection::Undecodable) => {
                    notes.push_str(
                        "\n\n[The tool also returned an image that could not be decoded.]",
                    );
                }
                Err(ToolImageRejection::TooLarge { bytes }) => {
                    notes.push_str(&format!(
                        "\n\n[The tool also returned an image of {:.2} MB even after it was reduced to {MAX_TOOL_IMAGE_EDGE} pixels, which is too large to show.]",
                        bytes as f64 / 1_000_000.0
                    ));
                }
                Err(ToolImageRejection::EncodingFailed(message)) => {
                    return Err(ApplicationError::InternalError(format!(
                        "agent.tool_image_encoding_failed: {message}"
                    )));
                }
            }
        }
        Ok(notes)
    }

    pub(super) async fn record_tool_outcome(
        &self,
        prepared: &PreparedInvocation,
        round: usize,
        file_base: &str,
        outcome: &AgentToolDispatchOutcome,
    ) -> Result<WorkspacePath, ApplicationError> {
        let run_id = prepared.invocation.run_id.as_str();
        let invocation_id = prepared.invocation.id.as_str();
        let snapshot_id = prepared.tool_snapshot.id().as_str();
        let path = self
            .store_tool_result(run_id, invocation_id, round, file_base, &outcome.result)
            .await?;
        let error_message = outcome.result.is_error.then(|| {
            if outcome.result.tool_id.is_builtin() {
                outcome.result.content.clone()
            } else {
                format!(
                    "External tool returned an error; full result: {}",
                    path.as_str()
                )
            }
        });
        self.event(
            run_id,
            if outcome.result.is_error {
                AgentRunEventLevel::Warn
            } else {
                AgentRunEventLevel::Info
            },
            if outcome.result.is_error {
                "tool_call_failed"
            } else {
                "tool_call_completed"
            },
            json!({
                "round": round,
                "invocationId": invocation_id,
                "callId": outcome.result.call_id.as_str(),
                "toolId": outcome.result.tool_id.as_str(),
                "snapshotId": snapshot_id,
                "name": outcome.result.tool_id.native_name(),
                "isError": outcome.result.is_error,
                "errorCode": outcome.result.error_code.as_deref(),
                "message": error_message,
                "elapsedMs": outcome.elapsed_ms,
                "resourceRefs": &outcome.result.resource_refs,
            }),
        )
        .await?;
        Ok(path)
    }

    async fn store_tool_result(
        &self,
        run_id: &str,
        invocation_id: &str,
        round: usize,
        file_base: &str,
        result: &AgentToolResult,
    ) -> Result<WorkspacePath, ApplicationError> {
        let path = WorkspacePath::parse(format!("{file_base}.json"))?;
        let text = serde_json::to_string_pretty(result).map_err(|error| {
            ApplicationError::ValidationError(format!(
                "agent.tool_result_serialize_failed: {error}"
            ))
        })?;
        self.workspace_files(run_id)
            .await?
            .write_text(&path, &text, WorkspaceWriteGuard::MustNotExist)
            .await?;
        self.event(
            run_id,
            AgentRunEventLevel::Debug,
            "tool_result_stored",
            json!({
                "invocationId": invocation_id,
                "round": round,
                "callId": result.call_id.as_str(),
                "toolId": result.tool_id.as_str(),
                "path": path.as_str(),
            }),
        )
        .await?;
        Ok(path)
    }

    pub(super) async fn tool_call_file_base(
        &self,
        run_id: &str,
        invocation_id: &str,
        round: usize,
        call_id: &str,
    ) -> Result<String, ApplicationError> {
        let root = if self
            .active_run_handle(run_id)
            .await?
            .target
            .session_id()
            .is_some()
        {
            format!("tool-results/{run_id}")
        } else {
            "tool-results".to_string()
        };
        Ok(format!(
            "{root}/{invocation_id}/round-{round:03}-{}",
            tool_call_audit_file_stem(call_id)
        ))
    }
}

pub(super) fn extension_reply_result(
    call: &ToolInvocation,
    reply: ExtensionToolReply,
    elapsed_ms: u128,
) -> AgentToolDispatchOutcome {
    match reply {
        ExtensionToolReply::Value { value, images } => {
            let content = match &value {
                Value::String(text) => text.clone(),
                _ => value.to_string(),
            };
            let result = AgentToolResult::ok(call, content, value);
            let images = decode_tool_images(images.into_iter().map(|image| image.data));
            AgentToolDispatchOutcome::new(result, AgentToolEffect::None, elapsed_ms)
                .with_images(images)
        }
        ExtensionToolReply::Error { message } => {
            AgentToolDispatchOutcome::error(call, "extension.tool_error", &message, elapsed_ms)
        }
    }
}

fn decode_tool_images(images: impl IntoIterator<Item = String>) -> Vec<IncomingToolImage> {
    images
        .into_iter()
        .map(|data| {
            STANDARD
                .decode(data)
                .map_err(|_| ToolImageRejection::Undecodable)
        })
        .collect()
}

fn project_external_result_for_model(
    result: &mut AgentToolResult,
    audit_path: &WorkspacePath,
    readable_path: &WorkspacePath,
    snapshot: &InvocationToolSnapshot,
    inline_char_limit: usize,
) -> Result<Option<String>, ApplicationError> {
    let char_count = TextMetrics::from_text(&result.content).chars;
    if char_count <= inline_char_limit {
        return Ok(None);
    }

    let readable = line_addressable_content(&result.content);
    externalize_result(
        result,
        audit_path,
        readable_path,
        snapshot,
        char_count,
        inline_char_limit,
    )?;
    Ok(Some(readable))
}

fn mcp_model_content(result: &AgentToolResult) -> String {
    let structured_content = result
        .structured
        .get("structuredContent")
        .filter(|value| !value.is_null());
    let mut sections = Vec::new();
    let text = result.content.trim();
    if !text.is_empty()
        && !structured_content.is_some_and(|value| text_is_serialized_value(text, value))
    {
        sections.push(text.to_string());
    }

    if let Some(value) = structured_content {
        sections.push(markdown_value_section("Details", value));
    }

    let notes = result
        .structured
        .get("diagnostics")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|diagnostic| {
            diagnostic.get("code").and_then(Value::as_str) != Some("mcp.call_metadata_unsupported")
        })
        .filter_map(|diagnostic| diagnostic.get("message").and_then(Value::as_str))
        .map(|message| format!("- {}", message.trim()))
        .collect::<Vec<_>>();
    if !notes.is_empty() {
        sections.push(format!("## Notes\n\n{}", notes.join("\n")));
    }

    if let Some(server_error) = result.structured.get("serverError")
        && let Some(data) = server_error.get("data").filter(|value| !value.is_null())
    {
        sections.push(markdown_value_section("Error details", data));
    }

    if sections.is_empty() && result.images.is_empty() {
        "The tool completed without returning content.".to_string()
    } else {
        sections.join("\n\n")
    }
}

fn markdown_value_section(title: &str, value: &Value) -> String {
    format!("## {title}\n\n{}", render_markdown_value(value, 0))
}

fn text_is_serialized_value(text: &str, value: &Value) -> bool {
    serde_json::from_str::<Value>(text).is_ok_and(|parsed| parsed == *value)
}

fn externalize_result(
    result: &mut AgentToolResult,
    audit_path: &WorkspacePath,
    readable_path: &WorkspacePath,
    snapshot: &InvocationToolSnapshot,
    char_count: usize,
    inline_char_limit: usize,
) -> Result<(), ApplicationError> {
    let preview = result
        .content
        .chars()
        .take(RESULT_CONTENT_CHUNK_CHARS)
        .collect::<String>();
    let read_tool = ToolId::builtin("workspace.read_file")?;
    let read_alias = snapshot
        .binding(&read_tool)
        .map(|binding| binding.model_alias());
    let search_tool = ToolId::builtin("workspace.search_files")?;
    let search_alias = snapshot
        .binding(&search_tool)
        .map(|binding| binding.model_alias());
    let mut instructions = format!(
        "This tool result is too large to include in full ({char_count} characters). The full result is saved at `{}`.",
        readable_path.as_str(),
    );
    if let Some(alias) = read_alias {
        instructions.push_str(&format!(
            "\nUse {alias} with this path to read the full result. If the reader returns another preview, continue from the next line it indicates."
        ));
    }
    if let Some(alias) = search_alias {
        instructions.push_str(&format!(
            "\nUse {alias} on the same path to find specific text."
        ));
    }
    if !preview.is_empty() {
        instructions.push_str(&format!(
            "\n\n## Prefix preview\n\nThe following is at most {RESULT_CONTENT_CHUNK_CHARS} Unicode characters from the beginning of the result and is not the complete result.\n\n{preview}"
        ));
    }
    result.content = instructions;
    result.structured = json!({
        "externalized": true,
        "path": readable_path.as_str(),
        "auditPath": audit_path.as_str(),
        "charCount": char_count,
        "charLimit": inline_char_limit,
    });
    for path in [readable_path, audit_path] {
        if !result
            .resource_refs
            .iter()
            .any(|reference| reference == path.as_str())
        {
            result.resource_refs.push(path.as_str().to_string());
        }
    }
    Ok(())
}

fn line_addressable_content(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut line_chars = 0;
    for character in text.chars() {
        if character == '\n' {
            output.push(character);
            line_chars = 0;
            continue;
        }
        if line_chars == RESULT_CONTENT_CHUNK_CHARS {
            output.push('\n');
            line_chars = 0;
        }
        output.push(character);
        line_chars += 1;
    }
    output
}

pub(super) fn mcp_known_response_result(
    call: &ToolInvocation,
    response: McpKnownResponse,
    elapsed_ms: u128,
) -> AgentToolDispatchOutcome {
    let (result, images) = match response {
        McpKnownResponse::ToolResult(result) => {
            let content = result
                .text
                .iter()
                .map(|block| block.text.as_str())
                .collect::<Vec<_>>()
                .join("\n\n");
            let diagnostics = result
                .diagnostics
                .iter()
                .map(|diagnostic| {
                    json!({
                        "code": diagnostic.code,
                        "message": diagnostic.message,
                        "contentIndex": diagnostic.content_index,
                    })
                })
                .collect::<Vec<_>>();
            let structured = json!({
                "structuredContent": result.structured_content,
                "diagnostics": diagnostics,
            });
            let projected = if result.is_error {
                let mut projected = AgentToolResult::error(call, "mcp.tool_error", &content);
                projected.structured = structured;
                projected
            } else {
                AgentToolResult::ok(call, content, structured)
            };
            let images = decode_tool_images(result.images.into_iter().map(|image| image.data));
            (projected, images)
        }

        McpKnownResponse::ServerError(error) => {
            let mut result = AgentToolResult::error(call, "mcp.server_error", &error.message);
            result.structured = json!({
                "serverError": {
                    "code": error.code,
                    "message": error.message,
                    "data": error.data,
                }
            });
            (result, Vec::new())
        }
        McpKnownResponse::Unsupported(response) => {
            let mut result =
                AgentToolResult::error(call, "mcp.unsupported_response", &response.message);
            result.structured = json!({
                "unsupportedResponse": {
                    "type": response.response_type,
                    "message": response.message,
                }
            });
            (result, Vec::new())
        }
    };
    AgentToolDispatchOutcome::new(result, AgentToolEffect::None, elapsed_ms).with_images(images)
}

pub(super) fn tool_call_audit_file_stem(call_id: &str) -> String {
    let digest = Sha256::digest(call_id.as_bytes());
    format!(
        "call_{}",
        hex_lower(&digest[..TOOL_CALL_AUDIT_DIGEST_BYTES])
    )
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use tt_domain::models::agent::AgentToolResult;
    use tt_domain::models::tool::{ToolId, ToolProviderId};

    use super::mcp_model_content;

    #[test]
    fn mcp_model_content_keeps_actionable_structured_data() {
        let result = AgentToolResult::ok(
            &tt_domain::models::tool::ToolInvocation {
                call_id: "call_mcp".to_string(),
                tool_id: ToolId::new(
                    &ToolProviderId::parse("mcp/550e8400-e29b-41d4-a716-446655440000").unwrap(),
                    "search",
                )
                .unwrap(),
                arguments: tt_domain::models::tool::ToolArguments::empty(),
                provider_metadata: serde_json::Value::Null,
            },
            "Created issue.".to_string(),
            json!({
                "structuredContent": { "issueId": 42 },
                "diagnostics": [{
                    "code": "mcp.call_content_unsupported",
                    "message": "Image content is not supported",
                    "contentIndex": 1,
                }, {
                    "code": "mcp.call_metadata_unsupported",
                    "message": "Result metadata is not supported",
                    "contentIndex": null,
                }],
            }),
        );

        let content = mcp_model_content(&result);
        assert!(content.contains("Created issue."));
        assert!(content.contains("issueId"));
        assert!(content.contains("42"));
        assert!(content.contains("Image content is not supported"));
        assert!(!content.contains("mcp.call_content_unsupported"));
        assert!(!content.contains("Result metadata"));
    }
}
