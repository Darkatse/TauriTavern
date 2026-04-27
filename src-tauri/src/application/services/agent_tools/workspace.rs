use serde_json::{Map, Value, json};

use super::dispatcher::AgentToolEffect;
use super::session::AgentToolSession;
use crate::application::errors::ApplicationError;
use crate::domain::errors::DomainError;
use crate::domain::models::agent::{AgentToolCall, AgentToolResult, AgentToolSpec, WorkspacePath};
use crate::domain::repositories::workspace_repository::{
    WorkspaceEntryKind, WorkspaceFileList, WorkspaceRepository,
};

pub(super) const WORKSPACE_LIST_FILES: &str = "workspace.list_files";
pub(super) const WORKSPACE_READ_FILE: &str = "workspace.read_file";
pub(super) const WORKSPACE_WRITE_FILE: &str = "workspace.write_file";
pub(super) const WORKSPACE_APPLY_PATCH: &str = "workspace.apply_patch";
pub(super) const WORKSPACE_FINISH: &str = "workspace.finish";

const MODEL_WORKSPACE_LIST_FILES: &str = "workspace_list_files";
const MODEL_WORKSPACE_READ_FILE: &str = "workspace_read_file";
const MODEL_WORKSPACE_WRITE_FILE: &str = "workspace_write_file";
const MODEL_WORKSPACE_APPLY_PATCH: &str = "workspace_apply_patch";
const MODEL_WORKSPACE_FINISH: &str = "workspace_finish";

const DEFAULT_LIST_DEPTH: usize = 2;
const MAX_LIST_DEPTH: usize = 4;
const MAX_LIST_ENTRIES: usize = 200;
const MAX_READ_BYTES: u64 = 256 * 1024;
const MAX_READ_LINES: usize = 1200;
const PHASE2B_WORKSPACE_ROOTS: &[&str] = &["output", "scratch", "plan", "summaries"];
const PHASE2B_WORKSPACE_ROOTS_FOR_MODEL: &str = "output/, scratch/, plan/, and summaries/";

#[derive(Debug, Clone, Copy)]
struct WorkspaceAccessPolicy {
    visible_roots: &'static [&'static str],
    writable_roots: &'static [&'static str],
}

impl WorkspaceAccessPolicy {
    fn phase2b_default() -> Self {
        // Path normalization remains a host safety invariant. These roots are
        // only the current product policy for model-visible workspace files.
        Self {
            visible_roots: PHASE2B_WORKSPACE_ROOTS,
            writable_roots: PHASE2B_WORKSPACE_ROOTS,
        }
    }

    fn ensure_visible(self, path: &WorkspacePath) -> Result<(), ApplicationError> {
        if self.is_visible(path) {
            return Ok(());
        }

        let value = path.as_str();
        Err(ApplicationError::PermissionDenied(format!(
            "agent.workspace_read_denied: path `{value}` is not visible in Phase 2B"
        )))
    }

    fn ensure_writable(self, path: &WorkspacePath) -> Result<(), ApplicationError> {
        if self.is_writable(path) {
            return Ok(());
        }

        let value = path.as_str();
        Err(ApplicationError::PermissionDenied(format!(
            "agent.workspace_write_denied: path `{value}` is not writable in Phase 2B"
        )))
    }

    fn is_visible(self, path: &WorkspacePath) -> bool {
        self.visible_roots
            .iter()
            .any(|root| path_matches_root_or_child(path.as_str(), root))
    }

    fn is_writable(self, path: &WorkspacePath) -> bool {
        self.writable_roots
            .iter()
            .any(|root| path_matches_child(path.as_str(), root))
    }
}

fn workspace_access_policy() -> WorkspaceAccessPolicy {
    WorkspaceAccessPolicy::phase2b_default()
}

