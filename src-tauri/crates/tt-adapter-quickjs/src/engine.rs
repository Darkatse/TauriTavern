//! `SkillScriptEngine` 实现：每次执行独立 Runtime+Context，spawn_blocking 中运行，
//! 30s 超时中断、32MB 内存/256KB 栈限制、256KB 返回值上限、模块白名单加载。

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use rquickjs::loader::{Loader, Resolver};
use rquickjs::{Context, Ctx, Function, Module, Runtime, Value as JsValue};
use tokio::task::spawn_blocking;

use tt_domain::errors::DomainError;
use tt_ports::skill_script::{SkillScriptEngine, SkillScriptRequest, SkillScriptResult};

use crate::api::{register_fs_api, register_log_api, register_world_info_api};
use crate::convert::{json_to_js, js_to_json};
use crate::sandbox::SandboxIoPolicy;

pub const DEFAULT_EXECUTION_TIMEOUT: Duration = Duration::from_secs(30);
pub const DEFAULT_MAX_RESULT_BYTES: usize = 256 * 1024;
const MEMORY_LIMIT_BYTES: usize = 32 * 1024 * 1024;
const MAX_STACK_BYTES: usize = 256 * 1024;

pub struct QuickJsScriptEngine {
    libs_dir: PathBuf,
    timeout: Duration,
    max_result_bytes: usize,
}

impl QuickJsScriptEngine {
    pub fn new(libs_dir: PathBuf) -> Self {
        Self {
            libs_dir,
            timeout: DEFAULT_EXECUTION_TIMEOUT,
            max_result_bytes: DEFAULT_MAX_RESULT_BYTES,
        }
    }

    /// 测试与装配侧收紧限制的构造器。
    pub fn with_limits(mut self, timeout: Duration, max_result_bytes: usize) -> Self {
        self.timeout = timeout;
        self.max_result_bytes = max_result_bytes;
        self
    }
}

#[async_trait]
impl SkillScriptEngine for QuickJsScriptEngine {
    async fn execute(&self, request: SkillScriptRequest) -> Result<SkillScriptResult, DomainError> {
        let scripts_dir = request
            .script_path
            .parent()
            .map(PathBuf::from)
            .ok_or_else(|| {
                DomainError::InvalidData(
                    "skill script path must have a parent directory".to_string(),
                )
            })?;
        let policy = SandboxIoPolicy::new(
            request.work_dir.clone(),
            request.visible_roots.clone(),
            request.writable_roots.clone(),
            scripts_dir,
            self.libs_dir.clone(),
        );
        let timeout = self.timeout;
        let max_result_bytes = self.max_result_bytes;
        spawn_blocking(move || execute_sync(request, policy, timeout, max_result_bytes))
            .await
            .map_err(|error| {
                DomainError::InternalError(format!("Skill script engine task failed: {error}"))
            })?
    }
}

fn internal_error(error: rquickjs::Error) -> DomainError {
    DomainError::InternalError(format!("QuickJS runtime failure: {error}"))
}

fn execute_sync(
    request: SkillScriptRequest,
    policy: SandboxIoPolicy,
    timeout: Duration,
    max_result_bytes: usize,
) -> Result<SkillScriptResult, DomainError> {
    // 每次执行全新的 Runtime + Context：无跨执行共享状态（项目既定教训）。
    let entry_source = std::fs::read_to_string(&request.script_path).map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to read skill script {}: {error}",
            request.script_path.display()
        ))
    })?;

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

    runtime.set_loader(PolicyResolver(policy.clone()), PolicyLoader(policy.clone()));

    let context = Context::full(&runtime).map_err(internal_error)?;
    let module_name = request.script_path.to_string_lossy().to_string();
    let outcome = context.with(|ctx| {
        register_fs_api(&ctx, policy.clone())?;
        register_world_info_api(&ctx, request.world_info_entries.clone())?;
        register_log_api(&ctx)?;

        let declared = Module::declare(ctx.clone(), module_name.clone(), entry_source)?;
        let (module, _promise) = declared.eval()?;

        let args = json_to_js(&ctx, &request.args)?;
        let entry_result = if let Ok(function) = module.get::<_, Function>("default") {
            function.call::<_, JsValue>((args,))?
        } else if let Ok(function) = module.get::<_, Function>("main") {
            function.call::<_, JsValue>((args,))?
        } else {
            JsValue::new_undefined(ctx.clone())
        };

        js_to_json(&ctx, &entry_result)
    });

    match outcome {
        Ok(value) => {
            let encoded = serde_json::to_string(&value).map_err(|error| {
                DomainError::skill_script_execution_failed(format!(
                    "Failed to serialize skill script result: {error}"
                ))
            })?;
            if encoded.len() > max_result_bytes {
                return Err(DomainError::SkillScriptResultTooLarge {
                    actual_bytes: encoded.len(),
                    limit_bytes: max_result_bytes,
                });
            }
            Ok(SkillScriptResult { value })
        }
        Err(error) => {
            if timed_out.load(Ordering::SeqCst) {
                return Err(DomainError::skill_script_execution_failed(format!(
                    "Skill script {} timed out after {:.1}s and was interrupted.",
                    request.script_path.display(),
                    timeout.as_secs_f64()
                )));
            }
            let detail = context.with(|ctx| format_exception(&ctx, &error));
            Err(DomainError::skill_script_execution_failed(format!(
                "Skill script {} failed: {detail}",
                request.script_path.display()
            )))
        }
    }
}

