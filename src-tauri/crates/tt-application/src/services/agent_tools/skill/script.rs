use std::collections::HashMap;

use serde_json::{Map, Value, json};

use super::super::common::{
    ensure_only_args, object_args, required_trimmed_string_arg, tool_error,
};
use super::super::dispatcher::AgentToolEffect;
use super::super::session::AgentToolSession;
use super::super::workspace::workspace_access_policy;
use super::list::skill_is_visible;
use crate::errors::ApplicationError;
use crate::services::skill_service::SkillService;
use tt_domain::models::agent::profile::ResolvedAgentProfile;
use tt_domain::models::agent::{AgentToolResult, WorkspacePath};
use tt_domain::models::skill::{SkillFileKind, SkillFileRef, SkillScope};
use tt_domain::models::tool::ToolInvocation;
use tt_ports::repositories::workspace_repository::{
    WorkspaceEntryKind, WorkspaceFile, WorkspaceRepository, WorkspaceWriteGuard,
};
use tt_ports::skill_script::{SkillScriptEngine, SkillScriptEngineError, SkillScriptRequest};

const SKILL_SCRIPT_INVALID_NAME: &str = "skill.run_script_invalid_name";
const SKILL_SCRIPT_SKILL_NOT_VISIBLE: &str = "skill.run_script_skill_not_visible";
const SKILL_SCRIPT_NOT_FOUND: &str = "skill.run_script_not_found";
const SKILL_SCRIPT_EXECUTION_FAILED: &str = "skill.run_script_execution_failed";
const SKILL_SCRIPT_RESULT_TOO_LARGE: &str = "skill.run_script_result_too_large";
const SKILL_SCRIPT_WRITE_FAILED: &str = "skill.run_script_write_failed";

/// 单个 skill 脚本执行允许携带的最大模块数与源码总字节数（fail-fast 上限）。
/// 为多模块脚本与 Skill 自带依赖预留足够空间。
const MAX_SCRIPT_MODULES: usize = 64;
const MAX_SCRIPT_MODULE_TOTAL_BYTES: usize = 2 * 1024 * 1024;

/// skill.run_script 依赖的服务与运行上下文。
pub(in crate::services::agent_tools) struct ScriptContext<'a> {
    pub(in crate::services::agent_tools) skill_service: &'a SkillService,
    pub(in crate::services::agent_tools) engine: &'a dyn SkillScriptEngine,
    pub(in crate::services::agent_tools) workspace_repository: &'a dyn WorkspaceRepository,
    pub(in crate::services::agent_tools) run_id: &'a str,
    pub(in crate::services::agent_tools) prompt_snapshot: Value,
}

