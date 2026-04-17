# iOS 分发 Policy（当前实现状态）

本文档记录 **已经落地** 的 iOS/iPadOS-only 分发 Policy 现状：通过 `profile + capabilities snapshot` 将公开 TestFlight 包的默认裁剪收敛到一个集中、可审计、可被导入覆盖的运行时策略系统。

核心结论：

- **profile 可被导入覆盖**：用户导入/覆盖 `tauritavern-settings.json` 后，必须能改变 profile 与 capability 边界（允许“导入后重启生效”）。
- **iOS-only 生效**：非 iOS/iPadOS 平台忽略 `ios_policy`（不解析、不校验、不参与裁剪），避免 iOS policy 配置错误拖垮桌面端。
- **fail-fast**：iOS/iPadOS 上 policy schema/keys/allowlist 一旦非法，启动期直接失败；能力被禁用时，相关命令/服务明确拒绝，不做 silent noop。
- **无编译期 HardPolicy**：构建时只注入“出厂默认 profile”，用户始终可通过导入 settings 覆盖/解锁。

---

## 1. Settings 契约（Public Contract）

### 1.1 存放位置（source of truth）

- 数据目录：`<data_root>/default-user/`
- 文件：`<data_root>/default-user/tauritavern-settings.json`
- Policy 字段：`ios_policy`（存储为 raw JSON：`serde_json::Value`）

实现位置：

- 读取/写入：`src-tauri/src/infrastructure/repositories/file_settings_repository.rs`
- Settings 模型：`src-tauri/src/domain/models/settings.rs`

> 选择 raw JSON 的原因：允许桌面端加载来自 iOS 的 settings（即使未来 iOS policy schema 演进），同时让 iOS 端在运行时严格校验并 fail-fast。

### 1.2 v1 最小 JSON 结构（严格校验）

```jsonc
{
  "ios_policy": {
    "version": 1,
    "profile": "ios_external_beta",
    "overrides": {
      "capabilities": {
        "updates": { "manual_check": true },
        "llm": { "endpoint_overrides": true }
      }
    }
  }
}
```

约束（iOS/iPadOS 上生效）：

- `version` 必填，且必须等于 `IOS_POLICY_VERSION`（当前为 1）
- `profile` 必填，允许：`full` / `ios_internal_full` / `ios_external_beta`
- `overrides.capabilities` 只允许覆盖 **已定义** 的 capability key
- policy JSON 采用 `deny_unknown_fields`：出现未知字段（含拼写错误）直接失败
- allowlist（如下两类）支持：
  - `"all"`
  - `["item-a", "item-b"]`

实现位置：

- schema + baseline + 解析器：`src-tauri/src/domain/ios_policy.rs`

### 1.3 iOS-only 生效语义（范围隔离）

- iOS/iPadOS：解析并应用 `ios_policy`，严格校验（fail-fast）
- 非 iOS/iPadOS：`ios_policy` **被忽略**，不会因为其非法导致启动失败

实现位置：

- `IosPolicyScope::for_current_platform()`：`src-tauri/src/domain/ios_policy.rs`
- 解析入口：`resolve_ios_policy_activation_report()`：`src-tauri/src/domain/ios_policy.rs`

---

## 2. 启动链路（Activation Report 下发）

### 2.1 解析与缓存（一次性快照）

当前实现为“启动期一次性解析并缓存”，变更需要重启生效：

1. `tauritavern-settings.json` 不存在时写入 `TauriTavernSettings::default()`（iOS 会注入默认 profile，见 2.2）
2. iOS 上解析 `ios_policy` → `IosPolicyActivationReport`
3. 将 `IosPolicyActivationReport` 缓存到 `AppState.ios_policy`
4. `/api/bootstrap` 返回 `ios_policy` 快照，前端启动期读取并投影 UI

实现位置：

- AppState 构建：`src-tauri/src/app/bootstrap.rs`
- Bootstrap DTO：`src-tauri/src/presentation/commands/bootstrap_commands.rs`
- Host 路由：`src/tauri/main/routes/bootstrap-routes.js`
- 前端启动：`src/script.js`
- 前端读取帮助函数：`src/scripts/tauritavern/ios-policy.js`

### 2.2 “出厂默认 profile”（仅默认值，不是硬限制）

iOS 构建可通过 `TAURITAVERN_IOS_POLICY_PROFILE` 注入默认 profile，用于首启生成默认 `tauritavern-settings.json`：

- iOS target：允许 `full` / `ios_internal_full` / `ios_external_beta`
- 非 iOS target：强制注入为空字符串（默认不 seed `ios_policy`）

实现位置：

- build-time 注入：`src-tauri/build.rs`
- default seed：`src-tauri/src/domain/models/settings.rs`（`default_ios_policy_seed()`）

