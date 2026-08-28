use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::sync::Mutex;

use super::*;
use tt_domain::errors::{DomainError, WorkspaceWriteConflictKind};
use tt_domain::models::agent::plan::{AgentPlanMode, AgentPlanPolicy};
use tt_domain::models::agent::profile::{
    AGENT_PROFILE_KIND, AGENT_PROFILE_SCHEMA_VERSION, AgentContextPolicy, AgentDelegationPolicy,
    AgentModelBinding, AgentModelBindingMode, AgentPresetBinding, AgentPresetBindingMode,
    AgentProfileId, AgentProfileInstructions, AgentProfileSourceTrace, AgentRunPolicy,
    AgentSkillPolicy, AgentToolPolicy, AgentWorkspacePolicy, ResolvedAgentOutputPolicy,
};
use tt_domain::models::agent::{
    AgentChatRef, AgentRun, AgentRunPresentation, ArtifactSpec, ArtifactTarget, CommitPolicy,
    WorkspaceInputManifest, WorkspaceManifest, WorkspacePath, WorkspacePersistentChangeSet,
    WorkspaceRootCommit, WorkspaceRootLifecycle, WorkspaceRootMount, WorkspaceRootScope,
    WorkspaceRootSpec,
};
use tt_domain::models::skill::{
    SkillExportResult, SkillFileRef, SkillImportInput, SkillImportPreview, SkillIndexEntry,
    SkillInstallRequest, SkillInstallResult, SkillMoveRequest, SkillReadRequest, SkillReadResult,
    SkillScope, SkillScopeFilter, SkillScopeRetargetRequest, SkillScopeRetargetResult,
    SkillSearchRequest, SkillSearchResult, SkillWriteRequest,
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
            stream: false,
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
        &session,
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

mod execution;
mod workspace;
