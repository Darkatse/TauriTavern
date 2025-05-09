# TauriTavern Workspace Guidelines for AI Agent/Chat

- **核心架构:** 严格遵循 `docs/BackendStructure.md` 中定义的 Clean Architecture（领域层 -> 应用层 -> 基础设施层/表示层）。保持各层职责清晰和依赖关系正确。
- **Rust 哲学:** 编写符合 Rust 习惯的惯用代码（idiomatic code）。优先使用 `Result` 和 `thiserror`/`anyhow` 进行错误处理。注意所有权和借用规则。
- **模块化与抽象:** 优先考虑模块化和低耦合设计。在层与层之间或需要解耦的地方，使用 Traits 定义接口（抽象）。
- **代码复用 (DRY):** 避免重复代码。当发现相似逻辑时，优先考虑将其抽象为函数、方法、泛型或 Trait。
- **文档优先:** 在开始编码前，务必查阅 `docs/` 目录下的相关文档（特别是 `BackendStructure.md`, `FrontendGuide.md`, `PRD.md`, `ImplementationPlan.md`），理解需求、架构和计划。
- **尊重边界:** 严格遵守 `PRD.md` 和 `ImplementationPlan.md` 中定义的项目范围和 MVP（最小可行产品）界限。不实现文档未定义或明确排除的功能。
- **代码一致性:** 在实现新功能前，检查项目中是否已有类似功能的代码或定义，尽可能复用或保持一致。
- **错误处理:** 必须实现健壮的错误处理。遵循 `BackendStructure.md` 中定义的分层错误类型（如 `DomainError`, `CommandError`）和转换模式。错误需要清晰地向上传递。
- **异步处理:** 对于 I/O 密集型或需要并发的操作（如文件读写、网络请求），必须使用 `async`/`await` 和 `tokio` 运行时。
- **数据传输对象 (DTO):** 在应用层和表示层（Tauri 命令）之间传递数据时，必须使用 DTO。DTO 的定义需参考文档并保持前后端一致。
- **Tauri 命令:** `#[tauri::command]` 只能存在于 `presentation` 层，并且应该调用 `application` 层的服务来执行业务逻辑，避免在命令中直接处理复杂逻辑或操作基础设施。
- **注释:** 为复杂、非显而易见的逻辑或算法添加清晰、简洁的注释。
- **测试:** 鼓励为核心业务逻辑（尤其是在 Domain 和 Application 层）编写单元测试。
- **前端交互:** 注意 `FrontendGuide.md` 中关于与前端交互的说明，特别是 DTO 和事件的约定。