pub(in crate::services::agent_tools) async fn script(
    context: ScriptContext<'_>,
    call: &ToolInvocation,
    session: &AgentToolSession,
    profile: &ResolvedAgentProfile,
) -> Result<(AgentToolResult, AgentToolEffect), ApplicationError> {
    let ScriptContext {
        skill_service,
        engine,
        workspace_repository,
        run_id,
        prompt_snapshot,
    } = context;
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
    if let Err(message) = ensure_only_args(args, &["skill", "script", "args"]) {
        return Ok((
            tool_error(call, "tool.invalid_arguments", &message),
            AgentToolEffect::None,
        ));
    }
    let Some(skill) = required_trimmed_string_arg(args, "skill") else {
        return Ok((
            tool_error(call, "tool.invalid_arguments", "skill is required"),
            AgentToolEffect::None,
        ));
    };
    let Some(script) = required_trimmed_string_arg(args, "script") else {
        return Ok((
            tool_error(call, "tool.invalid_arguments", "script is required"),
            AgentToolEffect::None,
        ));
    };
    let script_args = match args.get("args") {
        None => Value::Object(serde_json::Map::new()),
        Some(value) if value.is_object() => value.clone(),
        Some(_) => {
            return Ok((
                tool_error(call, "tool.invalid_arguments", "args must be an object"),
                AgentToolEffect::None,
            ));
        }
    };

    if !is_valid_script_name(script) {
        return Ok((
            tool_error(
                call,
                SKILL_SCRIPT_INVALID_NAME,
                &format!(
                    "script name `{script}` is invalid: use lowercase letters, digits, and hyphens (pattern ^[a-z0-9][a-z0-9-]*$). Check the exact script name in this skill's SKILL.md."
                ),
            ),
            AgentToolEffect::None,
        ));
    }
    if !skill_is_visible(&profile.skills, skill) {
        return Ok((
            tool_error(
                call,
                SKILL_SCRIPT_SKILL_NOT_VISIBLE,
                &format!(
                    "Skill `{skill}` is not available under the current policy. Call skill_list to see available skills."
                ),
            ),
            AgentToolEffect::None,
        ));
    }
    let Some(scope) = session.effective_skill_scope(skill) else {
        return Ok((
            tool_error(
                call,
                SKILL_SCRIPT_SKILL_NOT_VISIBLE,
                &format!(
                    "Skill `{skill}` is not available in the current Skill set. Call skill_list to see available skills."
                ),
            ),
            AgentToolEffect::None,
        ));
    };

    let entry_module = format!("scripts/{script}.js");
    let modules = match build_script_modules(skill_service, &scope, skill).await {
        Ok(modules) => modules,
        Err(error) => return reject_preparation(call, error),
    };
    if !modules.contains_key(&entry_module) {
        return Ok((
            tool_error(
                call,
                SKILL_SCRIPT_NOT_FOUND,
                &format!(
                    "Script `{entry_module}` was not found in skill `{skill}`. Call skill_read on this skill's SKILL.md to check which scripts it ships."
                ),
            ),
            AgentToolEffect::None,
        ));
    }

    // invocation repository 的 manifest 是本次调用唯一的 Workspace policy。
    let workspace_policy = workspace_access_policy(workspace_repository, run_id).await?;

    // 构建工作区文件快照：列出 visible_roots 下的文件并读取内容（含 sha256）。
    let workspace_snapshot = match build_workspace_snapshot(
        workspace_repository,
        run_id,
        &workspace_policy.visible_roots,
    )
    .await
    {
        Ok(snapshot) => snapshot,
        Err(error) => return reject_preparation(call, error),
    };
    let workspace_files = workspace_snapshot
        .iter()
        .map(|(path, file)| (path.clone(), file.text.clone()))
        .collect::<HashMap<_, _>>();

    let script_context = build_script_context_json(&prompt_snapshot)?;

    tracing::info!(
        "skill.run_script invoked: skill=`{skill}` script=`{script}` args_bytes={}",
        script_args.to_string().len()
    );

    let outcome = engine
        .execute(SkillScriptRequest {
            frozen_macros: session.frozen_macros.clone(),
            entry_module: entry_module.clone(),
            modules,
            args: script_args,
            workspace_files,
            visible_roots: workspace_policy.visible_roots.clone(),
            writable_roots: workspace_policy.writable_roots.clone(),
            context: script_context,
        })
        .await;

    let result = match outcome {
        Ok(result) => result,
        Err(SkillScriptEngineError::ExecutionFailed { message }) => {
            tracing::warn!(
                "skill.run_script execution failed for skill `{skill}` script `{script}`: {message}"
            );
            return Ok((
                tool_error(call, SKILL_SCRIPT_EXECUTION_FAILED, &message),
                AgentToolEffect::None,
            ));
        }
        Err(SkillScriptEngineError::ResultTooLarge {
            actual_bytes,
            limit_bytes,
        }) => {
            tracing::warn!(
                "skill.run_script result too large for skill `{skill}` script `{script}`: {actual_bytes} bytes (limit {limit_bytes})"
            );
            return Ok((
                tool_error(
                    call,
                    SKILL_SCRIPT_RESULT_TOO_LARGE,
                    &format!(
                        "Skill script result is {actual_bytes} bytes, exceeding the {limit_bytes}-byte limit. Return less data from the script and write large output to the workspace with workspace.writeText instead."
                    ),
                ),
                AgentToolEffect::None,
            ));
        }
        Err(SkillScriptEngineError::Internal(message)) => {
            return Err(ApplicationError::InternalError(message));
        }
    };

    let last_write_path = match result.last_write_path.as_deref() {
        None if result.writes.is_empty() => None,
        Some(path) if result.writes.iter().any(|write| write.path == path) => Some(path),
        _ => {
            return Err(ApplicationError::InternalError(
                "Skill script engine returned an inconsistent final workspace delta".to_string(),
            ));
        }
    };

    // 日志属于已经完成的脚本执行，不应因随后发生的 workspace 冲突而丢失。
    for log_entry in &result.logs {
        match log_entry.level {
            tt_ports::skill_script::SkillScriptLogLevel::Info => {
                tracing::info!("[skill-script] {}", log_entry.message)
            }
            tt_ports::skill_script::SkillScriptLogLevel::Warn => {
                tracing::warn!("[skill-script] {}", log_entry.message)
            }
            tt_ports::skill_script::SkillScriptLogLevel::Error => {
                tracing::error!("[skill-script] {}", log_entry.message)
            }
            tt_ports::skill_script::SkillScriptLogLevel::Debug => {
                tracing::debug!("[skill-script] {}", log_entry.message)
            }
        }
    }

    // ---- 落盘前一次性验证所有路径 ----
    // Application 重新验证正式写入策略（不复用 adapter 内存前缀检查的结论），
    // 并按快照时的文件状态映射 CAS guard；任何路径非法都在落盘前失败。
    let mut guards: Vec<(
        &tt_ports::skill_script::SkillScriptWrite,
        WorkspacePath,
        WorkspaceWriteGuard,
    )> = Vec::with_capacity(result.writes.len());
    for write in &result.writes {
        let path = WorkspacePath::parse(&write.path).map_err(ApplicationError::from)?;
        if !workspace_policy.is_writable(&path) {
            tracing::warn!(
                "skill.run_script rejected write outside writable roots: {}",
                write.path
            );
            return Ok((
                tool_error(
                    call,
                    SKILL_SCRIPT_WRITE_FAILED,
                    &format!(
                        "Write path `{}` is outside the writable workspace roots ({}).",
                        write.path,
                        workspace_policy.writable_roots.join(", ")
                    ),
                ),
                AgentToolEffect::None,
            ));
        }
        // guard 基于快照时的文件状态：存在 → MustMatch(快照 sha)；不存在 → MustNotExist
        let guard = match workspace_snapshot.get(write.path.as_str()) {
            Some(existing) => WorkspaceWriteGuard::MustMatchSha256(existing.sha256.clone()),
            None => WorkspaceWriteGuard::MustNotExist,
        };
        guards.push((write, path, guard));
    }

    // ---- 批量落盘：最终 delta 逐文件提交；中途失败保留已发生副作用 ----
    let mut written_files: Vec<WorkspaceFile> = Vec::with_capacity(guards.len());
    for (write, path, guard) in guards {
        match workspace_repository
            .write_text_guarded(run_id, &path, &write.text, guard)
            .await
        {
            Ok(file) => {
                tracing::info!(
                    "skill.run_script wrote workspace file: {} ({} bytes)",
                    write.path,
                    write.text.len()
                );
                written_files.push(file);
            }
            Err(error) => {
                // fail-fast：停止后续写入；已写入文件保留在 effect 与错误消息中，
                // 进入 journal / 事件 / resource refs，但失败的 batch 不自动提交到聊天。
                tracing::warn!(
                    error = %error,
                    "skill.run_script write failed: {}", write.path
                );
                let already_written = written_files.len();
                let written_paths = written_files
                    .iter()
                    .map(|f| f.path.as_str().to_string())
                    .collect::<Vec<_>>();
                let written_paths_summary = written_paths.join(", ");
                let effect = if written_files.is_empty() {
                    AgentToolEffect::None
                } else {
                    AgentToolEffect::WorkspaceFilesWritten {
                        files: written_files,
                        last_text_mutation: None,
                    }
                };
                let mut result = tool_error(
                    call,
                    SKILL_SCRIPT_WRITE_FAILED,
                    &format!(
                        "Write to `{}` failed: {error}. {already_written}/{} writes were applied; already written: {written_paths_summary}. Re-read the listed files before retrying.",
                        write.path,
                        result.writes.len(),
                    ),
                );
                // 已发生副作用同样进入 resource refs（journal 事件 tool_call_completed 消费）
                result.resource_refs = written_paths;
                return Ok((result, effect));
            }
        }
    }

    let rendered = result.value.to_string();
    let content = format!("Executed skill script `{skill}/{entry_module}`. Result:\n{rendered}");

    let resource_refs = written_files
        .iter()
        .map(|file| file.path.as_str().to_string())
        .collect::<Vec<_>>();
    let last_text_mutation = last_write_path.map(WorkspacePath::parse).transpose()?;

    tracing::info!(
        "skill.run_script completed: skill=`{skill}` script=`{script}` result_bytes={} writes={} write_bytes={}",
        rendered.len(),
        result.writes.len(),
        written_files
            .iter()
            .map(|f| f.bytes as usize)
            .sum::<usize>()
    );

    let effect = if written_files.is_empty() {
        AgentToolEffect::None
    } else {
        AgentToolEffect::WorkspaceFilesWritten {
            files: written_files,
            last_text_mutation,
        }
    };

    Ok((
        AgentToolResult {
            call_id: call.call_id.clone(),
            tool_id: call.tool_id.clone(),
            content,
            structured: result.value,
            is_error: false,
            error_code: None,
            resource_refs,
        },
        effect,
    ))
}

