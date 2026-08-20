//! `SkillScriptEngine` 实现：每次执行独立 Runtime+Context，spawn_blocking 中运行，
//! 30s 超时中断、32MB 内存/256KB 栈限制、256KB 返回值上限。
//!
//! 脚本源码 + 工作区快照 + JSON 上下文由应用层传入；`$fs` 操作内存覆盖层，
//! 写入 delta 收集到 `SkillScriptResult.writes`，由应用层落盘。

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use rquickjs::loader::{BuiltinLoader, BuiltinResolver};
use rquickjs::{Context, Ctx, Function, Module, Object, Runtime, Value as JsValue};
use tokio::task::spawn_blocking;

use tt_ports::skill_script::{
    SkillScriptEngine, SkillScriptEngineError, SkillScriptRequest, SkillScriptResult,
    SkillScriptWrite,
};

use crate::api::{
    register_fs_api, register_log_api, register_variables_api, register_world_info_api, OverlayFs,
};
use crate::convert::json_to_js;
use crate::skill_libs::builtin_modules;

pub const DEFAULT_EXECUTION_TIMEOUT: Duration = Duration::from_secs(30);
pub const DEFAULT_MAX_RESULT_BYTES: usize = 256 * 1024;
const MEMORY_LIMIT_BYTES: usize = 32 * 1024 * 1024;
const MAX_STACK_BYTES: usize = 256 * 1024;

pub struct QuickJsScriptEngine {
    timeout: Duration,
    max_result_bytes: usize,
}

impl QuickJsScriptEngine {
    pub fn new() -> Self {
        Self {
            timeout: DEFAULT_EXECUTION_TIMEOUT,
            max_result_bytes: DEFAULT_MAX_RESULT_BYTES,
        }
    }

    /// 测试侧收紧限制的构造器。
    pub fn with_limits(mut self, timeout: Duration, max_result_bytes: usize) -> Self {
        self.timeout = timeout;
        self.max_result_bytes = max_result_bytes;
        self
    }
}

impl Default for QuickJsScriptEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SkillScriptEngine for QuickJsScriptEngine {
    async fn execute(
        &self,
        request: SkillScriptRequest,
    ) -> Result<SkillScriptResult, SkillScriptEngineError> {
        let timeout = self.timeout;
        let max_result_bytes = self.max_result_bytes;
        spawn_blocking(move || execute_sync(request, timeout, max_result_bytes))
            .await
            .map_err(|error| {
                SkillScriptEngineError::Internal(format!(
                    "Skill script engine task failed: {error}"
                ))
            })?
    }
}

fn internal_error(error: rquickjs::Error) -> SkillScriptEngineError {
    SkillScriptEngineError::Internal(format!("QuickJS runtime failure: {error}"))
}