> 这只是“首次生成默认 settings”的默认值：用户导入 settings 后可覆盖 profile/overrides，且不会被系统偷偷改回去。

### 2.3 Activation Report（用于调试/回归）

`IosPolicyActivationReport` 字段：

- `scope`: `ios` / `ignored`
- `profile`: 最终采用的 profile
- `capabilities`: 最终快照（唯一真相）
- `overridden_capabilities`: overrides 生效的路径列表（用于定位“哪些能力被手动改过”）

实现位置：`src-tauri/src/domain/ios_policy.rs`

---

## 3. Profiles 与 v1 Baseline Capabilities

基线定义（profile → baseline capabilities）在：`src-tauri/src/domain/ios_policy.rs`。

### 3.1 Profiles

- `full`：无限制
- `ios_internal_full`：接近 full，但默认关闭启动后自动检查更新
- `ios_external_beta`：公开 TestFlight 外测的 review-safe 默认边界（可被导入覆盖解锁）

### 3.2 Baseline（概览）

| Capability | full | ios_internal_full | ios_external_beta |
|---|---:|---:|---:|
| `extensions.third_party_management` | ✅ | ✅ | ❌ |
| `extensions.third_party_execution` | ✅ | ✅ | ❌ |
| `extensions.system_allowlist` | `"all"` | `"all"` | `["data-migration","regex","quick-reply","tauritavern-version","token-counter"]` |
| `content.external_import` | ✅ | ✅ | ❌ |
| `updates.startup_check` | ✅ | ❌ | ❌ |
| `updates.manual_check` | ✅ | ✅ | ❌ |
| `prompts.nsfw_prompt` | ✅ | ✅ | ❌ |
| `prompts.jailbreak_prompt` | ✅ | ✅ | ❌ |
| `llm.chat_completion_sources.allowlist` | `"all"` | `"all"` | `["openai","claude","makersuite"]` |
| `llm.chat_completion_features.web_search` | ✅ | ✅ | ❌ |
| `llm.chat_completion_features.request_images` | ✅ | ✅ | ❌ |
| `llm.endpoint_overrides` | ✅ | ✅ | ❌ |
| `llm.text_completions.enabled` | ✅ | ✅ | ❌ |
| `network.request_proxy` | ✅ | ✅ | ❌ |
| `scripting.prompt_injections` | ✅ | ✅ | ❌ |
| `scripting.tool_registration` | ✅ | ✅ | ❌ |
| `ai.image_generation` | ✅ | ✅ | ❌ |
| `sync.lan` | ✅ | ✅ | ❌ |
| `about.git_info` | ✅ | ✅ | ❌ |

---

## 4. 裁决点覆盖（当前已落地）

本节只描述 **已经落地** 的裁决点；未覆盖的能力会明确标注为“仅 UI 投影”。

### 4.1 后端裁决（不可绕过）

通用拒绝 helper：

- `ensure_ios_policy_allows(...)`：`src-tauri/src/presentation/commands/helpers.rs`

能力 → 落点：

- `extensions.third_party_management`
  - Rust commands：`src-tauri/src/presentation/commands/extension_commands.rs`
    - `install_extension` / `update_extension` / `delete_extension` / `move_extension` / `get_extension_version`
- `extensions.third_party_execution` + `extensions.system_allowlist`
  - Rust commands：`src-tauri/src/presentation/commands/extension_commands.rs:get_extensions`
  - 行为：发现阶段过滤（system 仅 allowlist；local/global third-party 受 `third_party_execution` 控制）
- `content.external_import`
  - Rust commands：`src-tauri/src/presentation/commands/content_commands.rs:download_external_import_url`
- `updates.manual_check`
  - Rust commands：`src-tauri/src/presentation/commands/update_commands.rs:check_for_update`
- `llm.chat_completion_sources.allowlist`
  - Service：`src-tauri/src/application/services/chat_completion_service/mod.rs`
- `llm.endpoint_overrides`
  - Service：`src-tauri/src/application/services/chat_completion_service/mod.rs`
  - 行为：禁用时禁止 `custom` source；并拒绝任何非空 override 字段（reverse_proxy/custom_url/custom_include_headers/proxy_password）
- `llm.chat_completion_features.web_search` / `llm.chat_completion_features.request_images`
  - Service：`src-tauri/src/application/services/chat_completion_service/mod.rs`
  - 行为：payload 中出现 enable_web_search/request_images 或 request_image_* 即拒绝
- `network.request_proxy`
  - 启动期：`src-tauri/src/lib.rs` 若 settings 中 request_proxy.enabled=true 但 capability 禁用 → 直接启动失败
  - 运行期：`src-tauri/src/presentation/commands/settings_commands.rs:update_tauritavern_settings` 禁止启用 proxy
- `ai.image_generation`
  - Rust commands：`src-tauri/src/presentation/commands/stable_diffusion_commands.rs:sd_handle`
