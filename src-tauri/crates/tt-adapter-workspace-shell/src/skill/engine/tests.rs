use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use serde_json::json;

use tt_ports::skill_script::{SkillScriptEngine, SkillScriptEngineError, SkillScriptRequest};

use super::{DEFAULT_MAX_TOTAL_INPUT_BYTES, DEFAULT_MAX_TOTAL_OUTPUT_BYTES, QuickJsScriptEngine};

fn request(source: &str, args: serde_json::Value) -> SkillScriptRequest {
    let mut modules = HashMap::new();
    modules.insert("scripts/main.js".to_string(), source.to_string());
    SkillScriptRequest {
        frozen_macros: Default::default(),
        entry_module: "scripts/main.js".to_string(),
        modules,
        args,
        workspace_files: HashMap::new(),
        visible_roots: vec!["output".to_string()],
        writable_roots: vec!["output".to_string()],
        context: json!({
            "worldInfo": { "entries": [] },
            "variables": { "local": {}, "global": {} },
        }),
    }
}

#[tokio::test]
async fn executes_default_export_with_args() {
    let engine = QuickJsScriptEngine::new();
    let result = engine
        .execute(request(
            "export default function (args) { return { sum: args.a + args.b }; }",
            json!({ "a": 20, "b": 22 }),
        ))
        .await
        .expect("execute");
    assert_eq!(result.value, json!({ "sum": 42 }));
}