/// 把 skill 包内 `scripts/**/*.js` 读取为内存模块快照。
/// 超过数量/字节上限、或任一模块读取失败时拒绝本次脚本调用。
async fn build_script_modules(
    skill_service: &SkillService,
    scope: &SkillScope,
    skill: &str,
) -> Result<HashMap<String, String>, ApplicationError> {
    let files = skill_service.list_skill_files(scope.clone(), skill).await?;
    let script_files: Vec<&SkillFileRef> = files
        .iter()
        .filter(|file| {
            file.kind == SkillFileKind::Text
                && file.path.starts_with("scripts/")
                && file.path.ends_with(".js")
        })
        .collect();
    if script_files.len() > MAX_SCRIPT_MODULES {
        return Err(ApplicationError::ValidationError(format!(
            "Skill `{skill}` ships {} script modules, exceeding the limit of {MAX_SCRIPT_MODULES}",
            script_files.len()
        )));
    }
    let mut modules = HashMap::new();
    let mut total_bytes = 0usize;
    for file in script_files {
        let source = skill_service
            .read_skill_script(scope.clone(), skill, &file.path)
            .await?;
        total_bytes += source.len();
        if total_bytes > MAX_SCRIPT_MODULE_TOTAL_BYTES {
            return Err(ApplicationError::ValidationError(format!(
                "Skill `{skill}` script modules total {} bytes, exceeding the limit of {MAX_SCRIPT_MODULE_TOTAL_BYTES} bytes",
                total_bytes
            )));
        }
        modules.insert(file.path.clone(), source);
    }
    Ok(modules)
}