pub(super) async fn list_files(
    workspace_repository: &dyn WorkspaceRepository,
    run_id: &str,
    call: &AgentToolCall,
) -> Result<(AgentToolResult, AgentToolEffect), ApplicationError> {
    let Some(args) = object_args(call) else {
        return Ok((
            tool_error(
                call,
                "tool.invalid_arguments",
                "arguments must be an object",
            ),
            AgentToolEffect::None,
        ));
    };
    let path = match optional_list_path_arg(args, "path") {
        Ok(path) => path,
        Err(message) => {
            return Ok((
                tool_error(call, "tool.invalid_arguments", &message),
                AgentToolEffect::None,
            ));
        }
    };
    if let Some(path) = &path {
        if let Err(result) = ensure_visible_workspace_path(call, path) {
            return Ok((result, AgentToolEffect::None));
        }
    }
    let depth = match optional_usize_arg(args, "depth") {
        Ok(depth) => depth.unwrap_or(DEFAULT_LIST_DEPTH),
        Err(message) => {
            return Ok((
                tool_error(call, "tool.invalid_arguments", &message),
                AgentToolEffect::None,
            ));
        }
    };
    if depth > MAX_LIST_DEPTH {
        return Ok((
            tool_error(
                call,
                "workspace.list_depth_too_large",
                &format!("depth must be <= {MAX_LIST_DEPTH}"),
            ),
            AgentToolEffect::None,
        ));
    }

    let list = match workspace_repository
        .list_files(run_id, path.as_ref(), depth, MAX_LIST_ENTRIES)
        .await
    {
        Ok(list) => filter_visible_entries(list),
        Err(DomainError::NotFound(message)) => {
            return Ok((
                tool_error(call, "workspace.path_not_found", &message),
                AgentToolEffect::None,
            ));
        }
        Err(error) => return Err(error.into()),
    };

    let entries = list
        .entries
        .iter()
        .map(|entry| {
            json!({
                "path": entry.path.as_str(),
                "kind": match entry.kind {
                    WorkspaceEntryKind::File => "file",
                    WorkspaceEntryKind::Directory => "directory",
                },
                "bytes": entry.bytes,
            })
        })
        .collect::<Vec<_>>();
    let content = render_file_list(&list);

    Ok((
        AgentToolResult {
            call_id: call.id.clone(),
            name: call.name.clone(),
            content,
            structured: json!({
                "entries": entries,
                "truncated": list.truncated,
            }),
            is_error: false,
            error_code: None,
            resource_refs: list
                .entries
                .iter()
                .map(|entry| entry.path.as_str().to_string())
                .collect(),
        },
        AgentToolEffect::None,
    ))
}

