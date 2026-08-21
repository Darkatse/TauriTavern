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
async fn falls_back_to_main_export() {
    let engine = QuickJsScriptEngine::new();
    let result = engine
        .execute(request(
            "export function main(args) { return args.value; }",
            json!({ "value": "ok" }),
        ))
        .await
        .expect("execute");
    assert_eq!(result.value, json!("ok"));
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
async fn kit_modules_are_available() {
    let result = QuickJsScriptEngine::new()
        .execute(request(
            r#"
import dayjs from '@tauritavern/kit/dayjs';
import { uniq } from '@tauritavern/kit/es-toolkit';
import { XMLParser } from '@tauritavern/kit/fast-xml-parser';
import { marked } from '@tauritavern/kit/marked';
import Papa from '@tauritavern/kit/papaparse';
import slugify from '@tauritavern/kit/slugify';

export default function () {
  return {
date: dayjs('2026-08-20').add(2, 'day').format('YYYY-MM-DD'),
unique: uniq([1, 1, 2]),
xml: new XMLParser().parse('<note><to>Tove</to></note>').note.to,
markdown: marked.parseInline('**hi**'),
csv: Papa.parse('name\nAlice', { header: true }).data[0].name,
slug: slugify('Hello World!', { lower: true, strict: true }),
  };
}
"#,
            json!({}),
        ))
        .await
        .expect("kit modules must execute");

    assert_eq!(
        result.value,
        json!({
            "date": "2026-08-22",
            "unique": [1, 2],
            "xml": "Tove",
            "markdown": "<strong>hi</strong>",
            "csv": "Alice",
            "slug": "hello-world",
        })
    );
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
async fn async_entry_function_resolves() {
    let engine = QuickJsScriptEngine::new();
    let result = engine
        .execute(request(
            "export default async function (args) { return { doubled: args.n * 2 }; }",
            json!({ "n": 21 }),
        ))
        .await
        .expect("async entry must resolve");
    assert_eq!(result.value, json!({ "doubled": 42 }));
}

#[tokio::test]
async fn promise_rejection_propagates() {
    let engine = QuickJsScriptEngine::new();
    let error = engine
        .execute(request(
            "export default async function () { throw new Error('async kaboom'); }",
            json!({}),
        ))
        .await
        .expect_err("rejection must propagate");
    match error {
        SkillScriptEngineError::ExecutionFailed { message } => {
            assert!(message.contains("async kaboom"), "message was: {message}");
        }
        other => panic!("unexpected error: {other:?}"),
    }
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
async fn unresolved_top_level_await_fails() {
    // 没有宿主异步 API：永远 pending 的顶层 await 无法 settle → 报错而非丢弃
    let engine = QuickJsScriptEngine::new();
    let error = engine
        .execute(request(
            "await new Promise(() => {});\nexport default function () { return 1; }",
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
async fn final_delta_keeps_last_write_separate_from_path_order() {
    let result = QuickJsScriptEngine::new()
        .execute(request(
            "import { workspace } from '@tauritavern/runtime';\nexport default function () {\n  workspace.writeText('output/z-debug.txt', 'debug');\n  workspace.writeText('output/a-final.md', 'final');\n  return 1;\n}",
            json!({}),
        ))
        .await
        .expect("execute");

    assert_eq!(
        result
            .writes
            .iter()
            .map(|write| write.path.as_str())
            .collect::<Vec<_>>(),
        vec!["output/a-final.md", "output/z-debug.txt"]
    );
    assert_eq!(result.last_write_path.as_deref(), Some("output/a-final.md"));
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
async fn fs_api_rejects_write_to_root_itself() {
    // 与 Application canonical 写策略一致：root 本身不是合法写路径
    let engine = QuickJsScriptEngine::new();
    let error = engine
        .execute(request(
            "import { workspace } from '@tauritavern/runtime';\nexport default function () { workspace.writeText('output', 'x'); return 1; }",
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
async fn fs_exists_checks_overlay() {
    let engine = QuickJsScriptEngine::new();
    let mut req = request(
        "import { workspace } from '@tauritavern/runtime';\nexport default function () {\n  return {\n    hasDirectory: workspace.exists('output'),\n    hasExisting: workspace.exists('output/data.txt'),\n    hasMissing: workspace.exists('output/nope.txt'),\n  };\n}",
        json!({}),
    );
    req.workspace_files
        .insert("output/data.txt".to_string(), "content".to_string());

    let result = engine.execute(req).await.expect("execute");
    assert_eq!(
        result.value,
        json!({ "hasDirectory": true, "hasExisting": true, "hasMissing": false })
    );
}

#[tokio::test]
async fn world_info_snapshot_is_readable() {
    let engine = QuickJsScriptEngine::new();
    let mut req = request(
        "import { context } from '@tauritavern/runtime';\nexport default function () { return context.worldInfo; }",
        json!({}),
    );
    req.context = json!({
        "worldInfo": {
            "entries": [{
                "uid": "1",
                "ref": "worldinfo:lore#1",
                "content": "text",
                "constant": true,
                "world": "lore"
            }]
        },
        "variables": { "local": {}, "global": {} },
    });

    let result = engine.execute(req).await.expect("execute");
    assert_eq!(
        result.value,
        json!({
            "entries": [{
                "uid": "1",
                "ref": "worldinfo:lore#1",
                "content": "text",
                "constant": true,
                "world": "lore"
            }]
        })
    );
}

#[tokio::test]
async fn variables_are_readable() {
    let engine = QuickJsScriptEngine::new();
    let mut req = request(
        "import { context } from '@tauritavern/runtime';\nexport default function () {\n  return {\n    score: context.variables.local.score,\n    hasName: Object.hasOwn(context.variables.local, 'name'),\n    theme: context.variables.global.theme,\n    missing: context.variables.local.missing ?? '',\n  };\n}",
        json!({}),
    );
    req.context = json!({
        "worldInfo": { "entries": [] },
        "variables": {
            "local": { "score": 42, "name": "Alice" },
            "global": { "theme": "dark" }
        }
    });

    let result = engine.execute(req).await.expect("execute");
    assert_eq!(
        result.value,
        json!({
            "score": 42,
            "hasName": true,
            "theme": "dark",
            "missing": "",
        })
    );
}

#[tokio::test]
async fn logs_are_collected() {
    let engine = QuickJsScriptEngine::new();
    let result = engine
        .execute(request(
            "import { log } from '@tauritavern/runtime';\nexport default function () { log.info('hello'); log.warn('careful'); return 1; }",
            json!({}),
        ))
        .await
        .expect("execute");
    assert_eq!(result.value, json!(1));
    assert_eq!(result.logs.len(), 2);
    assert_eq!(result.logs[0].message, "hello");
    assert_eq!(result.logs[1].message, "careful");
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
async fn repeated_writes_to_same_path_do_not_double_count() {
    // 同路径覆盖写按最终值记账：预算 512 时 10 次 32 字节覆盖写成功。
    let engine = QuickJsScriptEngine::new().with_budgets(DEFAULT_MAX_TOTAL_INPUT_BYTES, 512);
    let result = engine
        .execute(request(
            "import { workspace } from '@tauritavern/runtime';\n\
             export default function () {\n\
             \x20 for (let i = 0; i < 10; i++) { workspace.writeText('output/same.txt', 'x'.repeat(32)); }\n\
             \x20 return 1;\n\
             }",
            json!({}),
        ))
        .await
        .expect("within budget");
    assert_eq!(result.writes.len(), 1);
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
