# 02. AI 同步辅助程序设计

## 1. 设计目标

让 AI 在同步时可“先看报告再改代码”，避免盲改和重复 diff 操作。  
程序输出必须直接回答四类问题：

1. 哪些文件可直接同步（无本地冲突）？
2. 哪些文件必须三方合并（上游变更且本地也改过）？
3. 哪些变更会触发 Tauri 注入冲突？
4. 哪些上游新接口在当前 Tauri 路由/命令中缺失？

## 2. 形态选择

推荐作为 `fastools` 新子命令实现（Rust），而非独立脚本：

- 复用现有项目工具入口，便于团队统一执行
- 可直接输出机器可读 JSON + 人类可读 Markdown
- 后续可扩展为“半自动补丁生成器”

建议命令名：`fastools upsync analyze`

## 3. 输入与输出

### 3.1 输入参数

- `--base`：旧基线目录（例：`sillytavern-1.15.0/public`）
- `--target`：上游目标目录（例：`sillytavern-1.16.0/public`）
- `--local`：本地前端目录（例：`src`）
- `--route-dir`：Tauri 路由目录（例：`src/tauri/main/routes`）
- `--command-registry`：Rust 命令注册文件（例：`src-tauri/src/presentation/commands/registry.rs`）
- `--out`：报告输出目录（例：`docs/upstream-sync-1.16-plan/reports`）

### 3.2 输出文件

1. `01-file-classification.json`
2. `02-injection-conflicts.json`
3. `03-endpoint-gap.json`
4. `04-command-gap.json`
5. `summary.md`

## 4. 分类规则

## 4.1 文件分类

- `upstream_only_changed`：`base->target` 变更，`base->local` 未变更（可自动同步）
- `local_only_changed`：本地改过但上游无变化（通常保留）
- `both_changed`：上游与本地都改过（必须合并）

## 4.2 注入冲突判定（高优先）

若 `both_changed` 且命中以下路径或关键字，标记 `injection_conflict=true`：

- 路径：`index.html`、`script.js`、`lib.js`、`scripts/extensions.js`
- 路径前缀：`tauri/`、`scripts/extensions/runtime/`
- 关键字：`init.js`、`tauri-main.js`、`APP_INITIALIZED`、`CHAT_LOADED`、`chatLoaded`

## 4.3 接口缺口判定

1. 从上游 `target` 抽取静态 `/api/*` 字符串
2. 从 `route-dir` 抽取 `router.get/post/all(...)`
3. 计算：
   - `new_in_target`（1.16 新增接口）
   - `new_in_target_but_unhandled`（新增且未被 Tauri 路由接管）

## 4.4 命令缺口判定

1. 从路由层抽取 `safeInvoke('xxx')`
2. 从 `registry.rs` 抽取已注册命令
3. 输出 `invoke_but_not_registered`

## 5. 报告结构（示例）

```json
{
  "summary": {
    "upstream_changed": 97,
    "local_changed": 300,
    "both_changed": 23,
    "injection_conflicts": 6
  },
  "both_changed": [
    {
      "path": "script.js",
      "injection_conflict": true,
      "reason": ["app_init_event_delta", "tauri_payload_transport_patch"]
    }
  ]
}
```

## 6. 先做 MVP，再扩展

### 6.1 MVP（本次同步必须）

1. 三方文件分类
2. 注入冲突标记
3. 新增接口缺口输出
4. Markdown 汇总

### 6.2 V2（可后续加）

1. 自动生成 `git checkout/cp` 脚本（仅无冲突文件）
2. 基于 AST 的 JS 精确冲突定位
3. 针对 `script.js` 输出可合并片段建议

## 7. 使用顺序（执行期）

1. 运行 `fastools upsync analyze`
2. 先处理 `upstream_only_changed`
3. 再处理 `both_changed` 中 `injection_conflict=true`
4. 最后处理接口/命令缺口