pub(super) async fn read_file(
    workspace_repository: &dyn WorkspaceRepository,
    run_id: &str,
    call: &AgentToolCall,
    session: &mut AgentToolSession,
) -> Result<(AgentToolResult, AgentToolEffect), ApplicationError> {
    let Some(args) = object_args(call) else {
        return Ok((
            tool_error(
                call,
                "tool.invalid_arguments",
                "arguments must be an object",
            ),
            AgentToolEffect::None,
        ));
    };
    let Some(path) = required_trimmed_string_arg(args, "path") else {
        return Ok((
            tool_error(call, "tool.invalid_arguments", "path is required"),
            AgentToolEffect::None,
        ));
    };
    let path = match parse_workspace_path(call, path) {
        Ok(path) => path,
        Err(result) => return Ok((result, AgentToolEffect::None)),
    };
    if let Err(result) = ensure_visible_workspace_path(call, &path) {
        return Ok((result, AgentToolEffect::None));
    }

    let start_line = match optional_usize_arg(args, "start_line") {
        Ok(start_line) => start_line.unwrap_or(1),
        Err(message) => {
            return Ok((
                tool_error(call, "tool.invalid_arguments", &message),
                AgentToolEffect::None,
            ));
        }
    };
    if start_line == 0 {
        return Ok((
            tool_error(
                call,
                "workspace.invalid_line_range",
                "start_line must be >= 1",
            ),
            AgentToolEffect::None,
        ));
    }
    let line_count = match optional_usize_arg(args, "line_count") {
        Ok(line_count) => line_count,
        Err(message) => {
            return Ok((
                tool_error(call, "tool.invalid_arguments", &message),
                AgentToolEffect::None,
            ));
        }
    };
    if line_count == Some(0) {
        return Ok((
            tool_error(
                call,
                "workspace.invalid_line_range",
                "line_count must be >= 1",
            ),
            AgentToolEffect::None,
        ));
    }
    if line_count.is_some_and(|value| value > MAX_READ_LINES) {
        return Ok((
            tool_error(
                call,
                "workspace.read_line_count_too_large",
                &format!("line_count must be <= {MAX_READ_LINES}"),
            ),
            AgentToolEffect::None,
        ));
    }

    let file = match workspace_repository.read_text(run_id, &path).await {
        Ok(file) => file,
        Err(DomainError::NotFound(message)) => {
            return Ok((
                tool_error(call, "workspace.file_not_found", &message),
                AgentToolEffect::None,
            ));
        }
        Err(error) => return Err(error.into()),
    };

    let lines = split_lines_for_display(&file.text);
    let total_lines = lines.len();
    if start_line > total_lines.max(1) {
        return Ok((
            tool_error(
                call,
                "workspace.invalid_line_range",
                &format!("start_line {start_line} is beyond total lines {total_lines}"),
            ),
            AgentToolEffect::None,
        ));
    }

    let full_read_requested = start_line == 1 && line_count.is_none();
    if full_read_requested && (file.bytes > MAX_READ_BYTES || total_lines > MAX_READ_LINES) {
        return Ok((
            tool_error(
                call,
                "workspace.read_too_large",
                &format!(
                    "file is too large for a full read: {} bytes, {} lines. Use start_line and line_count.",
                    file.bytes, total_lines
                ),
            ),
            AgentToolEffect::None,
        ));
    }

    let end_line = match line_count {
        Some(count) => (start_line + count - 1).min(total_lines),
        None => total_lines,
    };
    if end_line.saturating_sub(start_line) + 1 > MAX_READ_LINES {
        return Ok((
            tool_error(
                call,
                "workspace.read_line_count_too_large",
                &format!("read range must be <= {MAX_READ_LINES} lines"),
            ),
            AgentToolEffect::None,
        ));
    }

    let selected = if total_lines == 0 {
        Vec::new()
    } else {
        lines[start_line - 1..end_line].to_vec()
    };
    let full_read = start_line == 1 && (total_lines == 0 || end_line == total_lines);
    session.remember_file(&file, full_read);

    let content = format!(
        "{} lines {}-{} of {}, sha256 {}\n{}",
        file.path.as_str(),
        if total_lines == 0 { 0 } else { start_line },
        if total_lines == 0 { 0 } else { end_line },
        total_lines,
        file.sha256,
        format_lines_with_numbers(&selected, start_line),
    );

    Ok((
        AgentToolResult {
            call_id: call.id.clone(),
            name: call.name.clone(),
            content,
            structured: json!({
                "path": file.path.as_str(),
                "bytes": file.bytes,
                "sha256": file.sha256.as_str(),
                "totalLines": total_lines,
                "startLine": if total_lines == 0 { 0 } else { start_line },
                "endLine": if total_lines == 0 { 0 } else { end_line },
                "fullRead": full_read,
            }),
            is_error: false,
            error_code: None,
            resource_refs: vec![file.path.as_str().to_string()],
        },
        AgentToolEffect::None,
    ))
}

pub(super) async fn write_file(
    workspace_repository: &dyn WorkspaceRepository,
    run_id: &str,
    call: &AgentToolCall,
    session: &mut AgentToolSession,
) -> Result<(AgentToolResult, AgentToolEffect), ApplicationError> {
    let Some(args) = object_args(call) else {
        return Ok((
            tool_error(
                call,
                "tool.invalid_arguments",
                "arguments must be an object",
            ),
            AgentToolEffect::None,
        ));
    };
    let Some(path) = required_trimmed_string_arg(args, "path") else {
        return Ok((
            tool_error(call, "tool.invalid_arguments", "path is required"),
            AgentToolEffect::None,
        ));
    };
    let Some(content) = required_raw_string_arg(args, "content") else {
        return Ok((
            tool_error(call, "tool.invalid_arguments", "content is required"),
            AgentToolEffect::None,
        ));
    };

    let path = match parse_workspace_path(call, path) {
        Ok(path) => path,
        Err(result) => return Ok((result, AgentToolEffect::None)),
    };
    if let Err(result) = ensure_writable_workspace_path(call, &path) {
        return Ok((result, AgentToolEffect::None));
    }
    let file = workspace_repository
        .write_text(run_id, &path, content)
        .await?;
    session.remember_file(&file, true);

    let result = AgentToolResult {
        call_id: call.id.clone(),
        name: call.name.clone(),
        content: format!("Wrote {} bytes to {}.", file.bytes, file.path.as_str()),
        structured: json!({
            "path": file.path.as_str(),
            "bytes": file.bytes,
            "sha256": file.sha256.as_str(),
        }),
        is_error: false,
        error_code: None,
        resource_refs: vec![file.path.as_str().to_string()],
    };

    Ok((result, AgentToolEffect::WorkspaceFileWritten { file }))
}

