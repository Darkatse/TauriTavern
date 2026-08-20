use std::collections::HashMap;

use serde_json::Value;
use serde_json::json;

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
/// 宿主不内嵌第三方库，skill 需要的库随 `scripts/` 自带，因此预留足够空间。
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
    session: &mut AgentToolSession,
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
    let modules = build_script_modules(skill_service, &scope, skill).await?;
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
    let workspace_snapshot = build_workspace_snapshot(
        workspace_repository,
        run_id,
        &workspace_policy.visible_roots,
    )
    .await?;
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
/// fail-fast：超过数量/字节上限、或任一模块读取失败时直接报错。
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
/// fail-fast：列表截断或任一文件读取失败时直接报错，
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

    let variables = prompt_snapshot
        .get("frozenRunInputSnapshot")
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

    Ok(json!({
        "worldInfo": { "entries": world_info_entries },
        "variables": variables,
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
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use async_trait::async_trait;
    use serde_json::{Value, json};
    use tokio::sync::Mutex;

    use super::*;
    use tt_domain::errors::{DomainError, WorkspaceWriteConflictKind};
    use tt_domain::models::agent::plan::{AgentPlanMode, AgentPlanPolicy};
    use tt_domain::models::agent::profile::{
        AGENT_PROFILE_KIND, AGENT_PROFILE_SCHEMA_VERSION, AgentContextPolicy,
        AgentDelegationPolicy, AgentModelBinding, AgentModelBindingMode, AgentPresetBinding,
        AgentPresetBindingMode, AgentProfileId, AgentProfileInstructions, AgentProfileSourceTrace,
        AgentRunPolicy, AgentSkillPolicy, AgentToolPolicy, AgentWorkspacePolicy,
        ResolvedAgentOutputPolicy,
    };
    use tt_domain::models::agent::{
        AgentChatRef, AgentRun, AgentRunPresentation, ArtifactSpec, ArtifactTarget, CommitPolicy,
        WorkspaceInputManifest, WorkspaceManifest, WorkspacePath, WorkspacePersistentChangeSet,
        WorkspaceRootCommit, WorkspaceRootLifecycle, WorkspaceRootMount, WorkspaceRootScope,
        WorkspaceRootSpec,
    };
    use tt_domain::models::skill::{
        SkillExportResult, SkillFileRef, SkillImportInput, SkillImportPreview, SkillIndexEntry,
        SkillInstallRequest, SkillInstallResult, SkillMoveRequest, SkillReadRequest,
        SkillReadResult, SkillScope, SkillScopeFilter, SkillScopeRetargetRequest,
        SkillScopeRetargetResult, SkillSearchRequest, SkillSearchResult, SkillWriteRequest,
    };
    use tt_domain::models::tool::ToolId;
    use tt_ports::repositories::skill_repository::SkillRepository;
    use tt_ports::repositories::workspace_repository::{
        WorkspaceAppendResult, WorkspaceEntry, WorkspaceEntryKind, WorkspaceFile,
        WorkspaceFileList, WorkspaceWriteGuard,
    };

    // ---- fakes ----------------------------------------------------------

    enum FakeOutcome {
        Ok(Value),
        OkWithWrites {
            value: Value,
            writes: Vec<tt_ports::skill_script::SkillScriptWrite>,
            last_write_path: Option<String>,
        },
        Failed(String),
        TooLarge {
            actual_bytes: usize,
            limit_bytes: usize,
        },
    }

    struct FakeScriptEngine {
        outcome: FakeOutcome,
        requests: Mutex<Vec<SkillScriptRequest>>,
    }

    #[async_trait]
    impl SkillScriptEngine for FakeScriptEngine {
        async fn execute(
            &self,
            request: SkillScriptRequest,
        ) -> Result<tt_ports::skill_script::SkillScriptResult, SkillScriptEngineError> {
            self.requests.lock().await.push(request);
            match &self.outcome {
                FakeOutcome::Ok(value) => Ok(tt_ports::skill_script::SkillScriptResult {
                    value: value.clone(),
                    writes: Vec::new(),
                    last_write_path: None,
                    logs: Vec::new(),
                }),
                FakeOutcome::OkWithWrites {
                    value,
                    writes,
                    last_write_path,
                } => Ok(tt_ports::skill_script::SkillScriptResult {
                    value: value.clone(),
                    writes: writes.clone(),
                    last_write_path: last_write_path.clone(),
                    logs: Vec::new(),
                }),
                FakeOutcome::Failed(message) => Err(SkillScriptEngineError::ExecutionFailed {
                    message: message.clone(),
                }),
                FakeOutcome::TooLarge {
                    actual_bytes,
                    limit_bytes,
                } => Err(SkillScriptEngineError::ResultTooLarge {
                    actual_bytes: *actual_bytes,
                    limit_bytes: *limit_bytes,
                }),
            }
        }
    }

    struct FakeSkillRepo {
        script_source: Option<String>,
    }

    #[async_trait]
    impl SkillRepository for FakeSkillRepo {
        async fn list_skills(
            &self,
            _filter: SkillScopeFilter,
        ) -> Result<Vec<SkillIndexEntry>, DomainError> {
            Ok(Vec::new())
        }
        async fn list_skill_files(
            &self,
            _scope: SkillScope,
            _name: &str,
        ) -> Result<Vec<SkillFileRef>, DomainError> {
            let mut files = vec![
                SkillFileRef {
                    path: "scripts/lib/util.js".to_string(),
                    kind: SkillFileKind::Text,
                    media_type: "text/javascript".to_string(),
                    size_bytes: 24,
                    sha256: "x".to_string(),
                },
                SkillFileRef {
                    path: "SKILL.md".to_string(),
                    kind: SkillFileKind::Text,
                    media_type: "text/markdown".to_string(),
                    size_bytes: 8,
                    sha256: "x".to_string(),
                },
            ];
            if self.script_source.is_some() {
                files.insert(
                    0,
                    SkillFileRef {
                        path: "scripts/helper.js".to_string(),
                        kind: SkillFileKind::Text,
                        media_type: "text/javascript".to_string(),
                        size_bytes: 8,
                        sha256: "x".to_string(),
                    },
                );
            }
            Ok(files)
        }
        async fn preview_import(
            &self,
            _input: SkillImportInput,
            _target: SkillScope,
        ) -> Result<SkillImportPreview, DomainError> {
            unreachable!("not needed")
        }
        async fn install_import(
            &self,
            _request: SkillInstallRequest,
        ) -> Result<SkillInstallResult, DomainError> {
            unreachable!("not needed")
        }
        async fn read_skill_script(
            &self,
            _scope: SkillScope,
            _name: &str,
            relative_path: &str,
        ) -> Result<String, DomainError> {
            match relative_path {
                "scripts/helper.js" => Ok(self
                    .script_source
                    .clone()
                    .expect("entry listed only when present")),
                "scripts/lib/util.js" => Ok("export const answer = 42;".to_string()),
                _ => Err(DomainError::NotFound(format!(
                    "Skill file not found: {relative_path}"
                ))),
            }
        }
        async fn read_skill_file(
            &self,
            _request: SkillReadRequest,
        ) -> Result<SkillReadResult, DomainError> {
            unreachable!("not needed")
        }
        async fn write_skill_file(
            &self,
            _request: SkillWriteRequest,
        ) -> Result<SkillReadResult, DomainError> {
            unreachable!("not needed")
        }
        async fn search_skill_files(
            &self,
            _request: SkillSearchRequest,
        ) -> Result<SkillSearchResult, DomainError> {
            unreachable!("not needed")
        }
        async fn export_skill(
            &self,
            _scope: SkillScope,
            _name: &str,
        ) -> Result<SkillExportResult, DomainError> {
            unreachable!("not needed")
        }
        async fn delete_skill(&self, _scope: SkillScope, _name: &str) -> Result<(), DomainError> {
            unreachable!("not needed")
        }
        async fn move_skill(
            &self,
            _request: SkillMoveRequest,
        ) -> Result<SkillInstallResult, DomainError> {
            unreachable!("not needed")
        }
        async fn retarget_scope(
            &self,
            _request: SkillScopeRetargetRequest,
        ) -> Result<SkillScopeRetargetResult, DomainError> {
            unreachable!("not needed")
        }
        async fn delete_skills_for_source(
            &self,
            _kind: &str,
            _id: &str,
        ) -> Result<Vec<String>, DomainError> {
            unreachable!("not needed")
        }
    }

    struct FakeWorkspaceRepo {
        files: HashMap<String, String>,
        written: Mutex<Vec<(String, String)>>,
        /// list_files 是否报告 truncated
        truncated: bool,
        /// 指定此路径时，write_text_guarded 返回 InternalError 模拟落盘失败
        fail_write_on: Option<String>,
        /// 快照阶段 read_text 的数据源；为 None 时退回 self.files。
        /// 用于模拟"快照后文件被外部修改"的并发场景。
        snapshot_content: Option<HashMap<String, String>>,
    }

    fn fake_sha(text: &str) -> String {
        format!("sha:{text}")
    }

    #[async_trait]
    impl WorkspaceRepository for FakeWorkspaceRepo {
        async fn initialize_run(
            &self,
            _run: &AgentRun,
            _manifest: &WorkspaceManifest,
            _prompt_snapshot: &Value,
            _resolved_profile: &ResolvedAgentProfile,
        ) -> Result<(), DomainError> {
            unreachable!("not needed")
        }
        async fn read_manifest(&self, run_id: &str) -> Result<WorkspaceManifest, DomainError> {
            Ok(WorkspaceManifest {
                workspace_version: 1,
                run_id: run_id.to_string(),
                stable_chat_id: "chat-1".to_string(),
                chat_ref: AgentChatRef::Character {
                    character_id: "character-1".to_string(),
                    file_name: "character.png".to_string(),
                },
                created_at: chrono::Utc::now(),
                input: WorkspaceInputManifest {
                    mode: "snapshot".to_string(),
                    prompt_snapshot_path: "input/prompt_snapshot.json".to_string(),
                    resolved_profile_path: "input/resolved_profile.json".to_string(),
                },
                roots: vec![WorkspaceRootSpec {
                    path: "output".to_string(),
                    lifecycle: WorkspaceRootLifecycle::Run,
                    scope: WorkspaceRootScope::Run,
                    mount: WorkspaceRootMount::Materialized,
                    visible: true,
                    writable: true,
                    commit: WorkspaceRootCommit::Never,
                }],
                artifacts: Vec::new(),
                commit_policy: CommitPolicy {
                    default_target: ArtifactTarget::MessageBody,
                    combine_template: None,
                    store_artifacts_in_extra: false,
                },
            })
        }
        async fn write_text(
            &self,
            _run_id: &str,
            _path: &WorkspacePath,
            _text: &str,
        ) -> Result<WorkspaceFile, DomainError> {
            unreachable!("not needed")
        }
        async fn write_text_guarded(
            &self,
            _run_id: &str,
            path: &WorkspacePath,
            text: &str,
            guard: WorkspaceWriteGuard,
        ) -> Result<WorkspaceFile, DomainError> {
            if self.fail_write_on.as_deref() == Some(path.as_str()) {
                return Err(DomainError::InternalError(format!(
                    "simulated write failure: {}",
                    path.as_str()
                )));
            }
            let existing = self.files.get(path.as_str());
            match guard {
                WorkspaceWriteGuard::Unchecked => {}
                WorkspaceWriteGuard::MustNotExist => {
                    if let Some(existing_text) = existing {
                        return Err(DomainError::workspace_write_conflict(
                            path.as_str(),
                            WorkspaceWriteConflictKind::AlreadyExists {
                                actual_sha256: fake_sha(existing_text),
                            },
                        ));
                    }
                }
                WorkspaceWriteGuard::MustMatchSha256(expected) => {
                    let actual = existing.map(|t| fake_sha(t));
                    if actual.as_deref() != Some(expected.as_str()) {
                        return Err(DomainError::workspace_write_conflict(
                            path.as_str(),
                            WorkspaceWriteConflictKind::Stale {
                                expected_sha256: expected,
                                actual_sha256: actual,
                            },
                        ));
                    }
                }
            }
            self.written
                .lock()
                .await
                .push((path.as_str().to_string(), text.to_string()));
            Ok(WorkspaceFile {
                path: path.clone(),
                text: text.to_string(),
                bytes: text.len() as u64,
                sha256: fake_sha(text),
            })
        }
        async fn append_text(
            &self,
            _run_id: &str,
            _path: &WorkspacePath,
            _text: &str,
        ) -> Result<WorkspaceAppendResult, DomainError> {
            unreachable!("not needed")
        }
        async fn read_text(
            &self,
            _run_id: &str,
            path: &WorkspacePath,
        ) -> Result<WorkspaceFile, DomainError> {
            let source = self.snapshot_content.as_ref().unwrap_or(&self.files);
            source
                .get(path.as_str())
                .map(|text| WorkspaceFile {
                    path: path.clone(),
                    text: text.clone(),
                    bytes: text.len() as u64,
                    sha256: fake_sha(text),
                })
                .ok_or_else(|| DomainError::NotFound(format!("File not found: {}", path.as_str())))
        }
        async fn list_files(
            &self,
            _run_id: &str,
            path: Option<&WorkspacePath>,
            _depth: usize,
            _max_entries: usize,
        ) -> Result<WorkspaceFileList, DomainError> {
            let prefix = path.map(|p| p.as_str().to_string()).unwrap_or_default();
            let source = self.snapshot_content.as_ref().unwrap_or(&self.files);
            let entries: Vec<_> = source
                .keys()
                .filter_map(|key| {
                    if prefix.is_empty() || key.starts_with(&prefix) {
                        Some(WorkspaceEntry {
                            path: WorkspacePath::parse(key).unwrap(),
                            kind: WorkspaceEntryKind::File,
                        })
                    } else {
                        None
                    }
                })
                .collect();
            Ok(WorkspaceFileList {
                entries,
                truncated: self.truncated,
            })
        }
        async fn commit_persistent_changes(
            &self,
            _run_id: &str,
        ) -> Result<WorkspacePersistentChangeSet, DomainError> {
            unreachable!("not needed")
        }
    }

    // ---- helpers --------------------------------------------------------

    fn session_with_skill(name: &str) -> AgentToolSession {
        AgentToolSession::new(vec![SkillIndexEntry {
            scope: SkillScope::Global,
            name: name.to_string(),
            description: "test".to_string(),
            display_name: None,
            source_kind: None,
            license: None,
            author: None,
            version: None,
            tags: Vec::new(),
            installed_hash: "hash".to_string(),
            file_count: 1,
            total_bytes: 1,
            has_scripts: true,
            has_binary: false,
            installed_at: chrono::Utc::now(),
            source_refs: Vec::new(),
        }])
    }

    fn base_profile() -> ResolvedAgentProfile {
        ResolvedAgentProfile {
            schema_version: AGENT_PROFILE_SCHEMA_VERSION,
            kind: AGENT_PROFILE_KIND.to_string(),
            id: AgentProfileId::parse("test-profile").expect("profile id"),
            display_name: "Test Profile".to_string(),
            description: None,
            preset: AgentPresetBinding {
                mode: AgentPresetBindingMode::CurrentPromptSnapshot,
                ref_: None,
                required: false,
            },
            model: AgentModelBinding {
                mode: AgentModelBindingMode::CurrentPromptSnapshot,
                connection_ref: None,
                model_id: None,
            },
            run: AgentRunPolicy {
                presentation: AgentRunPresentation::Background,
                direct_runnable: true,
                model_retry: Default::default(),
            },
            context: AgentContextPolicy::default(),
            delegation: AgentDelegationPolicy::default(),
            instructions: AgentProfileInstructions::default(),
            tools: AgentToolPolicy {
                allow: vec![ToolId::builtin("skill.read").unwrap()],
                deny: Vec::new(),
                tool_descriptions: Default::default(),
                max_rounds: 1,
                max_calls_per_run: 1,
                mcp_result_inline_char_limit: 50_000,
                max_calls_per_tool: Default::default(),
            },
            skills: AgentSkillPolicy {
                visible: vec!["*".to_string()],
                deny: Vec::new(),
                max_read_chars_per_call: 100_000,
                max_read_chars_per_run: 100_000,
            },
            workspace: AgentWorkspacePolicy {
                visible_roots: vec!["output".to_string()],
                writable_roots: vec!["output".to_string()],
            },
            plan: AgentPlanPolicy {
                mode: AgentPlanMode::None,
                beta: true,
                nodes: Vec::new(),
            },
            output: ResolvedAgentOutputPolicy {
                artifacts: vec![ArtifactSpec {
                    id: "main".to_string(),
                    path: "output/main.md".to_string(),
                    kind: "markdown".to_string(),
                    target: ArtifactTarget::MessageBody,
                    required: true,
                    assembly_order: 0,
                }],
                message_body_artifact_id: "main".to_string(),
                message_body_path: "output/main.md".to_string(),
            },
            source_trace: AgentProfileSourceTrace {
                profile_source: "test".to_string(),
            },
        }
    }

    fn profile(visible: bool) -> ResolvedAgentProfile {
        let mut profile = base_profile();
        if !visible {
            profile.skills.visible = Vec::new();
        }
        profile
    }

    fn call(arguments: Value) -> ToolInvocation {
        ToolInvocation {
            call_id: "call_skill_script".to_string(),
            tool_id: ToolId::builtin("skill.run_script").unwrap(),
            arguments,
            provider_metadata: Value::Null,
        }
    }

    fn empty_prompt_snapshot() -> Value {
        json!({
            "worldInfoActivation": { "entries": [] },
            "frozenRunInputSnapshot": {},
        })
    }

    async fn run_with_repo_and_outcome(
        arguments: Value,
        repo: FakeSkillRepo,
        outcome: FakeOutcome,
        session: AgentToolSession,
        profile: ResolvedAgentProfile,
    ) -> (AgentToolResult, AgentToolEffect) {
        let engine = Arc::new(FakeScriptEngine {
            outcome,
            requests: Mutex::new(Vec::new()),
        });
        let mut session = session;
        script(
            ScriptContext {
                skill_service: &SkillService::new(Arc::new(repo)),
                engine: engine.as_ref(),
                workspace_repository: &FakeWorkspaceRepo {
                    files: HashMap::new(),
                    written: Mutex::new(Vec::new()),
                    truncated: false,
                    fail_write_on: None,
                    snapshot_content: None,
                },
                run_id: "run-1",
                prompt_snapshot: empty_prompt_snapshot(),
            },
            &call(arguments),
            &mut session,
            &profile,
        )
        .await
        .expect("handler must not propagate application errors")
    }

    async fn run(
        arguments: Value,
        session: AgentToolSession,
        profile: ResolvedAgentProfile,
    ) -> (AgentToolResult, AgentToolEffect) {
        run_with_repo_and_outcome(
            arguments,
            FakeSkillRepo {
                script_source: Some("export default function() { return {}; }".to_string()),
            },
            FakeOutcome::Ok(json!({})),
            session,
            profile,
        )
        .await
    }

    async fn run_with_repo(
        arguments: Value,
        repo: FakeSkillRepo,
    ) -> (AgentToolResult, AgentToolEffect) {
        run_with_repo_and_outcome(
            arguments,
            repo,
            FakeOutcome::Ok(json!({})),
            session_with_skill("demo"),
            profile(true),
        )
        .await
    }

    async fn run_with_outcome(
        arguments: Value,
        outcome: FakeOutcome,
    ) -> (AgentToolResult, AgentToolEffect) {
        run_with_repo_and_outcome(
            arguments,
            FakeSkillRepo {
                script_source: Some("export default function() { return {}; }".to_string()),
            },
            outcome,
            session_with_skill("demo"),
            profile(true),
        )
        .await
    }

    // ---- tests ----------------------------------------------------------

    #[tokio::test]
    async fn invalid_script_name_is_rejected() {
        let (result, _) = run(
            json!({ "skill": "demo", "script": "Bad_Name" }),
            session_with_skill("demo"),
            profile(true),
        )
        .await;
        assert!(result.is_error);
        assert_eq!(
            result.error_code.as_deref(),
            Some("skill.run_script_invalid_name")
        );
        assert!(result.content.contains("SKILL.md"));
    }

    #[tokio::test]
    async fn invisible_skill_is_rejected() {
        let (result, _) = run(
            json!({ "skill": "demo", "script": "helper" }),
            session_with_skill("demo"),
            profile(false),
        )
        .await;
        assert_eq!(
            result.error_code.as_deref(),
            Some("skill.run_script_skill_not_visible")
        );
    }

    #[tokio::test]
    async fn missing_script_file_reports_not_found() {
        let (result, _) = run_with_repo(
            json!({ "skill": "demo", "script": "helper" }),
            FakeSkillRepo {
                script_source: None,
            },
        )
        .await;
        assert_eq!(
            result.error_code.as_deref(),
            Some("skill.run_script_not_found")
        );
    }

    #[tokio::test]
    async fn execution_failure_keeps_full_message() {
        let (result, _) = run_with_outcome(
            json!({ "skill": "demo", "script": "helper" }),
            FakeOutcome::Failed("TypeError: x is not a function\n    at helper.js:3:9".to_string()),
        )
        .await;
        assert_eq!(
            result.error_code.as_deref(),
            Some("skill.run_script_execution_failed")
        );
        assert!(result.content.contains("TypeError: x is not a function"));
        assert!(result.content.contains("helper.js:3:9"));
    }

    #[tokio::test]
    async fn result_too_large_maps_dedicated_code() {
        let (result, _) = run_with_outcome(
            json!({ "skill": "demo", "script": "helper" }),
            FakeOutcome::TooLarge {
                actual_bytes: 300_000,
                limit_bytes: 262_144,
            },
        )
        .await;
        assert_eq!(
            result.error_code.as_deref(),
            Some("skill.run_script_result_too_large")
        );
        assert!(result.content.contains("workspace.writeText"));
    }

    #[tokio::test]
    async fn success_builds_result_and_passes_workspace_context() {
        let engine = Arc::new(FakeScriptEngine {
            outcome: FakeOutcome::Ok(json!({ "answer": 42 })),
            requests: Mutex::new(Vec::new()),
        });
        let mut session = session_with_skill("demo");
        let mut profile = profile(true);
        profile.workspace.visible_roots = vec!["profile-only".to_string()];
        profile.workspace.writable_roots = vec!["profile-only".to_string()];

        let (result, effect) = script(
            ScriptContext {
                skill_service: &SkillService::new(Arc::new(FakeSkillRepo {
                    script_source: Some("export default function() { return {}; }".to_string()),
                })),
                engine: engine.as_ref(),
                workspace_repository: &FakeWorkspaceRepo {
                    files: HashMap::new(),
                    written: Mutex::new(Vec::new()),
                    truncated: false,
                    fail_write_on: None,
                    snapshot_content: None,
                },
                run_id: "run-1",
                prompt_snapshot: empty_prompt_snapshot(),
            },
            &call(json!({ "skill": "demo", "script": "helper", "args": { "n": 7 } })),
            &mut session,
            &profile,
        )
        .await
        .expect("script must succeed");

        let requests = engine.requests.lock().await;
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].entry_module, "scripts/helper.js");
        assert!(requests[0].modules.contains_key("scripts/helper.js"));
        assert!(requests[0].modules.contains_key("scripts/lib/util.js"));
        // SKILL.md 不在 scripts/ 下，不得进入模块快照
        assert!(!requests[0].modules.contains_key("SKILL.md"));
        assert_eq!(requests[0].args, json!({ "n": 7 }));
        // Workspace authority 来自调用级 repository manifest，而不是 Profile 副本。
        assert_eq!(requests[0].visible_roots, vec!["output".to_string()]);
        assert_eq!(requests[0].writable_roots, vec!["output".to_string()]);
        assert_eq!(
            requests[0].context,
            json!({
                "worldInfo": { "entries": [] },
                "variables": { "local": {}, "global": {} },
            })
        );

        assert!(!result.is_error);
        assert_eq!(result.structured, json!({ "answer": 42 }));
        assert!(result.content.contains("demo/scripts/helper.js"));
        assert!(matches!(effect, AgentToolEffect::None));
    }

    #[tokio::test]
    async fn module_snapshot_contains_only_script_modules() {
        let engine = Arc::new(FakeScriptEngine {
            outcome: FakeOutcome::Ok(json!({})),
            requests: Mutex::new(Vec::new()),
        });
        let mut session = session_with_skill("demo");
        let profile = profile(true);
        let (result, _) = script(
            ScriptContext {
                skill_service: &SkillService::new(Arc::new(FakeSkillRepo {
                    script_source: Some(
                        "import { answer } from './lib/util.js';\nexport default function () { return answer; }"
                            .to_string(),
                    ),
                })),
                engine: engine.as_ref(),
                workspace_repository: &FakeWorkspaceRepo {
                    files: HashMap::new(),
                    written: Mutex::new(Vec::new()),
                    truncated: false,
                    fail_write_on: None,
                    snapshot_content: None,
                },
                run_id: "run-1",
                prompt_snapshot: empty_prompt_snapshot(),
            },
            &call(json!({ "skill": "demo", "script": "helper" })),
            &mut session,
            &profile,
        )
        .await
        .expect("script must succeed");
        assert!(!result.is_error);
        let requests = engine.requests.lock().await;
        assert_eq!(
            requests[0]
                .modules
                .get("scripts/lib/util.js")
                .map(String::as_str),
            Some("export const answer = 42;")
        );
    }

    #[tokio::test]
    async fn variables_from_frozen_run_input_snapshot_are_passed_to_engine() {
        let engine = Arc::new(FakeScriptEngine {
            outcome: FakeOutcome::Ok(json!({})),
            requests: Mutex::new(Vec::new()),
        });
        let mut session = session_with_skill("demo");
        let profile = profile(true);

        let prompt_snapshot = json!({
            "worldInfoActivation": { "entries": [] },
            "frozenRunInputSnapshot": {
                "variables": {
                    "local": { "score": 42, "name": "Alice" },
                    "global": { "theme": "dark" }
                }
            }
        });

        let (result, _) = script(
            ScriptContext {
                skill_service: &SkillService::new(Arc::new(FakeSkillRepo {
                    script_source: Some("export default function() { return {}; }".to_string()),
                })),
                engine: engine.as_ref(),
                workspace_repository: &FakeWorkspaceRepo {
                    files: HashMap::new(),
                    written: Mutex::new(Vec::new()),
                    truncated: false,
                    fail_write_on: None,
                    snapshot_content: None,
                },
                run_id: "run-1",
                prompt_snapshot,
            },
            &call(json!({ "skill": "demo", "script": "helper" })),
            &mut session,
            &profile,
        )
        .await
        .expect("script must succeed");

        assert!(!result.is_error);

        let requests = engine.requests.lock().await;
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0].context["variables"]["local"].get("score"),
            Some(&json!(42))
        );
        assert_eq!(
            requests[0].context["variables"]["local"].get("name"),
            Some(&json!("Alice"))
        );
        assert_eq!(
            requests[0].context["variables"]["global"].get("theme"),
            Some(&json!("dark"))
        );
    }

    #[tokio::test]
    async fn script_writes_produce_workspace_file_written_effect() {
        let engine = Arc::new(FakeScriptEngine {
            outcome: FakeOutcome::OkWithWrites {
                value: json!({ "done": true }),
                writes: vec![tt_ports::skill_script::SkillScriptWrite {
                    path: "output/result.txt".to_string(),
                    text: "generated content".to_string(),
                }],
                last_write_path: Some("output/result.txt".to_string()),
            },
            requests: Mutex::new(Vec::new()),
        });
        let workspace_repo = FakeWorkspaceRepo {
            files: HashMap::new(),
            written: Mutex::new(Vec::new()),
            truncated: false,
            fail_write_on: None,
            snapshot_content: None,
        };
        let mut session = session_with_skill("demo");
        let profile = profile(true);

        let (result, effect) = script(
            ScriptContext {
                skill_service: &SkillService::new(Arc::new(FakeSkillRepo {
                    script_source: Some("export default function () { return 1; }".to_string()),
                })),
                engine: engine.as_ref(),
                workspace_repository: &workspace_repo,
                run_id: "run-1",
                prompt_snapshot: empty_prompt_snapshot(),
            },
            &call(json!({ "skill": "demo", "script": "helper" })),
            &mut session,
            &profile,
        )
        .await
        .expect("script must succeed");

        assert!(!result.is_error);
        assert!(matches!(
            effect,
            AgentToolEffect::WorkspaceFilesWritten { .. }
        ));
        assert_eq!(result.resource_refs, vec!["output/result.txt".to_string()]);

        let written = workspace_repo.written.lock().await;
        assert_eq!(written.len(), 1);
        assert_eq!(written[0].0, "output/result.txt");
        assert_eq!(written[0].1, "generated content");
    }

    #[tokio::test]
    async fn script_without_writes_produces_none_effect() {
        let (result, effect) = run(
            json!({ "skill": "demo", "script": "helper" }),
            session_with_skill("demo"),
            profile(true),
        )
        .await;
        assert!(!result.is_error);
        assert!(matches!(effect, AgentToolEffect::None));
    }

    #[tokio::test]
    async fn multiple_files_written_produce_batch_effect() {
        let engine = Arc::new(FakeScriptEngine {
            outcome: FakeOutcome::OkWithWrites {
                value: json!({ "done": true }),
                writes: vec![
                    tt_ports::skill_script::SkillScriptWrite {
                        path: "output/a.txt".to_string(),
                        text: "alpha".to_string(),
                    },
                    tt_ports::skill_script::SkillScriptWrite {
                        path: "output/b.txt".to_string(),
                        text: "beta".to_string(),
                    },
                ],
                // final delta 按路径排序，但真实最后一次 writeText 是 a.txt。
                last_write_path: Some("output/a.txt".to_string()),
            },
            requests: Mutex::new(Vec::new()),
        });
        let workspace_repo = FakeWorkspaceRepo {
            files: HashMap::new(),
            written: Mutex::new(Vec::new()),
            truncated: false,
            fail_write_on: None,
            snapshot_content: None,
        };
        let mut session = session_with_skill("demo");
        let profile = profile(true);

        let (result, effect) = script(
            ScriptContext {
                skill_service: &SkillService::new(Arc::new(FakeSkillRepo {
                    script_source: Some("export default function () { return 1; }".to_string()),
                })),
                engine: engine.as_ref(),
                workspace_repository: &workspace_repo,
                run_id: "run-1",
                prompt_snapshot: empty_prompt_snapshot(),
            },
            &call(json!({ "skill": "demo", "script": "helper" })),
            &mut session,
            &profile,
        )
        .await
        .expect("script must succeed");

        assert!(!result.is_error);
        match effect {
            AgentToolEffect::WorkspaceFilesWritten {
                files,
                last_text_mutation,
            } => {
                assert_eq!(files.len(), 2);
                assert_eq!(files[0].path.as_str(), "output/a.txt");
                assert_eq!(files[1].path.as_str(), "output/b.txt");
                assert_eq!(
                    last_text_mutation.as_ref().map(WorkspacePath::as_str),
                    Some("output/a.txt")
                );
                assert_eq!(
                    result.resource_refs,
                    vec!["output/a.txt".to_string(), "output/b.txt".to_string()]
                );
            }
            other => panic!("expected batch effect, got: {other:?}"),
        }
        let written = workspace_repo.written.lock().await;
        assert_eq!(written.len(), 2);
    }

    #[tokio::test]
    async fn write_outside_writable_roots_is_rejected_before_any_disk_write() {
        let engine = Arc::new(FakeScriptEngine {
            outcome: FakeOutcome::OkWithWrites {
                value: json!({}),
                writes: vec![
                    tt_ports::skill_script::SkillScriptWrite {
                        path: "output/ok.txt".to_string(),
                        text: "ok".to_string(),
                    },
                    tt_ports::skill_script::SkillScriptWrite {
                        path: "input/forbidden.txt".to_string(),
                        text: "no".to_string(),
                    },
                ],
                last_write_path: Some("input/forbidden.txt".to_string()),
            },
            requests: Mutex::new(Vec::new()),
        });
        let workspace_repo = FakeWorkspaceRepo {
            files: HashMap::new(),
            written: Mutex::new(Vec::new()),
            truncated: false,
            fail_write_on: None,
            snapshot_content: None,
        };
        let mut session = session_with_skill("demo");
        let profile = profile(true);

        let (result, effect) = script(
            ScriptContext {
                skill_service: &SkillService::new(Arc::new(FakeSkillRepo {
                    script_source: Some("export default function () { return 1; }".to_string()),
                })),
                engine: engine.as_ref(),
                workspace_repository: &workspace_repo,
                run_id: "run-1",
                prompt_snapshot: empty_prompt_snapshot(),
            },
            &call(json!({ "skill": "demo", "script": "helper" })),
            &mut session,
            &profile,
        )
        .await
        .expect("handler must not propagate application errors");

        assert!(result.is_error);
        assert_eq!(
            result.error_code.as_deref(),
            Some("skill.run_script_write_failed")
        );
        assert!(matches!(effect, AgentToolEffect::None));
        // 一次性验证：任何文件都不落盘（包括列表中合法的 output/ok.txt）
        let written = workspace_repo.written.lock().await;
        assert!(
            written.is_empty(),
            "no file may be written when any path is invalid"
        );
    }

    #[tokio::test]
    async fn existing_file_write_uses_snapshot_sha_guard() {
        // 快照时文件已存在：guard 必须是 MustMatchSha256(快照 sha)。
        // 落盘时文件内容未变 → 写入成功。
        let engine = Arc::new(FakeScriptEngine {
            outcome: FakeOutcome::OkWithWrites {
                value: json!({}),
                writes: vec![tt_ports::skill_script::SkillScriptWrite {
                    path: "output/existing.txt".to_string(),
                    text: "rewritten".to_string(),
                }],
                last_write_path: Some("output/existing.txt".to_string()),
            },
            requests: Mutex::new(Vec::new()),
        });
        let mut files = HashMap::new();
        files.insert("output/existing.txt".to_string(), "original".to_string());
        let workspace_repo = FakeWorkspaceRepo {
            files,
            written: Mutex::new(Vec::new()),
            truncated: false,
            fail_write_on: None,
            snapshot_content: None,
        };
        let mut session = session_with_skill("demo");
        let profile = profile(true);

        let (result, effect) = script(
            ScriptContext {
                skill_service: &SkillService::new(Arc::new(FakeSkillRepo {
                    script_source: Some("export default function () { return 1; }".to_string()),
                })),
                engine: engine.as_ref(),
                workspace_repository: &workspace_repo,
                run_id: "run-1",
                prompt_snapshot: empty_prompt_snapshot(),
            },
            &call(json!({ "skill": "demo", "script": "helper" })),
            &mut session,
            &profile,
        )
        .await
        .expect("script must succeed");

        assert!(!result.is_error);
        assert!(matches!(
            effect,
            AgentToolEffect::WorkspaceFilesWritten { .. }
        ));
    }

    #[tokio::test]
    async fn stale_conflict_fails_without_side_effects() {
        // 快照后文件被外部改动（磁盘 sha 与快照 sha 不符）→ MustMatchSha256 冲突，
        // 且该冲突在任何落盘前暴露：第一个文件即冲突 → 零副作用。
        let engine = Arc::new(FakeScriptEngine {
            outcome: FakeOutcome::OkWithWrites {
                value: json!({}),
                writes: vec![tt_ports::skill_script::SkillScriptWrite {
                    path: "output/stale.txt".to_string(),
                    text: "new".to_string(),
                }],
                last_write_path: Some("output/stale.txt".to_string()),
            },
            requests: Mutex::new(Vec::new()),
        });
        // 快照阶段读到 "original"，写入阶段磁盘已是 "changed-by-someone-else"
        let mut snapshot_files = HashMap::new();
        snapshot_files.insert("output/stale.txt".to_string(), "original".to_string());
        let mut disk_files = HashMap::new();
        disk_files.insert(
            "output/stale.txt".to_string(),
            "changed-by-someone-else".to_string(),
        );
        let workspace_repo = FakeWorkspaceRepo {
            files: disk_files,
            written: Mutex::new(Vec::new()),
            truncated: false,
            fail_write_on: None,
            snapshot_content: Some(snapshot_files),
        };
        let mut session = session_with_skill("demo");
        let profile = profile(true);

        let (result, effect) = script(
            ScriptContext {
                skill_service: &SkillService::new(Arc::new(FakeSkillRepo {
                    script_source: Some("export default function () { return 1; }".to_string()),
                })),
                engine: engine.as_ref(),
                workspace_repository: &workspace_repo,
                run_id: "run-1",
                prompt_snapshot: empty_prompt_snapshot(),
            },
            &call(json!({ "skill": "demo", "script": "helper" })),
            &mut session,
            &profile,
        )
        .await
        .expect("conflict must surface as tool error");

        assert!(result.is_error);
        assert_eq!(
            result.error_code.as_deref(),
            Some("skill.run_script_write_failed")
        );
        assert!(matches!(effect, AgentToolEffect::None));
        let written = workspace_repo.written.lock().await;
        assert!(written.is_empty());
    }

    #[tokio::test]
    async fn mid_batch_failure_preserves_already_written_files_in_effect() {
        // 前一个文件已成功落盘、后一个失败：调用返回 tool_error，
        // 但已写入文件保留在 effect 中——副作用不从 journal 消失。
        let engine = Arc::new(FakeScriptEngine {
            outcome: FakeOutcome::OkWithWrites {
                value: json!({}),
                writes: vec![
                    tt_ports::skill_script::SkillScriptWrite {
                        path: "output/first.txt".to_string(),
                        text: "first".to_string(),
                    },
                    tt_ports::skill_script::SkillScriptWrite {
                        path: "output/second.txt".to_string(),
                        text: "second".to_string(),
                    },
                ],
                last_write_path: Some("output/second.txt".to_string()),
            },
            requests: Mutex::new(Vec::new()),
        });
        let workspace_repo = FakeWorkspaceRepo {
            files: HashMap::new(),
            written: Mutex::new(Vec::new()),
            truncated: false,
            fail_write_on: Some("output/second.txt".to_string()),
            snapshot_content: None,
        };
        let mut session = session_with_skill("demo");
        let profile = profile(true);

        let (result, effect) = script(
            ScriptContext {
                skill_service: &SkillService::new(Arc::new(FakeSkillRepo {
                    script_source: Some("export default function () { return 1; }".to_string()),
                })),
                engine: engine.as_ref(),
                workspace_repository: &workspace_repo,
                run_id: "run-1",
                prompt_snapshot: empty_prompt_snapshot(),
            },
            &call(json!({ "skill": "demo", "script": "helper" })),
            &mut session,
            &profile,
        )
        .await
        .expect("partial failure must surface as tool error");

        assert!(result.is_error);
        assert_eq!(
            result.error_code.as_deref(),
            Some("skill.run_script_write_failed")
        );
        assert!(
            result.content.contains("output/first.txt"),
            "message was: {}",
            result.content
        );
        match effect {
            AgentToolEffect::WorkspaceFilesWritten {
                files,
                last_text_mutation,
            } => {
                assert_eq!(files.len(), 1);
                assert_eq!(files[0].path.as_str(), "output/first.txt");
                assert!(last_text_mutation.is_none());
                assert_eq!(result.resource_refs, vec!["output/first.txt".to_string()]);
            }
            other => panic!("expected partial batch effect, got: {other:?}"),
        }
        let written = workspace_repo.written.lock().await;
        assert_eq!(written.len(), 1);
        assert_eq!(written[0].0, "output/first.txt");
    }

    #[tokio::test]
    async fn truncated_workspace_snapshot_fails_fast() {
        let engine = Arc::new(FakeScriptEngine {
            outcome: FakeOutcome::Ok(json!({})),
            requests: Mutex::new(Vec::new()),
        });
        let mut session = session_with_skill("demo");
        let profile = profile(true);

        let error = script(
            ScriptContext {
                skill_service: &SkillService::new(Arc::new(FakeSkillRepo {
                    script_source: Some("export default function () { return 1; }".to_string()),
                })),
                engine: engine.as_ref(),
                workspace_repository: &FakeWorkspaceRepo {
                    files: HashMap::new(),
                    written: Mutex::new(Vec::new()),
                    truncated: true,
                    fail_write_on: None,
                    snapshot_content: None,
                },
                run_id: "run-1",
                prompt_snapshot: empty_prompt_snapshot(),
            },
            &call(json!({ "skill": "demo", "script": "helper" })),
            &mut session,
            &profile,
        )
        .await
        .expect_err("truncated snapshot must fail fast");
        assert!(
            matches!(error, ApplicationError::ValidationError(message) if message.contains("truncated"))
        );
    }

    #[test]
    fn malformed_script_context_fails_fast() {
        assert!(build_script_context_json(&json!({ "worldInfoActivation": {} })).is_err());
        assert!(
            build_script_context_json(&json!({
                "worldInfoActivation": { "entries": [] },
                "frozenRunInputSnapshot": {
                    "variables": { "local": [], "global": {} }
                },
            }))
            .is_err()
        );
    }

    #[test]
    fn script_name_validation_rules() {
        assert!(is_valid_script_name("helper"));
        assert!(is_valid_script_name("helper-2"));
        assert!(is_valid_script_name("0helper"));
        assert!(!is_valid_script_name("Helper"));
        assert!(!is_valid_script_name("bad_name"));
        assert!(!is_valid_script_name("bad/name"));
        assert!(!is_valid_script_name("bad.js"));
        assert!(!is_valid_script_name(".hidden"));
        assert!(!is_valid_script_name(""));
        assert!(!is_valid_script_name("-leading"));
        assert!(!is_valid_script_name("a..b"));
    }
}