fn execute_sync(
    request: SkillScriptRequest,
    timeout: Duration,
    max_result_bytes: usize,
) -> Result<SkillScriptResult, SkillScriptEngineError> {
    let overlay = Rc::new(RefCell::new(OverlayFs::new(
        request.workspace_files,
        request.visible_roots,
        request.writable_roots,
    )));

    let runtime = Runtime::new().map_err(internal_error)?;
    runtime.set_memory_limit(MEMORY_LIMIT_BYTES);
    runtime.set_max_stack_size(MAX_STACK_BYTES);

    let deadline = Instant::now() + timeout;
    let timed_out = Arc::new(AtomicBool::new(false));
    let interrupt_flag = timed_out.clone();
    runtime.set_interrupt_handler(Some(Box::new(move || {
        if Instant::now() >= deadline {
            interrupt_flag.store(true, Ordering::SeqCst);
            return true;
        }
        false
    })));

    // 注册内嵌公共库与本次执行的内存模块快照到 BuiltinResolver/BuiltinLoader。
    // 相对导入经 BuiltinResolver 规范化后必须命中注册名单；
    // 快照外的模块（含越界 ../）解析失败，模块声明/求值报错。
    let builtin = builtin_modules();
    let mut resolver = BuiltinResolver::default();
    let mut loader = BuiltinLoader::default();
    for (name, source) in &builtin {
        resolver = resolver.with_module(name.to_string());
        loader = loader.with_module(name.to_string(), (*source).to_string());
    }
    for (name, source) in &request.modules {
        resolver = resolver.with_module(name.clone());
        loader = loader.with_module(name.clone(), source.clone());
    }
    runtime.set_loader(resolver, loader);

    let context = Context::full(&runtime).map_err(internal_error)?;
    let entry_module = request.entry_module.clone();
    let entry_source = request.modules.get(&entry_module).cloned().ok_or_else(|| {
        SkillScriptEngineError::Internal(format!(
            "entry module `{entry_module}` is missing from the module snapshot"
        ))
    })?;

    let overlay_for_fs = overlay.clone();
    let overlay_for_log = overlay.clone();
    let world_info = request.world_info.clone();
    let variables = request.variables.clone();
    let args = request.args.clone();

    let outcome = context.with(|ctx| {
        register_fs_api(&ctx, overlay_for_fs)?;
        register_world_info_api(&ctx, world_info)?;
        register_variables_api(&ctx, variables)?;
        register_log_api(&ctx, overlay_for_log)?;

        let declared = Module::declare(ctx.clone(), entry_module.clone(), entry_source)?;
        let (module, eval_promise) = declared.eval()?;
        // 顶层 await：驱动 job 队列直到模块求值 settle。
        // 沙箱内没有宿主异步 API，等待外部事件的 await 无法 settle
        // → job 队列耗尽返回 WouldBlock → 落入下方执行错误分支。
        eval_promise.finish::<JsValue>()?;

        let js_args = json_to_js(&ctx, &args)?;
        let entry = module
            .get::<_, Function>("default")
            .or_else(|_| module.get::<_, Function>("main"))
            .map_err(|_| {
                rquickjs::Exception::throw_message(
                    &ctx,
                    "skill script must export a `default` or `main` function",
                )
            })?;
        let returned = entry.call::<_, JsValue>((js_args,))?;
        // async 入口：等待返回的 Promise settle（rejection 作为 JS 异常传播）
        let entry_value = if returned.is_promise() {
            returned
                .into_promise()
                .ok_or_else(|| {
                    rquickjs::Exception::throw_message(&ctx, "expected a promise value")
                })?
                .finish::<JsValue>()?
        } else {
            returned
        };

        // 返回值边界：用 JavaScript 的 JSON.stringify 序列化，再交给 serde 解析。
        // 循环结构在 JS 侧抛 TypeError；undefined/函数不可序列化时明确报错。
        let json_object = ctx.globals().get::<_, Object>("JSON")?;
        let stringify = json_object.get::<_, Function>("stringify")?;
        let stringified = stringify.call::<_, JsValue>((entry_value,))?;
        if stringified.is_undefined() {
            return Err(rquickjs::Exception::throw_message(
                &ctx,
                "skill script must return a JSON-serializable value; `undefined` and functions cannot be returned (return `null` explicitly instead)",
            ));
        }
        let text = stringified
            .as_string()
            .ok_or_else(|| {
                rquickjs::Exception::throw_message(&ctx, "JSON.stringify returned a non-string value")
            })?
            .to_string()?;
        Ok(text)
    });

    match outcome {
        Ok(text) => {
            if text.len() > max_result_bytes {
                return Err(SkillScriptEngineError::ResultTooLarge {
                    actual_bytes: text.len(),
                    limit_bytes: max_result_bytes,
                });
            }
            let value = serde_json::from_str(&text).map_err(|error| {
                SkillScriptEngineError::ExecutionFailed {
                    message: format!("Skill script result is not valid JSON: {error}"),
                }
            })?;
            let overlay = overlay.borrow();
            // 收集最终 delta（BTreeMap 路径序，同一路径仅保留最终内容）
            let writes = overlay
                .writes
                .iter()
                .map(|(path, text)| SkillScriptWrite {
                    path: path.clone(),
                    text: text.clone(),
                })
                .collect();
            Ok(SkillScriptResult {
                value,
                writes,
                logs: overlay.logs.clone(),
            })
        }
        Err(error) => {
            if timed_out.load(Ordering::SeqCst) {
                return Err(SkillScriptEngineError::ExecutionFailed {
                    message: format!(
                        "Skill script {} timed out after {:.1}s and was interrupted.",
                        entry_module,
                        timeout.as_secs_f64()
                    ),
                });
            }
            let detail = context.with(|ctx| format_exception(&ctx, &error));
            Err(SkillScriptEngineError::ExecutionFailed {
                message: format!("Skill script {} failed: {detail}", entry_module),
            })
        }
    }
}

