use std::collections::HashMap;

use serde_json::Value;
use serde_json::json;

use super::super::common::{
    ensure_only_args, object_args, required_trimmed_string_arg, tool_error,
};
use super::super::dispatcher::AgentToolEffect;
use super::super::session::AgentToolSession;
use super::list::skill_is_visible;
use crate::errors::ApplicationError;
use crate::services::skill_service::SkillService;
use tt_domain::errors::DomainError;
use tt_domain::models::agent::profile::ResolvedAgentProfile;
use tt_domain::models::agent::{AgentToolResult, WorkspaceFileWriteMode, WorkspacePath};
use tt_domain::models::skill_script::{ActivatedWorldInfoEntry, SillyTavernVariableSnapshot};
use tt_domain::models::tool::ToolInvocation;
use tt_ports::repositories::workspace_repository::{WorkspaceRepository, WorkspaceWriteGuard};
use tt_ports::skill_script::{SkillScriptEngine, SkillScriptRequest};

const SKILL_SCRIPT_INVALID_NAME: &str = "skill.run_script_invalid_name";
const SKILL_SCRIPT_SKILL_NOT_VISIBLE: &str = "skill.run_script_skill_not_visible";
const SKILL_SCRIPT_NOT_FOUND: &str = "skill.run_script_not_found";
const SKILL_SCRIPT_EXECUTION_FAILED: &str = "skill.run_script_execution_failed";
const SKILL_SCRIPT_RESULT_TOO_LARGE: &str = "skill.run_script_result_too_large";

/// skill.run_script 依赖的服务与运行上下文。
pub struct ScriptContext<'a> {
    pub skill_service: &'a SkillService,
    pub engine: &'a dyn SkillScriptEngine,
    pub workspace_repository: &'a dyn WorkspaceRepository,
    pub run_id: &'a str,
    pub prompt_snapshot: Option<&'a Value>,
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
            tool_error(call, "tool.invalid_arguments", "arguments must be an object"),
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

    let relative_path = format!("scripts/{script}.js");
    let script_source = match skill_service
        .read_skill_script(scope, skill, &relative_path)
        .await
    {
        Ok(source) => source,
        Err(ApplicationError::NotFound(_)) | Err(ApplicationError::ValidationError(_)) => {
            return Ok((
                tool_error(
                    call,
                    SKILL_SCRIPT_NOT_FOUND,
                    &format!(
                        "Script `{relative_path}` was not found in skill `{skill}`. Call skill_read on this skill's SKILL.md to check which scripts it ships."
                    ),
                ),
                AgentToolEffect::None,
            ));
        }
        Err(error) => return Err(error),
    };

    // 构建工作区文件快照：列出 visible_roots 下的文件并读取内容。
    let workspace_files = build_workspace_snapshot(
        workspace_repository,
        run_id,
        &profile.workspace.visible_roots,
    )
    .await?;

    // 投影世界书快照为纯 JSON
    let world_info = build_world_info_json(prompt_snapshot);

    // 投影变量快照为纯 JSON
    let variables = build_variables_json(prompt_snapshot);

    let script_name = format!("{skill}/scripts/{script}.js");

    tracing::info!(
        "skill.run_script invoked: skill=`{skill}` script=`{script}` args={}",
        serde_json::to_string(&script_args).unwrap_or_else(|_| "<unserializable>".to_string())
    );

    let outcome = engine
        .execute(SkillScriptRequest {
            script_source,
            script_name: script_name.clone(),
            args: script_args,
            workspace_files,
            visible_roots: profile.workspace.visible_roots.clone(),
            writable_roots: profile.workspace.writable_roots.clone(),
            world_info,
            variables,
        })
        .await;

    let result = match outcome {
        Ok(result) => result,
        Err(DomainError::SkillScriptExecutionFailed { message }) => {
            tracing::warn!(
                "skill.run_script execution failed for skill `{skill}` script `{script}`: {message}"
            );
            return Ok((
                tool_error(call, SKILL_SCRIPT_EXECUTION_FAILED, &message),
                AgentToolEffect::None,
            ));
        }
        Err(DomainError::SkillScriptResultTooLarge {
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
                        "Skill script result is {actual_bytes} bytes, exceeding the {limit_bytes}-byte limit. Return less data from the script and write large output to the workspace with $fs.writeText instead."
                    ),
                ),
                AgentToolEffect::None,
            ));
        }
        Err(error) => return Err(ApplicationError::from(error)),
    };

    // 应用 delta 写入：通过 write_text_guarded 落盘
    let mut last_written_file: Option<tt_ports::repositories::workspace_repository::WorkspaceFile> = None;
    for write in &result.writes {
        let path = WorkspacePath::parse(&write.path).map_err(ApplicationError::from)?;
        let file = workspace_repository
            .write_text_guarded(run_id, &path, &write.text, WorkspaceWriteGuard::Unchecked)
            .await
            .map_err(ApplicationError::from)?;
        tracing::info!(
            "skill.run_script wrote workspace file: {} ({} bytes)",
            write.path,
            write.text.len()
        );
        last_written_file = Some(file);
    }

    // 输出日志
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

    let rendered = serde_json::to_string(&result.value).unwrap_or_else(|_| result.value.to_string());
    let content = format!("Executed skill script `{script_name}`. Result:\n{rendered}");

    tracing::info!(
        "skill.run_script completed: skill=`{skill}` script=`{script}` result_bytes={} writes={}",
        rendered.len(),
        result.writes.len()
    );

    let effect = match last_written_file {
        Some(file) => AgentToolEffect::WorkspaceFileWritten {
            file,
            mode: WorkspaceFileWriteMode::Replace,
        },
        None => AgentToolEffect::None,
    };

    Ok((
        AgentToolResult {
            call_id: call.call_id.clone(),
            tool_id: call.tool_id.clone(),
            content,
            structured: result.value,
            is_error: false,
            error_code: None,
            resource_refs: Vec::new(),
        },
        effect,
    ))
}