- `sync.lan`
  - Rust commands：`src-tauri/src/presentation/commands/lan_sync_commands.rs:*`（所有 LAN sync 命令统一门禁）

### 4.2 启动关键路径裁剪（避免外测默认自爆）

- `updates.startup_check`
  - 前端系统扩展 `tauritavern-version` 在 `APP_READY` 时检查 capability 并跳过启动更新检查
  - 实现：`src/scripts/extensions/tauritavern-version/index.js`
- `llm.text_completions.enabled`
  - 若用户 settings 里 `main_api === "textgenerationwebui"`，iOS policy 禁用时会在加载 settings 后强制切回 chat completion（并 toast 提示）
  - 实现：`src/script.js`

### 4.3 前端 UI 投影（入口隐藏/收口）

统一入口（启动期）：

- 将 bootstrap 的 `ios_policy` 写入 `window.__TAURITAVERN__.iosPolicy`，随后立刻执行 UI 投影
  - 实现：`src/script.js`

主要投影点：

- 核心 UI 投影：`src/scripts/tauritavern/ios-policy-ui.js`
  - `extensions.third_party_management=false`：隐藏扩展管理入口（Install/Manage/notify updates）
  - `content.external_import=false`：隐藏 external import 按钮 + onboarding 文案块
  - `prompts.nsfw_prompt=false`：隐藏 NSFW quick edit textarea 区块（仅 UI 投影）
  - `prompts.jailbreak_prompt=false`：隐藏 Jailbreak quick edit textarea + “prefer character jailbreak”开关（仅 UI 投影）
  - `llm.chat_completion_sources.allowlist`：过滤 `#chat_completion_source` 选项并修正当前值
  - `llm.endpoint_overrides=false`：隐藏 reverse proxy / custom endpoint 相关 UI
  - `llm.chat_completion_features.web_search=false`：隐藏 web search 开关
  - `llm.chat_completion_features.request_images=false`：隐藏 request images block
  - `llm.text_completions.enabled=false`：移除 Text Completion option 并隐藏 panel
  - `ai.image_generation=false`：隐藏 `#bg_chat_hint`（避免 SD 相关提示露出）
- Settings 面板投影：`src/scripts/tauri/setting/setting-panel/settings-popup.js`
  - `network.request_proxy=false`：隐藏 Request Proxy details（并在不支持 data root 选择时隐藏 system panel）
  - `sync.lan=false`：隐藏 Sync panel（避免触发相机/局域网相关入口）
- 外部导入点击门禁：`src/script.js`
  - `content.external_import=false` 时直接 toast 并 return（避免“隐藏失效/脚本触发”）
- LLM source selector 兜底：`src/scripts/openai.js`
  - `llm.chat_completion_sources.allowlist` + `llm.endpoint_overrides`：当当前选择不允许时自动回落到允许项（并 toast）
- Slash Commands 注册收口：
  - `scripting.prompt_injections=false`：不注册 `/inject` `/listinjects` `/flushinject(s)` 等命令
    - `src/scripts/slash-commands.js`
  - `scripting.tool_registration=false`：不注册 `/tools-register` 等注册类命令
    - `src/scripts/tool-calling.js`
- `tauritavern-version` 扩展（更新能力 UI）：
  - `updates.manual_check=false`：移除 “Check for Updates” 按钮，并隐藏兼容性信息/Discord 链接
  - `updates.startup_check=false`：跳过启动检查更新
  - `about.git_info=false`：隐藏 Git Info
  - `src/scripts/extensions/tauritavern-version/index.js`

> 当前仍存在少量按 `profile` 的 UI 差异（如外测隐藏 Discord 链接/兼容信息、data-migration 文案替换）；新增功能应优先以 `capabilities.*` 为唯一判断条件，避免 profile 分支扩散。

---

## 5. 已知边界（刻意未做/仅 UI）

- `prompts.nsfw_prompt` / `prompts.jailbreak_prompt`
  - 当前为 **UI 投影级别**（隐藏 quick-edit 入口），尚未在“生成请求 payload”层做后端拒绝/校验。
  - 这意味着：若未来出现绕过 UI 的 prompt 注入链路，仍需在生成链路追加更硬的裁决点（应以 capabilities 为唯一真相）。

---

## 6. 维护约束（防腐化要点）

1. 新增/调整能力时，先改 `src-tauri/src/domain/ios_policy.rs`（capability key + baseline + overrides），再落裁决点与 UI 投影；禁止直接在 UI 到处加 `if (profile === ...)`。
2. iOS 上拒绝 silent fallback：策略非法/能力禁用必须“明确失败或明确拒绝”，避免后续难排查。
3. 非 iOS 平台必须继续 **忽略** `ios_policy`，防止跨平台 settings 导入把桌面端拖死。
