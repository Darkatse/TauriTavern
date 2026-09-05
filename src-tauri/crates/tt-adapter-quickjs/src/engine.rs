//! `SkillScriptEngine` 实现：每次执行独立 Runtime+Context，spawn_blocking 中运行，
//! 30s 超时中断、32MB 内存/256KB 栈限制、256KB 返回值上限。
//!
//! 脚本源码 + 工作区快照 + JSON 上下文由应用层传入；`workspace` API 操作内存覆盖层，
//! 写入 delta 收集到 `SkillScriptResult.writes`，由应用层落盘。

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use rquickjs::loader::{BuiltinLoader, BuiltinResolver};
use rquickjs::{Context, Ctx, Function, Module, Runtime, Value as JsValue};
use tokio::sync::Semaphore;
use tokio::task::spawn_blocking;

use tt_ports::skill_script::{
    SkillScriptEngine, SkillScriptEngineError, SkillScriptRequest, SkillScriptResult,
    SkillScriptWrite,
};

use crate::api::OverlayFs;
use crate::kit::MODULES as KIT_MODULES;
use crate::runtime_module::{RUNTIME_MODULE_NAME, RuntimeModule, RuntimeState};

const DEFAULT_EXECUTION_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_MAX_RESULT_BYTES: usize = 256 * 1024;
const DEFAULT_MAX_TOTAL_INPUT_BYTES: usize = 8 * 1024 * 1024;
const DEFAULT_MAX_TOTAL_OUTPUT_BYTES: usize = 1024 * 1024;
const MEMORY_LIMIT_BYTES: usize = 32 * 1024 * 1024;
const MAX_STACK_BYTES: usize = 256 * 1024;

/// 全局并发执行上限（固定值，与 NativeRegexService 一致；
/// 每次 QuickJS 执行独立 Runtime，2 个并发 ≈ 64MB 峰值内存）。
const MAX_CONCURRENT_EXECUTIONS: usize = 2;

/// 一次执行的引擎限制集合。
#[derive(Clone, Copy)]
struct ExecutionLimits {
    timeout: Duration,
    max_result_bytes: usize,
    max_total_input_bytes: usize,
    max_total_output_bytes: usize,
}

pub struct QuickJsScriptEngine {
    limits: ExecutionLimits,
    /// 并发 permit 池，spawn_blocking 前取得。
    jobs: Arc<Semaphore>,
}

impl QuickJsScriptEngine {
    pub fn new() -> Self {
        Self {
            limits: ExecutionLimits {
                timeout: DEFAULT_EXECUTION_TIMEOUT,
                max_result_bytes: DEFAULT_MAX_RESULT_BYTES,
                max_total_input_bytes: DEFAULT_MAX_TOTAL_INPUT_BYTES,
                max_total_output_bytes: DEFAULT_MAX_TOTAL_OUTPUT_BYTES,
            },
            jobs: Arc::new(Semaphore::new(MAX_CONCURRENT_EXECUTIONS)),
        }
    }

    /// 测试侧收紧限制的构造器。
    #[cfg(test)]
    fn with_limits(mut self, timeout: Duration, max_result_bytes: usize) -> Self {
        self.limits.timeout = timeout;
        self.limits.max_result_bytes = max_result_bytes;
        self
    }

    /// 测试侧收紧输入/输出总预算的构造器。
    #[cfg(test)]
    fn with_budgets(mut self, max_total_input_bytes: usize, max_total_output_bytes: usize) -> Self {
        self.limits.max_total_input_bytes = max_total_input_bytes;
        self.limits.max_total_output_bytes = max_total_output_bytes;
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
        let permit = self.jobs.clone().acquire_owned().await.map_err(|error| {
            SkillScriptEngineError::Internal(format!("Skill script engine queue closed: {error}"))
        })?;
        let limits = self.limits;
        spawn_blocking(move || {
            let _permit = permit;
            execute_sync(request, limits)
        })
        .await
        .map_err(|error| {
            SkillScriptEngineError::Internal(format!("Skill script engine task failed: {error}"))
        })?
    }
}

fn internal_error(error: rquickjs::Error) -> SkillScriptEngineError {
    SkillScriptEngineError::Internal(format!("QuickJS runtime failure: {error}"))
}