/// 从 visible_roots 下读取所有文件，构建 `逻辑路径 → 文本内容` 快照。
async fn build_workspace_snapshot(
    repo: &dyn WorkspaceRepository,
    run_id: &str,
    visible_roots: &[String],
) -> Result<HashMap<String, String>, ApplicationError> {
    const MAX_DEPTH: usize = 10;
    const MAX_ENTRIES: usize = 1000;

    let mut snapshot = HashMap::new();
    for root in visible_roots {
        let root = root.trim();
        if root.is_empty() {
            continue;
        }
        let root_path = WorkspacePath::parse(root).map_err(ApplicationError::from)?;
        // 列出该根目录下的文件（深度足够覆盖常见场景）
        let listing = repo
            .list_files(run_id, Some(&root_path), MAX_DEPTH, MAX_ENTRIES)
            .await
            .map_err(ApplicationError::from)?;
        for entry in listing.entries {
            if entry.kind == tt_ports::repositories::workspace_repository::WorkspaceEntryKind::File
            {
                match repo.read_text(run_id, &entry.path).await {
                    Ok(file) => {
                        snapshot.insert(entry.path.as_str().to_string(), file.text);
                    }
                    Err(error) => {
                        tracing::warn!(
                            error = %error,
                            path = entry.path.as_str(),
                            "Failed to read workspace file for script snapshot; skipping"
                        );
                    }
                }
            }
        }
    }
    Ok(snapshot)
}

