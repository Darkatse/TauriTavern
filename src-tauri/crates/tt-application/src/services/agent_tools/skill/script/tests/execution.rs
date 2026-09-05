use super::*;

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
async fn module_budget_returns_tool_error() {
    let (result, effect) = run_with_repo(
        json!({ "skill": "demo", "script": "helper" }),
        FakeSkillRepo {
            script_source: Some("x".repeat(MAX_SCRIPT_MODULE_TOTAL_BYTES + 1)),
        },
    )
    .await;

    assert_eq!(
        result.error_code.as_deref(),
        Some("skill.run_script_execution_failed")
    );
    assert!(result.content.contains("exceeding the limit"));
    assert!(matches!(effect, AgentToolEffect::None));
}

#[tokio::test]
async fn success_builds_result_and_passes_workspace_context() {
    let engine = Arc::new(FakeScriptEngine {
        outcome: FakeOutcome::Ok(json!({ "answer": 42 })),
        requests: Mutex::new(Vec::new()),
    });
    let session = session_with_skill("demo");
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
        &session,
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
            "macro": {},
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
    let session = session_with_skill("demo");
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
        &session,
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
async fn frozen_host_context_is_passed_to_engine() {
    let engine = Arc::new(FakeScriptEngine {
        outcome: FakeOutcome::Ok(json!({})),
        requests: Mutex::new(Vec::new()),
    });
    let session = session_with_skill("demo");
    let profile = profile(true);

    let macro_context = json!({
        "schemaVersion": 1,
        "names": { "user": "Alice", "char": "Bob" },
        "character": {
            "description": "A test character",
            "personaPosition": 0,
            "alternateGreetings": ["Another greeting"]
        },
        "system": { "model": "captured-model" },
        "chat": { "lastMessageId": "42", "lastSwipeId": "2", "currentSwipeId": "1" }
    });
    let prompt_snapshot = json!({
        "worldInfoActivation": { "entries": [] },
        "frozenRunInputSnapshot": {
            "macroContext": macro_context,
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
        &session,
        &profile,
    )
    .await
    .expect("script must succeed");

    assert!(!result.is_error);

    let requests = engine.requests.lock().await;
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0].context,
        json!({
            "worldInfo": { "entries": [] },
            "variables": {
                "local": { "score": 42, "name": "Alice" },
                "global": { "theme": "dark" }
            },
            "macro": macro_context
        })
    );
}

#[test]
fn malformed_script_context_fails_fast() {
    assert!(build_script_context_json(&json!({ "worldInfoActivation": {} })).is_err());
    for (frozen, message) in [
        (
            json!({ "variables": { "local": [], "global": {} } }),
            "variables.local must be an object",
        ),
        (json!(7), "frozenRunInputSnapshot must be an object"),
        (
            json!({ "macroContext": 7 }),
            "macroContext must be an object",
        ),
    ] {
        let error = build_script_context_json(&json!({
            "worldInfoActivation": { "entries": [] },
            "frozenRunInputSnapshot": frozen,
        }))
        .unwrap_err();
        assert!(matches!(error, ApplicationError::ValidationError(_)));
        assert!(
            error
                .to_string()
                .contains(&format!("agent.invalid_skill_script_context: {message}")),
            "{error}"
        );
    }
}

#[test]
fn internal_preparation_error_remains_fatal() {
    let error = reject_preparation(
        &call(json!({})),
        ApplicationError::InternalError("disk failure".to_string()),
    )
    .expect_err("internal errors must remain fatal");
    assert!(matches!(error, ApplicationError::InternalError(_)));
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