/// 从 visible_roots 下读取所有文件，构建 `逻辑路径 → WorkspaceFile` 快照
/// （含 sha256，供写入 guard 映射使用）。
/// 列表截断或任一文件读取失败时拒绝本次脚本调用，
/// 不给脚本一个不完整却不可知的 VFS。
async fn build_workspace_snapshot(
    repo: &dyn WorkspaceRepository,
    run_id: &str,
    visible_roots: &[String],
) -> Result<HashMap<String, WorkspaceFile>, ApplicationError> {
    const MAX_DEPTH: usize = 10;
    const MAX_ENTRIES: usize = 1000;

    let mut snapshot = HashMap::new();
    for root in visible_roots {
        let root = root.trim();
        if root.is_empty() {
            continue;
        }
        let root_path = WorkspacePath::parse(root).map_err(ApplicationError::from)?;
        let listing = repo
            .list_files(run_id, Some(&root_path), MAX_DEPTH, MAX_ENTRIES)
            .await
            .map_err(ApplicationError::from)?;
        if listing.truncated {
            return Err(ApplicationError::ValidationError(format!(
                "Workspace snapshot for root `{root}` was truncated at {MAX_ENTRIES} entries; \
                 the skill script would see an incomplete workspace. \
                 Reduce the number of files in the workspace."
            )));
        }
        for entry in listing.entries {
            if entry.kind == WorkspaceEntryKind::File {
                let file = repo
                    .read_text(run_id, &entry.path)
                    .await
                    .map_err(ApplicationError::from)?;
                snapshot.insert(entry.path.as_str().to_string(), file);
            }
        }
    }
    Ok(snapshot)
}

