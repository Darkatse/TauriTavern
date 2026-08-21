use super::*;

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
    let session = session_with_skill("demo");
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
        &session,
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
    let session = session_with_skill("demo");
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
        &session,
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
    let session = session_with_skill("demo");
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
        &session,
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
    let session = session_with_skill("demo");
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
        &session,
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
    let session = session_with_skill("demo");
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
        &session,
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
    let session = session_with_skill("demo");
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
        &session,
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
async fn truncated_workspace_snapshot_returns_tool_error() {
    let engine = Arc::new(FakeScriptEngine {
        outcome: FakeOutcome::Ok(json!({})),
        requests: Mutex::new(Vec::new()),
    });
    let session = session_with_skill("demo");
    let profile = profile(true);

    let (result, effect) = script(
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
        &session,
        &profile,
    )
    .await
    .expect("truncated snapshot must remain recoverable");
    assert_eq!(
        result.error_code.as_deref(),
        Some("skill.run_script_execution_failed")
    );
    assert!(result.content.contains("truncated"));
    assert!(matches!(effect, AgentToolEffect::None));
}
