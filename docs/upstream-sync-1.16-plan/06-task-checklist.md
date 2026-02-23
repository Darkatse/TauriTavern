# 06. 执行清单（Checklist）

## A. 准备阶段

- [ ] 固定实施分支（建议：`feat/upstream-sync-1.16`）
- [ ] 实现并运行 `fastools upsync analyze`
- [ ] 生成报告并归档到 `docs/upstream-sync-1.16-plan/reports/`

## B. 自动同步阶段

- [ ] 同步 `upstream_only_changed` 文件批次 1
- [ ] 构建与启动 smoke 通过
- [ ] 同步 `upstream_only_changed` 文件批次 2
- [ ] 构建与启动 smoke 通过

## C. 冲突合并阶段（P0 -> P1 -> P2）

- [ ] 合并 `index.html`
- [ ] 合并 `script.js`
- [ ] 合并 `scripts/extensions.js`
- [ ] P0 完成后执行完整核心回归
- [ ] 合并剩余 P1 文件
- [ ] 合并剩余 P2 文件

## D. 契约补齐阶段

- [ ] 补齐 1.16 新增接口接管路由
- [ ] 补齐 Rust 命令或受控降级返回
- [ ] 校验 `safeInvoke` 与 `registry.rs` 一致性

## E. 收尾阶段

- [ ] 桌面端回归通过
- [ ] Android 回归通过（重点：大文件路径链路）
- [ ] 更新 `docs/FrontendGuide.md`（如有注入链变更）
- [ ] 更新 `docs/BackendStructure.md`（如有命令/服务变更）
- [ ] 编写同步总结与遗留事项