fn reject_preparation(
    call: &ToolInvocation,
    error: ApplicationError,
) -> Result<(AgentToolResult, AgentToolEffect), ApplicationError> {
    match error {
        ApplicationError::ValidationError(message) | ApplicationError::NotFound(message) => Ok((
            tool_error(call, SKILL_SCRIPT_EXECUTION_FAILED, &message),
            AgentToolEffect::None,
        )),
        error => Err(error),
    }
}

/// 把本次 run 的宿主事实投影为引擎无关的 JSON context。
fn build_script_context_json(prompt_snapshot: &Value) -> Result<Value, ApplicationError> {
    let entries = prompt_snapshot
        .get("worldInfoActivation")
        .and_then(|batch| batch.get("entries"))
        .and_then(Value::as_array)
        .ok_or_else(|| invalid_script_context("worldInfoActivation.entries must be an array"))?;
    let world_info_entries = entries
        .iter()
        .enumerate()
        .map(|(index, entry)| super::super::world_info::normalize_entry_json(index, entry))
        .collect::<Result<Vec<_>, ApplicationError>>()?;

    let frozen = prompt_snapshot
        .get("frozenRunInputSnapshot")
        .map(|value| {
            value
                .as_object()
                .ok_or_else(|| invalid_script_context("frozenRunInputSnapshot must be an object"))
        })
        .transpose()?;
    let variables = frozen
        .and_then(|frozen| frozen.get("variables"))
        .map(|variables| {
            let variables = variables
                .as_object()
                .ok_or_else(|| invalid_script_context("variables must be an object"))?;
            let local = variables
                .get("local")
                .and_then(Value::as_object)
                .ok_or_else(|| invalid_script_context("variables.local must be an object"))?;
            let global = variables
                .get("global")
                .and_then(Value::as_object)
                .ok_or_else(|| invalid_script_context("variables.global must be an object"))?;
            Ok::<Value, ApplicationError>(json!({ "local": local, "global": global }))
        })
        .transpose()?
        .unwrap_or_else(|| json!({ "local": {}, "global": {} }));

    let empty_macro_context = Map::new();
    let macro_context = frozen
        .and_then(|frozen| frozen.get("macroContext"))
        .map(|value| {
            value
                .as_object()
                .ok_or_else(|| invalid_script_context("macroContext must be an object"))
        })
        .transpose()?
        .unwrap_or(&empty_macro_context);

    Ok(json!({
        "worldInfo": { "entries": world_info_entries },
        "variables": variables,
        "macro": macro_context,
    }))
}

fn invalid_script_context(message: &str) -> ApplicationError {
    ApplicationError::ValidationError(format!("agent.invalid_skill_script_context: {message}"))
}

fn is_valid_script_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) if first.is_ascii_lowercase() || first.is_ascii_digit() => {}
        _ => return false,
    }
    chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
}

#[cfg(test)]
mod tests;