/// 模块解析门控：经 SandboxIoPolicy 解析相对/裸模块为白名单内物理路径。
struct PolicyResolver(SandboxIoPolicy);

impl Resolver for PolicyResolver {
    fn resolve<'js>(
        &mut self,
        _ctx: &Ctx<'js>,
        base: &str,
        name: &str,
    ) -> rquickjs::Result<String> {
        self.0
            .resolve_module(base, name)
            .map(|path| path.to_string_lossy().to_string())
            .map_err(|message| rquickjs::Error::new_resolving_message(base, name, message))
    }
}

/// 模块加载门控：目录前缀防御后同步读文件并声明模块。
struct PolicyLoader(SandboxIoPolicy);

impl Loader for PolicyLoader {
    fn load<'js>(&mut self, ctx: &Ctx<'js>, name: &str) -> rquickjs::Result<Module<'js>> {
        let path = PathBuf::from(name);
        if !(path.starts_with(&self.0.scripts_dir) || path.starts_with(&self.0.libs_dir)) {
            return Err(rquickjs::Error::new_loading_message(
                name,
                "module is outside the allowed script directories",
            ));
        }
        let source = std::fs::read(&path).map_err(|error| {
            rquickjs::Error::new_loading_message(
                name,
                format!("failed to load module `{name}`: {error}"),
            )
        })?;
        Module::declare(ctx.clone(), name, source)
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
    use std::path::PathBuf;
    use std::time::Duration;
    use tempfile::TempDir;
    use tt_domain::errors::DomainError;
    use tt_domain::models::skill_script::ActivatedWorldInfoEntry;
    use tt_ports::skill_script::{SkillScriptEngine, SkillScriptRequest};

    use super::QuickJsScriptEngine;

    struct Fixture {
        _temp: TempDir,
        scripts_dir: PathBuf,
        work_dir: PathBuf,
        libs_dir: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let temp = TempDir::new().expect("tempdir");
            let scripts_dir = temp.path().join("skills/demo/scripts");
            let work_dir = temp.path().join("work");
            let libs_dir = temp.path().join("skill-libs");
            std::fs::create_dir_all(&scripts_dir).expect("scripts dir");
            std::fs::create_dir_all(&work_dir).expect("work dir");
            std::fs::create_dir_all(&libs_dir).expect("libs dir");
            Self {
                _temp: temp,
                scripts_dir,
                work_dir,
                libs_dir,
            }
        }

        fn write_script(&self, name: &str, source: &str) -> PathBuf {
            let path = self.scripts_dir.join(name);
            std::fs::write(&path, source).expect("write script");
            path
        }

        fn request(&self, script_path: PathBuf, args: serde_json::Value) -> SkillScriptRequest {
            SkillScriptRequest {
                script_path,
                args,
                work_dir: self.work_dir.clone(),
                visible_roots: vec!["output".to_string()],
                writable_roots: vec!["output".to_string()],
                world_info_entries: Vec::new(),
            }
        }

        fn engine(&self) -> QuickJsScriptEngine {
            QuickJsScriptEngine::new(self.libs_dir.clone())
        }
    }

    #[tokio::test]
    async fn executes_default_export_with_args() {
        let fixture = Fixture::new();
        let script = fixture.write_script(
            "add.js",
            "export default function (args) { return { sum: args.a + args.b }; }",
        );

        let result = fixture
            .engine()
            .execute(fixture.request(script, json!({ "a": 20, "b": 22 })))
            .await
            .expect("execute");

        assert_eq!(result.value, json!({ "sum": 42 }));
    }

    #[tokio::test]
    async fn falls_back_to_main_export() {
        let fixture = Fixture::new();
        let script = fixture.write_script(
            "main.js",
            "export function main(args) { return args.value; }",
        );

        let result = fixture
            .engine()
            .execute(fixture.request(script, json!({ "value": "ok" })))
            .await
            .expect("execute");

        assert_eq!(result.value, json!("ok"));
    }

    #[tokio::test]
    async fn propagates_exception_message_and_stack() {
        let fixture = Fixture::new();
        let script = fixture.write_script(
            "boom.js",
            "export default function () { throw new Error('kaboom'); }",
        );

        let error = fixture
            .engine()
            .execute(fixture.request(script, json!({})))
            .await
            .expect_err("must fail");

        match error {
            DomainError::SkillScriptExecutionFailed { message } => {
                assert!(message.contains("kaboom"), "message was: {message}");
                assert!(message.contains("boom.js"), "message was: {message}");
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[tokio::test]
    async fn timeout_interrupts_infinite_loop() {
        let fixture = Fixture::new();
        let script = fixture.write_script(
            "loop.js",
            "export default function () { while (true) {} }",
        );
        let engine = QuickJsScriptEngine::new(fixture.libs_dir.clone())
            .with_limits(Duration::from_millis(200), 256 * 1024);

        let error = engine
            .execute(fixture.request(script, json!({})))
            .await
            .expect_err("must time out");

        match error {
            DomainError::SkillScriptExecutionFailed { message } => {
                assert!(message.contains("timed out"), "message was: {message}");
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[tokio::test]
    async fn result_size_limit_is_enforced() {
        let fixture = Fixture::new();
        let script = fixture.write_script(
            "big.js",
            "export default function () { return 'x'.repeat(1024); }",
        );
        let engine = QuickJsScriptEngine::new(fixture.libs_dir.clone())
            .with_limits(Duration::from_secs(5), 512);

        let error = engine
            .execute(fixture.request(script, json!({})))
            .await
            .expect_err("must exceed");

        assert!(matches!(
            error,
            DomainError::SkillScriptResultTooLarge { .. }
        ));
    }

    #[tokio::test]
    async fn relative_imports_stay_inside_scripts_dir() {
        let fixture = Fixture::new();
        std::fs::write(
            fixture.scripts_dir.join("helper.js"),
            "export const value = 7;",
        )
        .expect("write helper");
        let script = fixture.write_script(
            "main.js",
            "import { value } from './helper.js';\nexport default function () { return value; }",
        );

        let result = fixture
            .engine()
            .execute(fixture.request(script, json!({})))
            .await
            .expect("execute");

        assert_eq!(result.value, json!(7));
    }

    #[tokio::test]
    async fn bare_imports_resolve_from_public_libs() {
        let fixture = Fixture::new();
        std::fs::write(
            fixture.libs_dir.join("mathlib.js"),
            "export const triple = (x) => 3 * x;",
        )
        .expect("write lib");
        let script = fixture.write_script(
            "main.js",
            "import { triple } from 'mathlib';\nexport default function () { return triple(5); }",
        );

        let result = fixture
            .engine()
            .execute(fixture.request(script, json!({})))
            .await
            .expect("execute");

        assert_eq!(result.value, json!(15));
    }

    #[tokio::test]
    async fn imports_escaping_scripts_dir_are_rejected() {
        let fixture = Fixture::new();
        std::fs::write(
            fixture.scripts_dir.parent().unwrap().join("SKILL.md"),
            "# skill",
        )
        .expect("write sibling file");
        let script = fixture.write_script(
            "escape.js",
            "import data from '../SKILL.md';\nexport default function () { return 1; }",
        );

        let error = fixture
            .engine()
            .execute(fixture.request(script, json!({})))
            .await
            .expect_err("must reject");

        assert!(matches!(error, DomainError::SkillScriptExecutionFailed { .. }));
    }

    #[tokio::test]
    async fn fs_api_reads_and_writes_within_gated_roots() {
        let fixture = Fixture::new();
        let script = fixture.write_script(
            "io.js",
            "export default function () {\n\
             \x20 $fs.writeText('output/note.txt', 'hello');\n\
             \x20 return $fs.readText('output/note.txt');\n\
             }",
        );

        let result = fixture
            .engine()
            .execute(fixture.request(script, json!({})))
            .await
            .expect("execute");

        assert_eq!(result.value, json!("hello"));
        assert!(fixture.work_dir.join("output/note.txt").is_file());

        let denied = fixture.write_script(
            "deny.js",
            "export default function () { return $fs.readText('input/prompt_snapshot.json'); }",
        );
        let error = fixture
            .engine()
            .execute(fixture.request(denied, json!({})))
            .await
            .expect_err("read outside visible roots must fail");
        assert!(matches!(error, DomainError::SkillScriptExecutionFailed { .. }));
    }

    #[tokio::test]
    async fn world_info_snapshot_is_readable() {
        let fixture = Fixture::new();
        let script = fixture.write_script(
            "wi.js",
            "export default function () { return $worldInfo.readActivated(); }",
        );
        let mut request = fixture.request(script, json!({}));
        request.world_info_entries = vec![ActivatedWorldInfoEntry {
            world: "lore".to_string(),
            uid: "1".to_string(),
            display_name: None,
            constant: true,
            position: None,
            content: "text".to_string(),
            ref_key: "worldinfo:lore#1".to_string(),
        }];

        let result = fixture.engine().execute(request).await.expect("execute");

        assert_eq!(
            result.value,
            json!({ "entries": [{ "uid": "1", "ref": "worldinfo:lore#1", "content": "text", "constant": true, "world": "lore" }] })
        );
    }
}

#[cfg(test)]
mod sandbox_policy_reexports {
    // 确认公共导出与 port 类型联动存在（编译期检查）。
    #[test]
    fn engine_type_is_exported() {
        let _ = crate::QuickJsScriptEngine::new(std::path::PathBuf::from("libs"));
        let _: crate::SandboxIoPolicy =
            crate::SandboxIoPolicy::new(Default::default(), vec![], vec![], Default::default(), Default::default());
    }
}