#[tokio::test]
async fn propagates_exception_message_and_stack() {
    let engine = QuickJsScriptEngine::new();
    let error = engine
        .execute(request(
            "export default function () { throw new Error('kaboom'); }",
            json!({}),
        ))
        .await
        .expect_err("must fail");
    match error {
        SkillScriptEngineError::ExecutionFailed { message } => {
            assert!(message.contains("kaboom"), "message was: {message}");
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[tokio::test]
async fn timeout_interrupts_infinite_loop() {
    let engine = QuickJsScriptEngine::new().with_limits(Duration::from_millis(200), 256 * 1024);
    let error = engine
        .execute(request(
            "export default function () { while (true) {} }",
            json!({}),
        ))
        .await
        .expect_err("must time out");
    match error {
        SkillScriptEngineError::ExecutionFailed { message } => {
            assert!(message.contains("timed out"), "message was: {message}");
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[tokio::test]
async fn result_size_limit_is_enforced() {
    let engine = QuickJsScriptEngine::new().with_limits(Duration::from_secs(5), 512);
    let error = engine
        .execute(request(
            "export default function () { return 'x'.repeat(1024); }",
            json!({}),
        ))
        .await
        .expect_err("must exceed");
    assert!(matches!(
        error,
        SkillScriptEngineError::ResultTooLarge { .. }
    ));
}

#[tokio::test]
async fn relative_imports_resolve_within_module_snapshot() {
    let engine = QuickJsScriptEngine::new();
    let mut req = request(
        "import { add } from './lib/a.js';\nexport default function () { return add(1, 2); }",
        json!({}),
    );
    req.modules.insert(
        "scripts/lib/a.js".to_string(),
        "export const add = (a, b) => a + b;".to_string(),
    );
    let result = engine.execute(req).await.expect("execute");
    assert_eq!(result.value, json!(3));
}

#[tokio::test]
async fn imports_outside_module_snapshot_fail() {
    // `../outside.js` 从 scripts/main.js 规范化为 outside.js，
    // 不在模块快照中 → 解析失败（Application 只提供 scripts/ 下的模块，
    // 越界导入由此天然失败，无需物理路径沙箱）。
    let engine = QuickJsScriptEngine::new();
    let error = engine
        .execute(request(
            "import { secret } from '../outside.js';\nexport default function () { return secret; }",
            json!({}),
        ))
        .await
        .expect_err("must fail");
    assert!(matches!(
        error,
        SkillScriptEngineError::ExecutionFailed { .. }
    ));
}

#[tokio::test]
async fn missing_entry_module_in_snapshot_fails() {
    let engine = QuickJsScriptEngine::new();
    let mut req = request("export default function () { return 1; }", json!({}));
    req.entry_module = "scripts/absent.js".to_string();
    let error = engine.execute(req).await.expect_err("must fail");
    assert!(matches!(error, SkillScriptEngineError::Internal(..)));
}

#[tokio::test]
async fn top_level_await_is_waited() {
    let engine = QuickJsScriptEngine::new();
    let result = engine
        .execute(request(
            "let ready = false;\nawait Promise.resolve().then(() => { ready = true; });\nexport default function () { return { ready }; }",
            json!({}),
        ))
        .await
        .expect("top-level await must settle");
    assert_eq!(result.value, json!({ "ready": true }));
}

#[tokio::test]
async fn missing_export_fails_with_clear_message() {
    let engine = QuickJsScriptEngine::new();
    let error = engine
        .execute(request("export const helper = 42;", json!({})))
        .await
        .expect_err("must fail on missing export");
    match error {
        SkillScriptEngineError::ExecutionFailed { message } => {
            assert!(
                message.contains("default") || message.contains("main"),
                "message was: {message}"
            );
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[tokio::test]
async fn circular_reference_fails_instead_of_recursing() {
    // JSON.stringify 在 JS 侧对循环结构抛 TypeError，不再依赖 Rust 递归转换
    let engine = QuickJsScriptEngine::new();
    let error = engine
        .execute(request(
            "export default function () { const a = {}; a.self = a; return a; }",
            json!({}),
        ))
        .await
        .expect_err("must fail");
    match error {
        SkillScriptEngineError::ExecutionFailed { message } => {
            assert!(
                message.to_lowercase().contains("circular"),
                "message was: {message}"
            );
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[tokio::test]
async fn undefined_return_is_rejected() {
    // undefined / 函数不可 JSON 序列化：明确报错，不再静默转 null
    let engine = QuickJsScriptEngine::new();
    let error = engine
        .execute(request(
            "export default function () { return undefined; }",
            json!({}),
        ))
        .await
        .expect_err("must fail");
    assert!(matches!(
        error,
        SkillScriptEngineError::ExecutionFailed { .. }
    ));
}

#[tokio::test]
async fn result_serialization_ignores_mutated_global_json() {
    let result = QuickJsScriptEngine::new()
        .execute(request(
            "export default function () { globalThis.JSON.stringify = () => '\"wrong\"'; return { ok: true }; }",
            json!({}),
        ))
        .await
        .expect("execute");

    assert_eq!(result.value, json!({ "ok": true }));
}

#[tokio::test]
async fn fs_api_reads_and_writes_overlay() {
    let engine = QuickJsScriptEngine::new();
    let mut req = request(
        "import { workspace } from '@tauritavern/runtime';\nexport default function () {\n  workspace.writeText('output/note.txt', 'hello');\n  return workspace.readText('output/note.txt');\n}",
        json!({}),
    );
    req.workspace_files.insert(
        "output/existing.txt".to_string(),
        "pre-existing".to_string(),
    );

    let result = engine.execute(req).await.expect("execute");

    assert_eq!(result.value, json!("hello"));
    assert_eq!(result.writes.len(), 1);
    assert_eq!(result.writes[0].path, "output/note.txt");
    assert_eq!(result.writes[0].text, "hello");
}

#[tokio::test]
async fn multiple_writes_to_same_path_produce_single_final_delta() {
    let engine = QuickJsScriptEngine::new();
    let result = engine
        .execute(request(
            "import { workspace } from '@tauritavern/runtime';\nexport default function () {\n  workspace.writeText('output/log.txt', 'first');\n  workspace.writeText('output/log.txt', 'second');\n  workspace.writeText('output/log.txt', 'final');\n  return 1;\n}",
            json!({}),
        ))
        .await
        .expect("execute");
    assert_eq!(result.writes.len(), 1);
    assert_eq!(result.writes[0].path, "output/log.txt");
    assert_eq!(result.writes[0].text, "final");
    assert_eq!(result.last_write_path.as_deref(), Some("output/log.txt"));
}

#[tokio::test]
async fn fs_api_rejects_reads_outside_visible_roots() {
    let engine = QuickJsScriptEngine::new();
    let req = request(
        "import { workspace } from '@tauritavern/runtime';\nexport default function () { return workspace.readText('input/secret.json'); }",
        json!({}),
    );
    let error = engine.execute(req).await.expect_err("must reject");
    assert!(matches!(
        error,
        SkillScriptEngineError::ExecutionFailed { .. }
    ));
}

#[tokio::test]
async fn fs_api_rejects_writes_outside_writable_roots() {
    let engine = QuickJsScriptEngine::new();
    let req = request(
        "import { workspace } from '@tauritavern/runtime';\nexport default function () { workspace.writeText('input/note.txt', 'x'); }",
        json!({}),
    );
    let error = engine.execute(req).await.expect_err("must reject");
    assert!(matches!(
        error,
        SkillScriptEngineError::ExecutionFailed { .. }
    ));
}

#[tokio::test]
async fn fs_api_rejects_backslash_path_escape() {
    let error = QuickJsScriptEngine::new()
        .execute(request(
            r#"import { workspace } from '@tauritavern/runtime';
export default function () { workspace.writeText('output\\..\\input\\note.txt', 'x'); }"#,
            json!({}),
        ))
        .await
        .expect_err("must reject");
    assert!(matches!(
        error,
        SkillScriptEngineError::ExecutionFailed { .. }
    ));
}

#[tokio::test]
async fn runtime_api_globals_are_not_injected() {
    let engine = QuickJsScriptEngine::new();
    let result = engine
        .execute(request(
            "import { workspace, log, context } from '@tauritavern/runtime';\n\
             export default function () {\n\
             \x20 return {\n\
             \x20   hasFs: typeof $fs !== 'undefined',\n\
             \x20   hasLog: typeof $log !== 'undefined',\n\
             \x20   hasWorldInfo: typeof $worldInfo !== 'undefined',\n\
             \x20   hasVariables: typeof $variables !== 'undefined',\n\
             \x20   workspaceWorks: typeof workspace.writeText === 'function',\n\
             \x20   logWorks: typeof log.info === 'function',\n\
             \x20   contextWorks: Array.isArray(context.worldInfo.entries),\n\
             \x20 };\n\
             }",
            json!({}),
        ))
        .await
        .expect("execute");
    assert_eq!(
        result.value,
        json!({
            "hasFs": false,
            "hasLog": false,
            "hasWorldInfo": false,
            "hasVariables": false,
            "workspaceWorks": true,
            "logWorks": true,
            "contextWorks": true,
        })
    );
}

#[tokio::test]
async fn input_budget_exceeded_fails_fast() {
    let engine = QuickJsScriptEngine::new().with_budgets(1024, DEFAULT_MAX_TOTAL_OUTPUT_BYTES);
    let mut req = request("export default function () { return 1; }", json!({}));
    req.workspace_files
        .insert("output/big.txt".to_string(), "x".repeat(2048));
    let error = engine.execute(req).await.expect_err("must fail");
    match error {
        SkillScriptEngineError::ExecutionFailed { message } => {
            assert!(message.contains("input"), "message was: {message}");
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[tokio::test]
async fn output_budget_exceeded_by_writes_fails_fast() {
    let engine = QuickJsScriptEngine::new().with_budgets(DEFAULT_MAX_TOTAL_INPUT_BYTES, 1024);
    let error = engine
        .execute(request(
            "import { workspace } from '@tauritavern/runtime';\n\
             export default function () {\n\
             \x20 for (let i = 0; i < 40; i++) {\n\
             \x20   workspace.writeText('output/f' + i + '.txt', 'x'.repeat(64));\n\
             \x20 }\n\
             \x20 return 1;\n\
             }",
            json!({}),
        ))
        .await
        .expect_err("must exceed output budget");
    match error {
        SkillScriptEngineError::ExecutionFailed { message } => {
            assert!(message.contains("output"), "message was: {message}");
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[tokio::test]
async fn output_budget_exceeded_by_logs_fails_fast() {
    let engine = QuickJsScriptEngine::new().with_budgets(DEFAULT_MAX_TOTAL_INPUT_BYTES, 512);
    let error = engine
        .execute(request(
            "import { log } from '@tauritavern/runtime';\n\
             export default function () {\n\
             \x20 for (let i = 0; i < 100; i++) { log.info('x'); }\n\
             \x20 return 1;\n\
             }",
            json!({}),
        ))
        .await
        .expect_err("must exceed output budget");
    assert!(matches!(
        error,
        SkillScriptEngineError::ExecutionFailed { .. }
    ));
}

#[tokio::test]
async fn concurrent_executions_all_complete() {
    let engine = Arc::new(QuickJsScriptEngine::new());
    let mut handles = Vec::new();
    for _ in 0..8 {
        let engine = engine.clone();
        handles.push(tokio::spawn(async move {
            engine
                .execute(request(
                    "export default function () { return 1; }",
                    json!({}),
                ))
                .await
        }));
    }
    for handle in handles {
        handle.await.expect("join").expect("execute");
    }
}

#[tokio::test]
async fn native_macro_render_uses_frozen_values_and_enforces_its_output_budget() {
    let captured = "abcdefghijklmnopqrst";
    let macro_context = json!({ "names": { "char": captured } });
    let mut input = request(
        r#"
        import { context, macros } from '@tauritavern/runtime';
        export default () => {
            context.macro.names.char = 'changed';
            let limited = false;
            try { macros.render('{{char}}'.repeat(4)); } catch (error) { limited = /exceeds 64 bytes/.test(error.message); }
            return { text: macros.render('{{char}}'), limited };
        }
    "#,
        json!({}),
    );
    input.context["macro"] = macro_context.clone();
    input.frozen_macros = Arc::new(
        tt_domain::frozen_macros::FrozenMacros::from_context(&macro_context, None).unwrap(),
    );
    let engine = QuickJsScriptEngine::new().with_budgets(DEFAULT_MAX_TOTAL_INPUT_BYTES, 64);
    let result = engine.execute(input).await.unwrap();
    assert_eq!(result.value, json!({ "text": captured, "limited": true }));
}
