use serde_json::Value;

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
use tt_domain::models::agent::AgentToolResult;
use tt_domain::models::skill_script::ActivatedWorldInfoEntry;
use tt_domain::models::tool::ToolInvocation;
use tt_ports::repositories::workspace_repository::WorkspaceRepository;
use tt_ports::skill_script::{SkillScriptEngine, SkillScriptRequest};

const SKILL_SCRIPT_INVALID_NAME: &str = "skill.run_script_invalid_name";
const SKILL_SCRIPT_SKILL_NOT_VISIBLE: &str = "skill.run_script_skill_not_visible";
const SKILL_SCRIPT_NOT_FOUND: &str = "skill.run_script_not_found";
const SKILL_SCRIPT_EXECUTION_FAILED: &str = "skill.run_script_execution_failed";
const SKILL_SCRIPT_RESULT_TOO_LARGE: &str = "skill.run_script_result_too_large";

/// skill.run_script 依赖的服务与运行上下文。
/// 与工具共用的 `call` / `session` / `profile` 保持显式参数（与其他工具一致）。
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
    let script_path = match skill_service
        .skill_file_path(scope, skill, &relative_path)
        .await
    {
        Ok(path) => path,
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

    let work_dir = workspace_repository
        .run_workspace_root(run_id)
        .await
        .map_err(ApplicationError::from)?;

    let world_info_entries = prompt_snapshot
        .and_then(|snapshot| snapshot.get("worldInfoActivation"))
        .and_then(|batch| batch.get("entries"))
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .enumerate()
                .filter_map(|(index, entry)| ActivatedWorldInfoEntry::from_value(index, entry))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    tracing::info!(
        "skill.run_script invoked: skill=`{skill}` script=`{script}` args={} work_dir={}",
        serde_json::to_string(&script_args).unwrap_or_else(|_| "<unserializable>".to_string()),
        work_dir.display()
    );

    let outcome = engine
        .execute(SkillScriptRequest {
            script_path,
            args: script_args,
            work_dir,
            visible_roots: profile.workspace.visible_roots.clone(),
            writable_roots: profile.workspace.writable_roots.clone(),
            world_info_entries,
        })
        .await;

    let value = match outcome {
        Ok(result) => result.value,
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

    let rendered = serde_json::to_string(&value).unwrap_or_else(|_| value.to_string());
    let content = format!("Executed skill script `{skill}/scripts/{script}.js`. Result:\n{rendered}");

    tracing::info!(
        "skill.run_script completed: skill=`{skill}` script=`{script}` result_bytes={}",
        rendered.len()
    );

    Ok((
        AgentToolResult {
            call_id: call.call_id.clone(),
            tool_id: call.tool_id.clone(),
            content,
            structured: value,
            is_error: false,
            error_code: None,
            resource_refs: Vec::new(),
        },
        AgentToolEffect::None,
    ))
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
    use std::path::PathBuf;
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
        WorkspaceAppendResult, WorkspaceFile, WorkspaceFileList, WorkspaceWriteGuard,
    };

    // ---- fakes ----------------------------------------------------------

    enum FakeOutcome {
        Ok(Value),
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
                }),
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
        script_path: Option<PathBuf>,
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
        async fn skill_file_path(
            &self,
            _scope: SkillScope,
            _name: &str,
            _relative_path: &str,
        ) -> Result<PathBuf, DomainError> {
            match &self.script_path {
                Some(path) => Ok(path.clone()),
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
        root: PathBuf,
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
        async fn run_workspace_root(&self, _run_id: &str) -> Result<PathBuf, DomainError> {
            Ok(self.root.clone())
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
            _path: &WorkspacePath,
            _text: &str,
            _guard: WorkspaceWriteGuard,
        ) -> Result<WorkspaceFile, DomainError> {
            unreachable!("not needed")
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
            _path: &WorkspacePath,
        ) -> Result<WorkspaceFile, DomainError> {
            unreachable!("not needed")
        }
        async fn list_files(
            &self,
            _run_id: &str,
            _path: Option<&WorkspacePath>,
            _depth: usize,
            _max_entries: usize,
        ) -> Result<WorkspaceFileList, DomainError> {
            unreachable!("not needed")
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
                    root: PathBuf::from("/fake/run"),
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
                script_path: Some(PathBuf::from("/fake/scripts/helper.js")),
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
                script_path: Some(PathBuf::from("/fake/scripts/helper.js")),
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
            FakeSkillRepo { script_path: None },
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
                    script_path: Some(PathBuf::from("/fake/scripts/helper.js")),
                })),
                engine: engine.as_ref(),
                workspace_repository: &FakeWorkspaceRepo {
                    root: PathBuf::from("/fake/run"),
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
        assert_eq!(
            requests[0].script_path,
            PathBuf::from("/fake/scripts/helper.js")
        );
        assert_eq!(requests[0].args, json!({ "n": 7 }));
        assert_eq!(requests[0].work_dir, PathBuf::from("/fake/run"));
        assert_eq!(requests[0].visible_roots, vec!["output".to_string()]);
        assert_eq!(requests[0].writable_roots, vec!["output".to_string()]);
        assert!(requests[0].world_info_entries.is_empty());

        assert!(!result.is_error);
        assert_eq!(result.structured, json!({ "answer": 42 }));
        assert!(result.content.contains("demo/scripts/helper.js"));
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