pub(super) async fn apply_patch(
    workspace_repository: &dyn WorkspaceRepository,
    run_id: &str,
    call: &AgentToolCall,
    session: &mut AgentToolSession,
) -> Result<(AgentToolResult, AgentToolEffect), ApplicationError> {
    let Some(args) = object_args(call) else {
        return Ok((
            tool_error(
                call,
                "tool.invalid_arguments",
                "arguments must be an object",
            ),
            AgentToolEffect::None,
        ));
    };
    let Some(path) = required_trimmed_string_arg(args, "path") else {
        return Ok((
            tool_error(call, "tool.invalid_arguments", "path is required"),
            AgentToolEffect::None,
        ));
    };
    let Some(old_string) = required_raw_string_arg(args, "old_string") else {
        return Ok((
            tool_error(call, "tool.invalid_arguments", "old_string is required"),
            AgentToolEffect::None,
        ));
    };
    let Some(new_string) = required_raw_string_arg(args, "new_string") else {
        return Ok((
            tool_error(call, "tool.invalid_arguments", "new_string is required"),
            AgentToolEffect::None,
        ));
    };
    let replace_all = match optional_bool_arg(args, "replace_all") {
        Ok(replace_all) => replace_all.unwrap_or(false),
        Err(message) => {
            return Ok((
                tool_error(call, "tool.invalid_arguments", &message),
                AgentToolEffect::None,
            ));
        }
    };

    if old_string.is_empty() {
        return Ok((
            tool_error(
                call,
                "workspace.patch_empty_old_string",
                "old_string cannot be empty",
            ),
            AgentToolEffect::None,
        ));
    }
    if old_string == new_string {
        return Ok((
            tool_error(
                call,
                "workspace.patch_no_change",
                "old_string and new_string are identical",
            ),
            AgentToolEffect::None,
        ));
    }

    let path = match parse_workspace_path(call, path) {
        Ok(path) => path,
        Err(result) => return Ok((result, AgentToolEffect::None)),
    };
    if let Err(result) = ensure_writable_workspace_path(call, &path) {
        return Ok((result, AgentToolEffect::None));
    }
    let path_key = path.as_str().to_string();
    let Some(read_state) = session.read_state(&path_key) else {
        return Ok((
            tool_error(
                call,
                "workspace.patch_requires_read",
                "file must be read with workspace_read_file before applying a patch",
            ),
            AgentToolEffect::None,
        ));
    };
    if !read_state.full_read {
        return Ok((
            tool_error(
                call,
                "workspace.patch_requires_full_read",
                "file must be fully read before applying a patch",
            ),
            AgentToolEffect::None,
        ));
    }

    let file = match workspace_repository.read_text(run_id, &path).await {
        Ok(file) => file,
        Err(DomainError::NotFound(message)) => {
            return Ok((
                tool_error(call, "workspace.file_not_found", &message),
                AgentToolEffect::None,
            ));
        }
        Err(error) => return Err(error.into()),
    };
    if file.sha256 != read_state.sha256 {
        return Ok((
            tool_error(
                call,
                "workspace.patch_stale_file",
                &format!(
                    "file changed since last full read: previous sha256 {}, current sha256 {}",
                    read_state.sha256, file.sha256
                ),
            ),
            AgentToolEffect::None,
        ));
    }

    let matches = file.text.matches(old_string).count();
    if matches == 0 {
        return Ok((
            tool_error(
                call,
                "workspace.patch_old_string_not_found",
                "old_string was not found in the file",
            ),
            AgentToolEffect::None,
        ));
    }
    if matches > 1 && !replace_all {
        return Ok((
            tool_error(
                call,
                "workspace.patch_old_string_not_unique",
                &format!(
                    "old_string matched {matches} times; provide more context or set replace_all=true"
                ),
            ),
            AgentToolEffect::None,
        ));
    }

    let updated = if replace_all {
        file.text.replace(old_string, new_string)
    } else {
        file.text.replacen(old_string, new_string, 1)
    };
    let old_sha256 = file.sha256.clone();
    let file = workspace_repository
        .write_text(run_id, &path, &updated)
        .await?;
    session.remember_file(&file, true);

    let result = AgentToolResult {
        call_id: call.id.clone(),
        name: call.name.clone(),
        content: format!(
            "Patched {} with {} replacement(s).",
            file.path.as_str(),
            matches
        ),
        structured: json!({
            "path": file.path.as_str(),
            "bytes": file.bytes,
            "oldSha256": old_sha256,
            "sha256": file.sha256.as_str(),
            "replacements": matches,
        }),
        is_error: false,
        error_code: None,
        resource_refs: vec![file.path.as_str().to_string()],
    };

    Ok((
        result,
        AgentToolEffect::WorkspaceFilePatched {
            file,
            replacements: matches,
            old_sha256,
        },
    ))
}