/// 从 prompt_snapshot 投影世界书条目为纯 JSON `{ "entries": [...] }`。
fn build_world_info_json(prompt_snapshot: Option<&Value>) -> Value {
    let entries = prompt_snapshot
        .and_then(|snapshot| snapshot.get("worldInfoActivation"))
        .and_then(|batch| batch.get("entries"))
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .enumerate()
                .filter_map(|(index, entry)| ActivatedWorldInfoEntry::from_value(index, entry))
                .map(|e| {
                    json!({
                        "uid": e.uid,
                        "ref": e.ref_key,
                        "content": e.content,
                        "constant": e.constant,
                        "world": e.world,
                        "position": e.position,
                        "displayName": e.display_name,
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    json!({ "entries": entries })
}

/// 从 prompt_snapshot 投影变量快照为纯 JSON `{ "local": { ... }, "global": { ... } }`。
fn build_variables_json(prompt_snapshot: Option<&Value>) -> Value {
    let snapshot = prompt_snapshot
        .and_then(|snapshot| snapshot.get("frozenRunInputSnapshot"))
        .and_then(|frozen| frozen.get("variables"))
        .map(SillyTavernVariableSnapshot::from_value)
        .unwrap_or_default();
    json!({
        "local": snapshot.local,
        "global": snapshot.global,
    })
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
    use tt_domain::models::agent::profile::{
        AGENT_PROFILE_KIND, AGENT_PROFILE_SCHEMA_VERSION, AgentContextPolicy,
        AgentDelegationPolicy, AgentModelBinding, AgentModelBindingMode, AgentPresetBinding,
        AgentPresetBindingMode, AgentProfileId, AgentProfileInstructions,
        AgentProfileSourceTrace, AgentRunPolicy, AgentSkillPolicy, AgentToolPolicy,
        AgentWorkspacePolicy, ResolvedAgentOutputPolicy,
    };
    use tt_domain::models::agent::{
        AgentRun, AgentRunPresentation, ArtifactSpec, ArtifactTarget, WorkspaceManifest,
        WorkspacePersistentChangeSet, WorkspacePath,
    };
    use tt_domain::models::agent::plan::{AgentPlanMode, AgentPlanPolicy};
    use tt_domain::models::skill::{
        SkillExportResult, SkillFileRef, SkillImportInput, SkillImportPreview, SkillIndexEntry,
        SkillInstallRequest, SkillInstallResult, SkillMoveRequest, SkillReadRequest,
        SkillReadResult, SkillScope, SkillScopeFilter, SkillScopeRetargetRequest,
        SkillScopeRetargetResult, SkillSearchRequest, SkillSearchResult, SkillWriteRequest,
    };
    use tt_domain::models::tool::ToolId;
    use tt_ports::repositories::skill_repository::SkillRepository;
    use tt_ports::repositories::workspace_repository::{
        WorkspaceAppendResult, WorkspaceEntry, WorkspaceEntryKind, WorkspaceFile, WorkspaceFileList,
        WorkspaceWriteGuard,
    };

    // ---- fakes ----------------------------------------------------------

    enum FakeOutcome {
        Ok(Value),
        OkWithWrites { value: Value, writes: Vec<tt_ports::skill_script::SkillScriptWrite> },
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
        ) -> Result<tt_ports::skill_script::SkillScriptResult, DomainError> {
            self.requests.lock().await.push(request);
            match &self.outcome {
                FakeOutcome::Ok(value) => Ok(tt_ports::skill_script::SkillScriptResult {
                    value: value.clone(),
                    writes: Vec::new(),
                    logs: Vec::new(),
                }),
                FakeOutcome::OkWithWrites { value, writes } => {
                    Ok(tt_ports::skill_script::SkillScriptResult {
                        value: value.clone(),
                        writes: writes.clone(),
                        logs: Vec::new(),
                    })
                }
                FakeOutcome::Failed(message) => {
                    Err(DomainError::skill_script_execution_failed(message.clone()))
                }
                FakeOutcome::TooLarge {
                    actual_bytes,
                    limit_bytes,
                } => Err(DomainError::SkillScriptResultTooLarge {
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
            unreachable!("not needed")
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
            _relative_path: &str,
        ) -> Result<String, DomainError> {
            match &self.script_source {
                Some(source) => Ok(source.clone()),
                None => Err(DomainError::NotFound("Skill file not found".to_string())),
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
        async fn read_manifest(&self, _run_id: &str) -> Result<WorkspaceManifest, DomainError> {
            unreachable!("not needed")
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
            _guard: WorkspaceWriteGuard,
        ) -> Result<WorkspaceFile, DomainError> {
            self.written
                .lock()
                .await
                .push((path.as_str().to_string(), text.to_string()));
            Ok(WorkspaceFile {
                path: path.clone(),
                text: text.to_string(),
                bytes: text.len() as u64,
                sha256: "fake-sha".to_string(),
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
            self.files
                .get(path.as_str())
                .map(|text| WorkspaceFile {
                    path: path.clone(),
                    text: text.clone(),
                    bytes: text.len() as u64,
                    sha256: "fake-sha".to_string(),
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
            let entries: Vec<_> = self
                .files
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
                truncated: false,
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
                },
                run_id: "run-1",
                prompt_snapshot: None,
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

    async fn run_with_repo(arguments: Value, repo: FakeSkillRepo) -> (AgentToolResult, AgentToolEffect) {
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
            FakeSkillRepo { script_source: None },
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
        assert!(result.content.contains("$fs.writeText"));
    }

    #[tokio::test]
    async fn success_builds_result_and_passes_workspace_context() {
        let engine = Arc::new(FakeScriptEngine {
            outcome: FakeOutcome::Ok(json!({ "answer": 42 })),
            requests: Mutex::new(Vec::new()),
        });
        let mut session = session_with_skill("demo");
        let profile = profile(true);

        let (result, effect) = script(
            ScriptContext {
                skill_service: &SkillService::new(Arc::new(FakeSkillRepo {
                    script_source: Some("export default function() { return {}; }".to_string()),
                })),
                engine: engine.as_ref(),
                workspace_repository: &FakeWorkspaceRepo {
                    files: HashMap::new(),
                    written: Mutex::new(Vec::new()),
                },
                run_id: "run-1",
                prompt_snapshot: None,
            },
            &call(json!({ "skill": "demo", "script": "helper", "args": { "n": 7 } })),
            &mut session,
            &profile,
        )
        .await
        .expect("script must succeed");

        let requests = engine.requests.lock().await;
        assert_eq!(requests.len(), 1);
        assert!(requests[0]
            .script_source
            .contains("export default"));
        assert_eq!(requests[0].args, json!({ "n": 7 }));
        assert_eq!(requests[0].visible_roots, vec!["output".to_string()]);
        assert_eq!(requests[0].writable_roots, vec!["output".to_string()]);
        assert_eq!(requests[0].world_info, json!({ "entries": [] }));
        assert_eq!(requests[0].variables, json!({ "local": {}, "global": {} }));

        assert!(!result.is_error);
        assert_eq!(result.structured, json!({ "answer": 42 }));
        assert!(result.content.contains("demo/scripts/helper.js"));
        assert!(matches!(effect, AgentToolEffect::None));
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
                },
                run_id: "run-1",
                prompt_snapshot: Some(&prompt_snapshot),
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
            requests[0].variables.get("local").unwrap().get("score"),
            Some(&json!(42))
        );
        assert_eq!(
            requests[0].variables.get("local").unwrap().get("name"),
            Some(&json!("Alice"))
        );
        assert_eq!(
            requests[0].variables.get("global").unwrap().get("theme"),
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
            },
            requests: Mutex::new(Vec::new()),
        });
        let workspace_repo = FakeWorkspaceRepo {
            files: HashMap::new(),
            written: Mutex::new(Vec::new()),
        };
        let mut session = session_with_skill("demo");
        let profile = profile(true);

        let (result, effect) = script(
            ScriptContext {
                skill_service: &SkillService::new(Arc::new(FakeSkillRepo {
                    script_source: Some("export default function() { return 1; }".to_string()),
                })),
                engine: engine.as_ref(),
                workspace_repository: &workspace_repo,
                run_id: "run-1",
                prompt_snapshot: None,
            },
            &call(json!({ "skill": "demo", "script": "helper" })),
            &mut session,
            &profile,
        )
        .await
        .expect("script must succeed");

        assert!(!result.is_error);
        assert!(matches!(effect, AgentToolEffect::WorkspaceFileWritten { .. }));

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