/// 提取 JS 异常的 message 与 stack（如可用），否则回退到错误字符串。
fn format_exception(ctx: &Ctx<'_>, error: &rquickjs::Error) -> String {
    if !matches!(error, rquickjs::Error::Exception) {
        return error.to_string();
    }
    let Some(exception) = ctx.catch().into_object() else {
        return "unknown JavaScript exception".to_string();
    };
    let message = exception
        .get::<_, JsValue>("message")
        .ok()
        .and_then(|value| value.as_string().map(|s| s.to_string()))
        .and_then(Result::ok);
    let stack = exception
        .get::<_, JsValue>("stack")
        .ok()
        .and_then(|value| value.as_string().map(|s| s.to_string()))
        .and_then(Result::ok);
    match (message, stack) {
        (Some(message), Some(stack)) => format!("{message}\n{stack}"),
        (Some(message), None) => message,
        (None, Some(stack)) => stack,
        (None, None) => "JavaScript exception without message".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use std::collections::HashMap;
    use std::time::Duration;
    use tt_ports::skill_script::{SkillScriptEngine, SkillScriptEngineError, SkillScriptRequest};

    use super::QuickJsScriptEngine;

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
            world_info: json!({ "entries": [] }),
            variables: json!({ "local": {}, "global": {} }),
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
        let engine = QuickJsScriptEngine::new()
            .with_limits(Duration::from_millis(200), 256 * 1024);
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
        let engine =
            QuickJsScriptEngine::new().with_limits(Duration::from_secs(5), 512);
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
    async fn namespaced_builtin_libs_resolve() {
        let engine = QuickJsScriptEngine::new();
        // slugify 库体积小、为 `export default` 形式，适合冒烟。
        let result = engine
            .execute(request(
                "import slugify from '@tauritavern/runtime/slugify';\nexport default function () { return slugify('Hello World!'); }",
                json!({}),
            ))
            .await
            .expect("execute");
        assert!(result.value.is_string());
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
                assert!(message.to_lowercase().contains("circular"), "message was: {message}");
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[tokio::test]
    async fn undefined_return_is_rejected() {
        // undefined / 函数不可 JSON 序列化：明确报错，不再静默转 null
        let engine = QuickJsScriptEngine::new();
        let error = engine
            .execute(request("export default function () { return undefined; }", json!({})))
            .await
            .expect_err("must fail");
        assert!(matches!(
            error,
            SkillScriptEngineError::ExecutionFailed { .. }
        ));
    }

    #[tokio::test]
    async fn fs_api_reads_and_writes_overlay() {
        let engine = QuickJsScriptEngine::new();
        let mut req = request(
            "export default function () {\n\
             \x20 $fs.writeText('output/note.txt', 'hello');\n\
             \x20 return $fs.readText('output/note.txt');\n\
             }",
            json!({}),
        );
        req.workspace_files
            .insert("output/existing.txt".to_string(), "pre-existing".to_string());

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
                "export default function () {\n\
                 \x20 $fs.writeText('output/log.txt', 'first');\n\
                 \x20 $fs.writeText('output/log.txt', 'second');\n\
                 \x20 $fs.writeText('output/log.txt', 'final');\n\
                 \x20 return 1;\n\
                 }",
                json!({}),
            ))
            .await
            .expect("execute");
        assert_eq!(result.writes.len(), 1);
        assert_eq!(result.writes[0].path, "output/log.txt");
        assert_eq!(result.writes[0].text, "final");
    }

    #[tokio::test]
    async fn fs_api_rejects_reads_outside_visible_roots() {
        let engine = QuickJsScriptEngine::new();
        let req = request(
            "export default function () { return $fs.readText('input/secret.json'); }",
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
            "export default function () { $fs.writeText('input/note.txt', 'x'); }",
            json!({}),
        );
        let error = engine.execute(req).await.expect_err("must reject");
        assert!(matches!(
            error,
            SkillScriptEngineError::ExecutionFailed { .. }
        ));
    }

    #[tokio::test]
    async fn fs_exists_checks_overlay() {
        let engine = QuickJsScriptEngine::new();
        let mut req = request(
            "export default function () {\n\
             \x20 return {\n\
             \x20   hasExisting: $fs.exists('output/data.txt'),\n\
             \x20   hasMissing: $fs.exists('output/nope.txt'),\n\
             \x20 };\n\
             }",
            json!({}),
        );
        req.workspace_files
            .insert("output/data.txt".to_string(), "content".to_string());

        let result = engine.execute(req).await.expect("execute");
        assert_eq!(result.value, json!({ "hasExisting": true, "hasMissing": false }));
    }

    #[tokio::test]
    async fn world_info_snapshot_is_readable() {
        let engine = QuickJsScriptEngine::new();
        let mut req = request(
            "export default function () { return $worldInfo.readActivated(); }",
            json!({}),
        );
        req.world_info = json!({
            "entries": [{
                "uid": "1",
                "ref": "worldinfo:lore#1",
                "content": "text",
                "constant": true,
                "world": "lore"
            }]
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
            "export default function () {\n\
             \x20 return {\n\
             \x20   score: $variables.local.get('score'),\n\
             \x20   hasName: $variables.local.has('name'),\n\
             \x20   theme: $variables.global.get('theme'),\n\
             \x20   missing: $variables.local.get('missing'),\n\
             \x20 };\n\
             }",
            json!({}),
        );
        req.variables = json!({
            "local": { "score": 42, "name": "Alice" },
            "global": { "theme": "dark" }
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
    async fn variables_write_operations_fail() {
        let engine = QuickJsScriptEngine::new();
        let error = engine
            .execute(request(
                "export default function () { $variables.local.set('x', 1); }",
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
    async fn logs_are_collected() {
        let engine = QuickJsScriptEngine::new();
        let result = engine
            .execute(request(
                "export default function () { $log.info('hello'); $log.warn('careful'); return 1; }",
                json!({}),
            ))
            .await
            .expect("execute");
        assert_eq!(result.value, json!(1));
        assert_eq!(result.logs.len(), 2);
        assert_eq!(result.logs[0].message, "hello");
        assert_eq!(result.logs[1].message, "careful");
    }
}