pub(super) fn finish(
    call: &AgentToolCall,
) -> Result<(AgentToolResult, AgentToolEffect), ApplicationError> {
    let args = call.arguments.as_object();
    let final_path = args
        .and_then(|args| required_trimmed_string_arg(args, "final_path"))
        .unwrap_or("output/main.md");
    let final_path = match parse_workspace_path(call, final_path) {
        Ok(path) => path,
        Err(result) => return Ok((result, AgentToolEffect::None)),
    };

    let result = AgentToolResult {
        call_id: call.id.clone(),
        name: call.name.clone(),
        content: format!("Finished with final artifact {}.", final_path.as_str()),
        structured: json!({
            "finalPath": final_path.as_str(),
            "reason": args.and_then(|args| required_trimmed_string_arg(args, "reason")),
        }),
        is_error: false,
        error_code: None,
        resource_refs: vec![final_path.as_str().to_string()],
    };

    Ok((result, AgentToolEffect::Finish { final_path }))
}

pub(super) fn workspace_list_files_spec() -> AgentToolSpec {
    AgentToolSpec {
        name: WORKSPACE_LIST_FILES.to_string(),
        model_name: MODEL_WORKSPACE_LIST_FILES.to_string(),
        title: "Workspace List Files".to_string(),
        description: format!(
            "List visible Agent workspace files under {PHASE2B_WORKSPACE_ROOTS_FOR_MODEL}. Use this before reading when you need to inspect available artifacts."
        ),
        input_schema: json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Optional relative workspace directory or file path. Omit to list the visible workspace roots."
                },
                "depth": {
                    "type": "integer",
                    "description": "Directory depth to list. Defaults to 2; maximum is 4."
                }
            }
        }),
        output_schema: None,
        annotations: json!({ "readOnly": true }),
        source: "builtin".to_string(),
    }
}

pub(super) fn workspace_read_file_spec() -> AgentToolSpec {
    AgentToolSpec {
        name: WORKSPACE_READ_FILE.to_string(),
        model_name: MODEL_WORKSPACE_READ_FILE.to_string(),
        title: "Workspace Read File".to_string(),
        description: "Read a visible UTF-8 Agent workspace file with line numbers. Fully read a file before using workspace_apply_patch on it; partial reads are only for inspection.".to_string(),
        input_schema: json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "path": {
                    "type": "string",
                    "description": format!("Relative workspace file path under {PHASE2B_WORKSPACE_ROOTS_FOR_MODEL}.")
                },
                "start_line": {
                    "type": "integer",
                    "description": "1-based starting line. Omit for a full read."
                },
                "line_count": {
                    "type": "integer",
                    "description": "Number of lines to read. Omit for a full read."
                }
            },
            "required": ["path"]
        }),
        output_schema: None,
        annotations: json!({ "readOnly": true }),
        source: "builtin".to_string(),
    }
}

