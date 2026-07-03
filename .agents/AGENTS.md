# TauriTavern Workspace Guidelines for AI Agent/Chat

- **核心架构:** 严格遵循 `docs/BackendStructure.md` 中定义的 workspace crate 边界。`tauritavern` host 只承载 Tauri shell、composition、commands、AppHandle/WebView/resource/platform glue；Tauri-free concrete IO 应放入对应 `tt-adapter-*` crate。
- **Rust 哲学:** 编写符合 Rust 习惯的惯用代码（idiomatic code）。优先使用 `Result` 和 `thiserror`/`anyhow` 进行错误处理。注意所有权和借用规则。
- **模块化与抽象:** repository trait / outbound port 放在 `tt-ports`；具体实现放在 adapter crate。不要为单个实现新增无意义 trait、factory 或 facade。
- **代码复用 (DRY):** 先复用项目已有 helper；不要为了“看起来通用”抽出跨 bounded context 的清洗器、格式库或抽象层。
- **文档优先:** 在开始编码前，务必查阅 `docs/` 目录下的相关文档（特别是 `BackendStructure.md`, `FrontendGuide.md`, `FrontendHostContract.md`），理解需求、架构和计划。
- **代码一致性:** 在实现新功能前，检查项目中是否已有类似功能的代码或定义，尽可能复用或保持一致。
- **错误处理:** 必须实现清晰的错误传播。遵循 fail-fast，避免静默降级；Tauri command 边界使用 `CommandError`，领域/仓储侧使用 `DomainError`。
- **异步处理:** 对于 I/O 密集型或需要并发的操作（如文件读写、网络请求），必须使用 `async`/`await` 和 `tokio` 运行时。
- **数据传输对象 (DTO):** 在应用层和表示层（Tauri 命令）之间传递数据时，必须使用 DTO。DTO 的定义需参考文档并保持前后端一致。
- **Tauri 命令:** `#[tauri::command]` 只能存在于 `presentation` 层，并且应该调用 `application` 层的服务来执行业务逻辑，避免在命令中直接处理复杂逻辑或操作基础设施。
- **注释:** 为复杂、非显而易见的逻辑或算法添加清晰、简洁的注释。
- **测试:** 核心仓储迁移或格式语义变更必须保留最小可运行测试；优先运行受影响 crate 的 focused tests 与 `scripts/check-rust-crate-boundaries.mjs`。每次实现完成后都必须运行 harness：`pnpm run check`。
- **前端交互:** 注意 `FrontendGuide.md` 中关于与前端交互的说明，特别是 DTO 和事件的约定。
