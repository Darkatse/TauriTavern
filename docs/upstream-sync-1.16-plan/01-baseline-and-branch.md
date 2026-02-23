# 01. 基线与分支策略

## 1. 同步基线定义

- 上游旧基线：`sillytavern-1.15.0/public`（符号链接）
- 上游目标：`sillytavern-1.16.0/public`（符号链接）
- 本地前端：`src/`（在 1.15 基础上叠加 Tauri 注入与项目特性）
- 本地后端：`src-tauri/`（Rust 命令、仓储、服务）

## 2. 本轮工作边界

- 本轮只做“计划文档”，不改业务代码。
- 后续实施按“先自动同步、后冲突合并、再契约补齐”推进。
- 不追求前向兼容；以“与上游文件系统一致、方便迁移”为主。

## 3. 分支与提交策略

- 计划分支（已创建）：`chore/upstream-sync-1.16-plan`
- 实施建议新分支：`feat/upstream-sync-1.16`

建议提交粒度：

1. `sync(frontend): import upstream 1.16 non-conflict files`
2. `refactor(tauri-injection): merge conflict files`
3. `feat(routes): align new 1.16 endpoints`
4. `feat(backend): add missing commands/contracts`
5. `test(sync): add/adjust validation cases`

## 4. 工程约束（执行时必须遵守）

- 维持 Tauri 注入层“入口收敛”：`init.js -> tauri-main.js -> tauri/main/*`
- 避免在 `script.js` 大面积硬分叉，优先通过注入层和 runtime 适配
- Rust 后端遵循现有分层（Domain/Application/Infrastructure/Presentation）
- 只新增必要逻辑，不引入临时兼容分支和冗余胶水代码

## 5. 同步原则

1. 上游优先：非 Tauri 专属文件，优先对齐上游 1.16。
2. 注入隔离：Tauri 改动集中在 `src/tauri*`、`src/init.js`、`src/tauri-main.js`、`src/scripts/extensions/runtime/*` 等隔离点。
3. 契约先行：任何前端新接口先映射到路由与命令契约，再补实现。
4. 可回滚：每一阶段可独立回退，不跨阶段混杂大改。