pub(super) fn workspace_write_file_spec() -> AgentToolSpec {
    AgentToolSpec {
        name: WORKSPACE_WRITE_FILE.to_string(),
        model_name: MODEL_WORKSPACE_WRITE_FILE.to_string(),
        title: "Workspace Write File".to_string(),
        description: "Write complete UTF-8 text to a writable Agent workspace file. Use output/main.md for the final chat message body, then call workspace_finish.".to_string(),
        input_schema: json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "path": {
                    "type": "string",
                    "description": format!("Relative workspace path. Writable prefixes are {PHASE2B_WORKSPACE_ROOTS_FOR_MODEL}.")
                },
                "content": {
                    "type": "string",
                    "description": "Complete UTF-8 file content."
                }
            },
            "required": ["path", "content"]
        }),
        output_schema: None,
        annotations: json!({ "mutating": true }),
        source: "builtin".to_string(),
    }
}

pub(super) fn workspace_apply_patch_spec() -> AgentToolSpec {
    AgentToolSpec {
        name: WORKSPACE_APPLY_PATCH.to_string(),
        model_name: MODEL_WORKSPACE_APPLY_PATCH.to_string(),
        title: "Workspace Apply Patch".to_string(),
        description: "Apply a precise single-file string replacement. The file must have been fully read with workspace_read_file or created by workspace_write_file in this run. old_string must match exactly and uniquely unless replace_all is true.".to_string(),
        input_schema: json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "path": {
                    "type": "string",
                    "description": format!("Relative writable workspace file path under {PHASE2B_WORKSPACE_ROOTS_FOR_MODEL}.")
                },
                "old_string": {
                    "type": "string",
                    "description": "Exact text to replace. Do not include line number prefixes from read output."
                },
                "new_string": {
                    "type": "string",
                    "description": "Replacement text."
                },
                "replace_all": {
                    "type": "boolean",
                    "description": "Replace every occurrence of old_string. Defaults to false."
                }
            },
            "required": ["path", "old_string", "new_string"]
        }),
        output_schema: None,
        annotations: json!({ "mutating": true }),
        source: "builtin".to_string(),
    }
}

pub(super) fn workspace_finish_spec() -> AgentToolSpec {
    AgentToolSpec {
        name: WORKSPACE_FINISH.to_string(),
        model_name: MODEL_WORKSPACE_FINISH.to_string(),
        title: "Workspace Finish".to_string(),
        description: "Finish the Agent loop after the final artifact has been written. The default final_path is output/main.md.".to_string(),
        input_schema: json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "final_path": {
                    "type": "string",
                    "description": "Relative workspace path for the final artifact. Defaults to output/main.md."
                },
                "reason": {
                    "type": "string",
                    "description": "Short completion reason."
                }
            }
        }),
        output_schema: None,
        annotations: json!({ "control": true }),
        source: "builtin".to_string(),
    }
}

fn filter_visible_entries(list: WorkspaceFileList) -> WorkspaceFileList {
    let policy = workspace_access_policy();
    WorkspaceFileList {
        truncated: list.truncated,
        entries: list
            .entries
            .into_iter()
            .filter(|entry| policy.is_visible(&entry.path))
            .collect(),
    }
}

fn path_matches_root_or_child(path: &str, root: &str) -> bool {
    path == root || path_matches_child(path, root)
}

fn path_matches_child(path: &str, root: &str) -> bool {
    path.len() > root.len()
        && path.starts_with(root)
        && path.as_bytes().get(root.len()) == Some(&b'/')
}

fn object_args(call: &AgentToolCall) -> Option<&Map<String, Value>> {
    call.arguments.as_object()
}

fn optional_list_path_arg(
    args: &Map<String, Value>,
    key: &str,
) -> Result<Option<WorkspacePath>, String> {
    let Some(value) = args.get(key) else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }

    let Some(raw) = value.as_str() else {
        return Err(format!("{key} must be a string"));
    };
    let value = raw.trim();
    if value.is_empty() || value == "." || value == "./" {
        return Ok(None);
    }

    WorkspacePath::parse(value)
        .map(Some)
        .map_err(|error| error.to_string())
}

fn parse_workspace_path(call: &AgentToolCall, raw: &str) -> Result<WorkspacePath, AgentToolResult> {
    WorkspacePath::parse(raw)
        .map_err(|error| tool_error(call, "workspace.invalid_path", &error.to_string()))
}

