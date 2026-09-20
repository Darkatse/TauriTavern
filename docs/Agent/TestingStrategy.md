# Agent 测试

测试准入沿用 [贡献指南](../../CONTRIBUTING.md#测试准入)：验证可观察行为、非平凡边界或已经修复的缺陷。优先扩展已有行为测试，让一次验证覆盖完整的数据流。

## 在哪里验证

| 改动 | 现有入口 |
| --- | --- |
| Run、委派、交接、提交 | [host Agent contract tests](../../src-tauri/crates/tauritavern/src/app/contract_tests/agent_runtime) |
| 文件与持久版本 | [FileAgentRepository tests](../../src-tauri/crates/tt-adapter-storage-userdata/src/repositories/file_agent_repository/tests.rs)、[文件语义闭环](../../src-tauri/crates/tt-adapter-storage-userdata/src/repositories/file_agent_repository/tests/workspace_fs.rs) |
| 模型协议与续接 | [gateway tests](../../src-tauri/crates/tt-application/src/services/agent_model_gateway/tests.rs) |
| Host API | [agent-api-contract.test.mjs](../../tests/agent-api-contract.test.mjs) |
| Profile、历史与 Timeline 界面 | [agent-system tests](../../src/scripts/extensions/agent-system/src) |
| Skill 脚本 | [脚本工具 tests](../../src-tauri/crates/tt-application/src/services/agent_tools/skill/script/tests)、[旧 Skill 执行 tests](../../src-tauri/crates/tt-adapter-workspace-shell/src/skill/engine/tests.rs) |
| Shell / JS 文件与提交 | [host 集成](../../src-tauri/crates/tauritavern/src/app/contract_tests/agent_runtime/shell.rs) |
| 执行与取消边界 | [JS tests](../../src-tauri/crates/tt-adapter-workspace-shell/src/javascript/tests.rs)、[收尾 tests](../../src-tauri/crates/tt-adapter-workspace-shell/src/tests.rs) |

涉及文件时使用临时目录和真实仓储；涉及并发时用 channel、barrier 或受控 future 协调，断言竞争操作的结果和最终内容。不要通过取得私有锁或一次 poll 返回 Pending 来证明并发正确。

## 运行检查

先运行受影响部分，例如：

```sh
cargo test --manifest-path src-tauri/Cargo.toml -p tauritavern contract_tests::agent_runtime
cargo test --manifest-path src-tauri/Cargo.toml -p tt-adapter-workspace-shell
node --test tests/agent-api-contract.test.mjs
```

完成改动后运行仓库检查：

```sh
node scripts/check-rust-crate-boundaries.mjs
pnpm run check
```

手动验证可从默认 Profile 开始：运行一次写作，查看文件与提交，再选择此次改动涉及的委派、交接、取消或历史场景。测试范围随实际行为变化确定。