fn execute_sync(
    request: SkillScriptRequest,
    limits: ExecutionLimits,
) -> Result<SkillScriptResult, SkillScriptEngineError> {
    let input_bytes = total_input_bytes(&request);
    if input_bytes > limits.max_total_input_bytes {
        return Err(SkillScriptEngineError::ExecutionFailed {
            message: format!(
                "total skill script input is {input_bytes} bytes (modules + workspace snapshot + args + context), \
                 exceeding the {}-byte limit",
                limits.max_total_input_bytes
            ),
        });
    }

    let overlay = Rc::new(RefCell::new(OverlayFs::new(
        request.workspace_files,
        request.visible_roots,
        request.writable_roots,
        limits.max_total_output_bytes,
    )));

    let runtime = Runtime::new().map_err(internal_error)?;
    runtime.set_memory_limit(MEMORY_LIMIT_BYTES);
    runtime.set_max_stack_size(MAX_STACK_BYTES);

    let deadline = Instant::now() + limits.timeout;
    let timed_out = Arc::new(AtomicBool::new(false));
    let interrupt_flag = timed_out.clone();
    runtime.set_interrupt_handler(Some(Box::new(move || {
        if Instant::now() >= deadline {
            interrupt_flag.store(true, Ordering::SeqCst);
            return true;
        }
        false
    })));

    // 注册内嵌工具箱与本次执行的内存模块快照。
    // 相对导入经 BuiltinResolver 规范化后必须命中注册名单；
    // 快照外的模块（含越界 ../ 与未知的裸模块名）解析失败，模块声明/求值报错。
    let mut resolver = BuiltinResolver::default();
    let mut loader = BuiltinLoader::default();
    for &(name, source) in KIT_MODULES {
        resolver.add_module(name);
        loader.add_module(name, source);
    }
    for (name, source) in &request.modules {
        resolver.add_module(name.clone());
        loader.add_module(name.clone(), source.clone());
    }
    // Runtime 原生模块：import 解析命中 loaded_modules 注册表，
    // declare_def 声明的模块无需 loader，但名字必须在 resolver 集合中。
    resolver.add_module(RUNTIME_MODULE_NAME);
    runtime.set_loader(resolver, loader);

    let context = Context::full(&runtime).map_err(internal_error)?;
    let entry_module = request.entry_module.clone();
    let entry_source = request.modules.get(&entry_module).cloned().ok_or_else(|| {
        SkillScriptEngineError::Internal(format!(
            "entry module `{entry_module}` is missing from the module snapshot"
        ))
    })?;

    let args = request.args.clone();

    let outcome = context.with(|ctx| {
        // Runtime 状态经 ctx userdata 传入原生模块（模块求值时读取）。
        // store_userdata 仅在 userdata 被并发访问时失败，此处不可能。
        ctx.store_userdata(RuntimeState {
            frozen_macros: request.frozen_macros.clone(),
            max_render_bytes: limits.max_total_output_bytes,
            overlay: overlay.clone(),
            context: request.context.clone(),
        })
        .map_err(|_| rquickjs::Error::Unknown)?;

        // Runtime 模块：import 解析命中 loaded_modules 注册表，
        // 依赖求值先于入口 body，userdata 此时已就绪。
        Module::declare_def::<RuntimeModule, _>(ctx.clone(), RUNTIME_MODULE_NAME)?;

        let declared = Module::declare(ctx.clone(), entry_module.clone(), entry_source)?;
        let (module, eval_promise) = declared.eval()?;
        // 顶层 await：驱动 job 队列直到模块求值 settle。
        // 沙箱内没有宿主异步 API，等待外部事件的 await 无法 settle
        // → job 队列耗尽返回 WouldBlock → 落入下方执行错误分支。
        eval_promise.finish::<JsValue>()?;

        let js_args = ctx.json_parse(args.to_string())?;
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

        // 使用 QuickJS 原生 JSON 边界，不受脚本修改 globalThis.JSON 的影响。
        let text = ctx
            .json_stringify(entry_value)?
            .ok_or_else(|| {
                rquickjs::Exception::throw_message(
                    &ctx,
                    "skill script must return a JSON-serializable value; `undefined` and functions cannot be returned (return `null` explicitly instead)",
                )
            })?
            .to_string()?;
        Ok(text)
    });

    match outcome {
        Ok(text) => {
            let overlay_ref = overlay.borrow();
            let total_output = overlay_ref.output_bytes() + text.len();
            if total_output > limits.max_total_output_bytes {
                return Err(SkillScriptEngineError::ExecutionFailed {
                    message: format!(
                        "total script output is {total_output} bytes (workspace writes + logs + result), \
                         exceeding the {}-byte limit",
                        limits.max_total_output_bytes
                    ),
                });
            }
            if text.len() > limits.max_result_bytes {
                return Err(SkillScriptEngineError::ResultTooLarge {
                    actual_bytes: text.len(),
                    limit_bytes: limits.max_result_bytes,
                });
            }
            let value = serde_json::from_str(&text).map_err(|error| {
                SkillScriptEngineError::ExecutionFailed {
                    message: format!("Skill script result is not valid JSON: {error}"),
                }
            })?;
            // 收集最终 delta（BTreeMap 路径序，同一路径仅保留最终内容）
            let writes = overlay_ref
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
                last_write_path: overlay_ref.last_write_path.clone(),
                logs: overlay_ref.logs.clone(),
            })
        }
        Err(error) => {
            if timed_out.load(Ordering::SeqCst) {
                return Err(SkillScriptEngineError::ExecutionFailed {
                    message: format!(
                        "Skill script {} timed out after {:.1}s and was interrupted.",
                        entry_module,
                        limits.timeout.as_secs_f64()
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

/// 输入总量：模块源码 + 工作区快照 + args + 宿主上下文。
fn total_input_bytes(request: &SkillScriptRequest) -> usize {
    request.modules.values().map(|s| s.len()).sum::<usize>()
        + request
            .workspace_files
            .values()
            .map(|s| s.len())
            .sum::<usize>()
        + request.args.to_string().len()
        + request.context.to_string().len()
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
mod tests;