fn ensure_visible_workspace_path(
    call: &AgentToolCall,
    path: &WorkspacePath,
) -> Result<(), AgentToolResult> {
    workspace_access_policy()
        .ensure_visible(path)
        .map_err(|error| tool_error(call, "workspace.path_not_visible", &error.to_string()))
}

fn ensure_writable_workspace_path(
    call: &AgentToolCall,
    path: &WorkspacePath,
) -> Result<(), AgentToolResult> {
    workspace_access_policy()
        .ensure_writable(path)
        .map_err(|error| tool_error(call, "workspace.path_not_writable", &error.to_string()))
}

fn required_trimmed_string_arg<'a>(args: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn required_raw_string_arg<'a>(args: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}

fn optional_usize_arg(args: &Map<String, Value>, key: &str) -> Result<Option<usize>, String> {
    let Some(value) = args.get(key) else {
        return Ok(None);
    };
    let Some(value) = value.as_u64() else {
        return Err(format!("{key} must be a non-negative integer"));
    };
    usize::try_from(value)
        .map(Some)
        .map_err(|_| format!("{key} is too large"))
}

fn optional_bool_arg(args: &Map<String, Value>, key: &str) -> Result<Option<bool>, String> {
    let Some(value) = args.get(key) else {
        return Ok(None);
    };
    value
        .as_bool()
        .map(Some)
        .ok_or_else(|| format!("{key} must be a boolean"))
}

fn split_lines_for_display(text: &str) -> Vec<&str> {
    if text.is_empty() {
        return Vec::new();
    }
    text.split('\n').collect()
}

fn format_lines_with_numbers(lines: &[&str], start_line: usize) -> String {
    if lines.is_empty() {
        return String::new();
    }
    let last_line = start_line + lines.len() - 1;
    let width = last_line.to_string().len();
    lines
        .iter()
        .enumerate()
        .map(|(index, line)| format!("{:>width$} | {}", start_line + index, line, width = width))
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_file_list(list: &WorkspaceFileList) -> String {
    if list.entries.is_empty() {
        return "No visible workspace files found.".to_string();
    }

    let mut lines = list
        .entries
        .iter()
        .map(|entry| match entry.kind {
            WorkspaceEntryKind::Directory => format!("{}/", entry.path.as_str()),
            WorkspaceEntryKind::File => {
                format!(
                    "{} ({} bytes)",
                    entry.path.as_str(),
                    entry.bytes.unwrap_or(0)
                )
            }
        })
        .collect::<Vec<_>>();
    if list.truncated {
        lines.push(format!("... truncated at {MAX_LIST_ENTRIES} entries"));
    }
    lines.join("\n")
}

fn tool_error(call: &AgentToolCall, error_code: &str, message: &str) -> AgentToolResult {
    AgentToolResult {
        call_id: call.id.clone(),
        name: call.name.clone(),
        content: message.to_string(),
        structured: json!({
            "error": {
                "code": error_code,
                "message": message,
            }
        }),
        is_error: true,
        error_code: Some(error_code.to_string()),
        resource_refs: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writable_policy_rejects_input_paths() {
        let path = WorkspacePath::parse("input/prompt_snapshot.json").unwrap();
        assert!(workspace_access_policy().ensure_writable(&path).is_err());
    }

    #[test]
    fn visible_policy_allows_workspace_artifact_roots() {
        for value in [
            "output",
            "scratch/file.md",
            "plan/outline.md",
            "summaries/a.md",
        ] {
            let path = WorkspacePath::parse(value).unwrap();
            assert!(workspace_access_policy().ensure_visible(&path).is_ok());
        }
    }

    #[test]
    fn writable_policy_requires_child_path() {
        let root = WorkspacePath::parse("output").unwrap();
        let file = WorkspacePath::parse("output/main.md").unwrap();

        assert!(workspace_access_policy().ensure_writable(&root).is_err());
        assert!(workspace_access_policy().ensure_writable(&file).is_ok());
    }

    #[test]
    fn list_path_arg_treats_empty_and_dot_as_workspace_root() {
        for value in ["", " ", ".", "./"] {
            let args = json!({ "path": value });
            assert!(
                optional_list_path_arg(args.as_object().unwrap(), "path")
                    .unwrap()
                    .is_none()
            );
        }
    }
}
