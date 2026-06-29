# Logging / Dev Observability 当前状态

本文记录当前已经落地的日志与开发观测链路。`.cache/PlanDocs/RustCompileAcceleratingPlan/LoggingRefactorPlan/` 是历史分析与重构计划，不是当前实现的唯一事实来源。

## 1. 普通日志链路

Rust 普通日志统一使用 `tracing::{debug, info, warn, error}` 宏，不再经过 `infrastructure::logging::logger` facade。启动期在 `src-tauri/src/infrastructure/logging/tracing_runtime.rs` 安装全局 subscriber：

- 全局 `EnvFilter` 控制所有 tracing event；
- stdout 与 rolling file 记录普通日志；
- backend log layer 写入 Dev 面板使用的 backend log store；
- user-error layer 只观察 `ERROR + target = tauritavern::user_error`。

`infrastructure::logging::logger.rs` 已删除。不要重新引入跨层 logger facade，也不要给 application service 注入 Logger trait；普通 instrumentation 直接用 `tracing`。

## 2. 用户可见错误

backend error toast 不是普通 error 日志的副作用，而是显式产品事件：

- command boundary 失败通过 `presentation/commands/helpers.rs::map_command_error()` 统一记录；
- `Cancelled` / `TooManyRequests` 等预期失败只记录 command warn，不触发 toast；
- 无法通过 `Result` 自然返回、但用户应立即感知的重要后台/宿主失败，显式使用 `target: crate::observability_targets::USER_VISIBLE_ERROR`；
- 普通诊断 `tracing::error!` 只进入 stdout/file/backend log，不自动弹窗。

应用遵循全局 `EnvFilter`。如果用户过滤掉 `tauritavern::user_error` target，对应 backend error toast 也会被过滤；这是用户对观测输出的合理控制。

## 3. 前端桥接

user-error layer 将消息交给 `app/backend_errors.rs::BackendErrorHub`。前端启动时 `src/tauri/main/bootstrap/backend-error-bridge.js` 先监听 Tauri 事件 `tauritavern-backend-error`，再调用 `backend_error_bridge_ready`：

1. Rust 侧把 bridge 标记为 ready；
2. 启动早期排队的 pending backend errors 被 drain 给前端；
3. 之后的新 backend errors 直接 emit。

如果 ready command 失败，Tauri integration 初始化会 fail-fast，而不是静默禁用 backend error toast。前端 `src/script.js` 最终消费自定义事件 `tauritavern:backend-error` 并展示 toastr。

## 4. Dev Observability

Dev Observability 通过 `app/dev_observability.rs::DevObservabilityHub` 暴露给 presentation commands。`dev_logging_commands.rs` 和 `settings_commands.rs` 不直接接触 `infrastructure::logging::*` store。

LLM API logs 仍由 infrastructure decorator `LoggingChatCompletionRepository` 记录 raw/readable 请求响应。持久化失败是诊断日志，不触发 backend error toast。

## 5. 持续开发守卫

`pnpm run check` 会运行 `pnpm run check:logging-boundaries`，防止以下旧边界回潮：

- `infrastructure::logging::logger` facade 引用；
- `logger::{debug,info,warn,error}` 调用；
- application/presentation 重新依赖 logging infrastructure；
- dev logging commands 直连 infrastructure；
- settings commands 直连 `LlmApiLogStore`。

涉及日志/可观测性改动时，至少运行：

```bash
pnpm run check:logging-boundaries
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features
```
