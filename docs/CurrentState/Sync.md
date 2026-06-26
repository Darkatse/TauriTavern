# 同步（LAN Sync / TT-Sync v2）当前落地状态

本文档描述 **当前已经落地** 的同步能力现状：它解决什么问题、端到端链路如何工作、明确支持/不支持的边界、以及后续开发最容易误改的契约。

> 性能与协议演进背景见：`docs/TT-SyncPerformanceOptimization.md`

---

## 1. 范围与结论

TauriTavern 当前存在两种同步拓扑：

- **LAN Sync**：局域网 HTTPS/SPKI peer 协议；复用 TT-Sync v2 的 session、manifest、plan、bundle 与 DatasetPolicy 语义。旧 LAN v1 HTTP/HMAC 纵切面已删除，旧设备需要重新配对。
- **TT-Sync v2**：远端同步（TauriTavern ⇄ TT-Sync 服务端）；与 LAN Sync 共享协议语义，但拓扑是 remote hub。

关键结论（后续改动优先守住这些）：

1. **同步语义以“用户数据一致性”为中心**：scope/exclude、`(size_bytes, modified_ms)` 增量判定、Mirror delete 的时序、原子写入与 mtime 保留。
2. **同步作业全局串行**：LAN Sync 与 TT-Sync v2 共用同一个 `Semaphore(1)`（即同一时刻只能跑一个同步作业），避免多条链路并发写入相同数据目录导致破坏性竞态（见 `src-tauri/src/app/bootstrap.rs`）。
3. **长期同步 scope 由 TT-Sync `DatasetPolicy` 定义**：LAN Sync 与 TT-Sync v2 消费同一份策略，不再存在独立的 LAN v1 allowlist。
4. **v2 协议已落地 Bundle + zstd 传输形态**：把 N 个 per-file 请求收敛为 1 个 bundle 请求，并可选 zstd 压缩；旧的 per-file 端点仍保留作为 fallback。
5. **Sync Panel 入口默认走 scoped sync**：前端持久化一份 `DatasetSelection` 作为后续 LAN Sync / TT-Sync v2 默认范围，并要求对端支持 `bundle_v1 + zstd_v1`；不再静默降级到旧 LAN v1。

---

## 2. 状态目录（Sync State）与“永不入库”的排除规则

同步本身会产生状态文件（identity / paired devices / paired servers 等）。**这些状态文件必须永远不进入同步 scope**，否则会出现自我同步/循环变更/权限泄露等问题。

当前目录结构（默认用户目录下）：

- LAN Sync 状态：`default-user/user/lan-sync/`
  - LAN server 配置：`server-settings.json`（由旧 `config.json.v2_port` 一次迁移；同时保存随 App 启动开启同步端口的 `auto_start`）
  - Sync 偏好：`sync-preferences.json`（由旧 `config.json.sync_mode` 一次迁移；见 `src-tauri/src/infrastructure/lan_sync/store.rs`）
  - 自动同步规则：`automation.json`（运行期自动上传开关、目标、间隔、显式 Sync mode、范围与 bundle 要求；见 `src-tauri/src/infrastructure/sync_automation_store.rs`）
  - peer 状态：`v2/identity.json` / `v2/peers.json` / TLS 状态（见 `src-tauri/src/infrastructure/sync/lan/store.rs`）
- TT-Sync v2 状态：`default-user/user/lan-sync/tt-sync-v2/`
  - `identity.json` / `paired-servers.json`（见 `src-tauri/src/infrastructure/tt_sync/store.rs`）

LAN Sync 与 TT-Sync v2 的 manifest 扫描严格遵循 `ttsync_core::dataset::ResolvedDatasetPolicy`，并且会排除 LAN/TT 同步状态目录（见 `src-tauri/src/infrastructure/tt_sync/fs.rs`）。当前 TauriTavern 默认数据集还会排除 `_tauritavern/prompt-cache/`、`_tauritavern/.ios-policy.json`、`_cache/`、`.staging` 与同步临时文件。

默认 TauriTavern 数据集已经覆盖 Agent 连续性数据：

- `_tauritavern/agent-profiles/profiles/**`
- `_tauritavern/llm-connections/**`
- `_tauritavern/skills/{installed,index}/**`
- `_tauritavern/agent-workspaces/chats/**/persistent-states/**`
- `_tauritavern/agent-workspaces/index/runs/**`
- `_tauritavern/agent-workspaces/chats/**/runs/*/{run.json,events.jsonl}`

`default-user/secrets.json`、Agent `model-responses/`、`checkpoints/`、`backups/`、`vectors/`、`thumbnails/` 被保留为独立数据集，不在 TauriTavern 推荐默认同步中；用户可以在 Sync Panel 的“Sync content”范围选择弹窗里显式勾选。

Agent run history 只同步终态运行。扫描器会读取 `run.json` / run index JSON 的 `status`，仅纳入 `completed`、`partial_success`、`cancelled`、`failed`；运行中的 `calling_model` 等状态不会进入 manifest。

Agent run retention 复用同一套 run storage class 词汇来描述 `run_journal`、`run_context`、`run_workspace_projection`、`run_tool_io` 等路径归属，但 prune 策略不读取 `DatasetSelection`，同步 scope 仍只由 TT-Sync `DatasetPolicy` 决定。

---

## 3. 事件语义（前端可观测契约）

两类产品入口都通过统一作业事件暴露“阶段（phase）+ 进度（files/bytes）”与最终结果；手动命令仍返回 `SyncJobReport`，前端用命令返回负责手动完成/错误弹窗，避免同一作业双重提示。

- 统一作业事件：
  - 作业流：`sync:job`
  - `job` 是轻量作业上下文：`id` / `endpoint` / `intent` / `execution` / `origin`；不暴露内部 `policy` / selection。
  - progress payload：`status: "progress"`，携带 `job` 与 `progress`
  - final payload：`status: "completed" | "remote_request_accepted" | "failed"`，携带 `job` 与 `result`
  - `job.origin.type: "remote_request"` 表示“这是远端请求触发的本地作业”；`result.status: "remote_request_accepted"` 表示“本地请求对端稍后 pull 已被接受”，二者不是同一概念。

- LAN Sync：
  - pairing 请求事件：`lan_sync:pair_request`
  - 进度/完成/错误：`sync:job`
  - 手动作业完成/错误：命令返回 `SyncJobReport`；后台 inbound pull-request 由 `sync:job` final event 驱动提示和 reload。
  - 应用边界：`src-tauri/src/application/services/lan_sync_service.rs`
  - Tauri event / pairing approval adapter：`src-tauri/src/app/bootstrap.rs`
  - Axum server lifecycle adapter：`src-tauri/src/infrastructure/sync/lan/control.rs`
- TT-Sync：
  - 进度/完成/错误：`sync:job`
  - 完成/错误：手动命令通过 `SyncJobReport` 返回；不额外发 TT 专属 completed/error 事件。
  - runtime：`src-tauri/src/infrastructure/tt_sync/runtime.rs`
- 自动同步：
  - 状态/提示：`sync_auto:status` / `sync_auto:toast`
  - `last_success_at_ms` 只表示一次同步作业已经实际完成；LAN pull-request 只会更新 `last_request_accepted_at_ms`，不再记为成功完成。
  - 自动同步作业的 `sync:job` payload 会携带 `job.origin.type: "scheduled"`；前端监听器据此不打开手动进度弹窗、不触发 reload。最终面板数据刷新由 `sync_auto:toast` 复用 `SYNC_AUTOMATION_CHANGED_EVENT` 触发。

**不要破坏事件时序**：允许提升并发与吞吐，但不应改动“哪个阶段会发什么事件、作业完成/错误如何回到调用方”的外部语义。

---

## 4. 前端 Sync Panel 契约

Sync Panel 是 TauriTavern 自有设置面板，不属于上游 SillyTavern 事件 ABI。它遵循现有 host wrapper 边界：

- `src/scripts/tauri/setting/sync-app/**` 只做 Vue 展示组件，不直接访问 Tauri invoke、Popup、扫码服务或 SillyTavern host API。
- `src/scripts/tauri/setting/setting-panel/sync-popup.js` 拥有 popup / Tauri invoke / QR 扫码能力，并负责把 UI 选择转换为命令参数。
- `sync_get_dataset_catalog` 返回当前 `DatasetPolicy` 版本、支持的数据集 ID、profile ID 与 TauriTavern 默认范围；前端只持久化 dataset ID，不持久化路径。
- “Sync content”是独立持久化设置，保存在 localStorage。保存后所有 Sync Panel 发起的 LAN Sync pull、LAN Sync push-request、TT-Sync pull/push 默认都携带同一份 `DatasetSelection`。
- 自动同步配置保存在后端本地 `automation.json`，不进入同步 scope。Sync Panel 保存自动同步设置或同步范围时，会把当前 `DatasetSelection` 写入这份本地配置，供面板关闭后的 Rust 调度器使用。
- Sync Panel 展示并复制 LAN pairing URI/QR；粘贴 LAN pairing URI 时只接受 `tauritavern://lan-sync/pair?v=2`。旧 LAN v1 设备不会再作为可同步目标出现，需要重新配对。
- Sync Panel 发起同步时传入 `require_bundle_zstd: true`。如果对端缺少 `bundle_v1` 或 `zstd_v1`，操作 fail-fast，不静默降级到 per-file 或旧 LAN v1。

### 4.1 自动同步契约

- 自动同步由 Rust 后端 `SyncAutomationService` 拥有生命周期，不依赖 Sync Panel 打开，也不使用前端 `setInterval`。
- 自动同步只在 App 进程运行期间工作；冷启动后延迟 **45 秒** 才允许第一次自动上传。
- 自动同步只做上传：
  - TT-Sync 目标：执行 v2 push。
  - LAN Sync 目标：发送 pull-request，让对端从本机回拉；本机只能确认“上传请求已发送”，实际写入发生在目标设备。
- TT-Sync 自动上传的 Sync mode 是自动同步规则的一部分，不再隐式读取 LAN 手动同步偏好；Incremental / Mirror 都允许。LAN Sync 自动上传沿用现有 pull-request 语义，实际下载与 Mirror delete 由目标设备执行，因此删除行为取决于目标设备的有效 Sync mode。Mirror 可能删除目标端不存在于源端的文件，面板固定提示同步期间不要在目标设备上使用或编辑数据。
- 自动同步最小间隔为 5 分钟，最大间隔为 1440 分钟。配置启用时必须选择目标；LAN 自动目标必须是 LAN Sync peer，TT-Sync 自动目标必须具备 write 权限，Mirror 模式还必须具备 mirror_delete 权限。
- “随 App 启动开启同步端口”只启动 LAN HTTPS server，不自动开启配对。
- 自动同步成功通过 SillyTavern `toastr.info` 提示；失败通过 `toastr.warning` 提示。手动同步仍使用原来的进度弹窗、完成弹窗与必要 reload。

---

## 5. v2 同步链路（现在如何工作）

LAN Sync 与 TT-Sync v2 共享 `/v2/*` 协议族：

- `GET /v2/status`
- `POST /v2/session/open`
- `POST /v2/sync/pull-plan`
- `POST /v2/sync/push-plan`
- `GET/PUT /v2/plans/{plan_id}/files/{path_b64}`
- `GET/PUT /v2/plans/{plan_id}/bundle`
- `POST /v2/plans/{plan_id}/commit`

两者差异主要在拓扑与配对入口：TT-Sync v2 绑定远端服务端；LAN Sync 由本机启动 HTTPS peer server，并在 LAN pairing URI 中携带 SPKI pin。

### 5.1 TT-Sync v2 Pair（绑定远端服务端）

入口：`tt_sync_pair`（`src-tauri/src/presentation/commands/tt_sync_commands.rs`）→ `TtSyncService::pair`（`src-tauri/src/application/services/tt_sync_service.rs`）。

链路要点：

1. 前端传入 `pair_uri`（包含 `url` / `token` / `spki_sha256` / `expires_at_ms` 等）。
2. 客户端校验过期时间；加载/生成 TT-Sync 身份（Ed25519 seed）。
3. 调用服务端 `POST /v2/pair/complete?token=...`，保存 `paired-servers.json`。

契约：

- `base_url` **必须是 https**，并进行 **SPKI pinning**（见 `src-tauri/src/infrastructure/sync/http_client.rs`）。
- Pair 只建立信任与权限，不传输用户数据。

### 5.2 TT-Sync v2 Push / Pull（远端同步）

入口：`tt_sync_push` / `tt_sync_pull`（`src-tauri/src/presentation/commands/tt_sync_commands.rs`）。

共同步骤由 `SyncJobCoordinator` 串行化，并通过 `ttsync_client::ClientSyncEngine` 执行共享 pull/direct-push 状态机：

1. **全局 permit**：尝试获取同步许可；失败则直接返回失败 `SyncJobReport`。
2. Status：读取 `GET /v2/status`，必须支持 `dataset_scope_v1` 且 `dataset_policy_version` 匹配；旧 server 会 fail-fast，避免静默漏同步 Agent 数据。
3. `POST /v2/session/open`：用 Ed25519 对 canonical request 签名，获得 `session_token` 与 `granted_permissions`。
4. Scanning：按调用方显式传入的 `DatasetSelection` 扫描本地 manifest；缺失 selection 会 fail-fast，不再回退到旧固定范围或隐式默认范围。
5. Diffing：携带同一份 `DatasetSelection` 请求 plan：
   - pull：`POST /v2/sync/pull-plan`
   - push：`POST /v2/sync/push-plan`
6. Transfer：
   - **优先 bundle**（需服务端 `features` 声明支持；见 6.x）
   - 否则 fallback 到 per-file 并发传输
7. Deleting（仅 Mirror）：
   - pull：本地按 plan.delete 删除
   - push：在 commit 后由服务端应用删除（Mirror 语义）

pull 的额外步骤：

- pull 成功或失败但已修改本地数据时，会通过 `DataChangeReconciler` 刷新运行时缓存，避免前端继续使用旧索引/缓存。

push 的额外步骤：

- push 在上传完毕后 `POST /v2/plans/{plan_id}/commit`，Mirror delete 只在 commit 阶段生效（保持语义一致性）。

### 5.3 LAN Sync Pair / Pull / Push（局域网 peer）

入口仍是现有 LAN Sync 命令面（`src-tauri/src/presentation/commands/lan_sync_commands.rs`）：

1. `lan_sync_start_server` 启动单一 LAN HTTPS server。
2. `lan_sync_enable_pairing` / `lan_sync_get_pairing_info` 返回当前 LAN pairing URI/QR；URI 包含 `base_url`、pair token、过期时间与 `spki_sha256`。
3. `lan_sync_request_pairing` 只接受 `tauritavern://lan-sync/pair?v=2`，并通过 `POST /v2/lan/pair/complete` 建立 Ed25519 身份、SPKI pin 与 peer grant。
4. `lan_sync_sync_from_device` 直接按 peer store 查找目标并走 LAN Sync pull；找不到 peer 时 fail-fast，不回退到旧 v1 pull。
5. `lan_sync_push_to_device` 不直接上传文件，而是 `POST /v2/lan/pull-request` 请求对端回拉；pull-request body 会携带同一份 `SyncOperationOptions`，实际数据传输仍发生在对端的 pull 链路。对端需声明 `lan_pull_request_selection_v1` 才能接受 Sync Panel 的 scoped push-request。

LAN Sync 默认权限是 `read: true`、`mirror_delete: true`、`write: false`。也就是说 peer 可以从本机读取并按 Mirror 语义计算删除，但不能直接向本机 PUT 写入；局域网“push”通过通知对端 pull 来保持写入方向清晰。

---

## 6. v2 传输形态（per-file vs bundle）

### 6.1 能力协商（features）

客户端会先调用 `GET /v2/status` 获取 `features` 与 DatasetPolicy 版本；状态请求失败、`dataset_scope_v1` 缺失或策略版本不匹配都会 fail-fast：

- `bundle_v1`：支持 bundle 端点
- `zstd_v1`：支持 bundle 的 zstd 编解码
- `dataset_scope_v1`：支持携带 `DatasetSelection` 的 scope-aware plan/delete
- `lan_pull_request_selection_v1`：LAN peer 支持在 `/v2/lan/pull-request` body 中携带 `DatasetSelection`

客户端策略（见 `ttsync_client::ClientSyncEngine` 与 `src-tauri/src/infrastructure/sync/job_executor.rs`）：

- `dataset_scope_v1` 缺失或策略版本不匹配时直接报错。
- 未要求严格传输形态的旧调用：仅当存在 `bundle_v1` 才启用 bundle；仅当同时存在 `bundle_v1` + `zstd_v1` 才启用 zstd。
- Sync Panel 调用：传入 `require_bundle_zstd: true`，缺少 `bundle_v1` 或 `zstd_v1` 都会 fail-fast。

### 6.2 per-file（fallback，兼容路径）

端点：`GET/PUT /v2/plans/{plan_id}/files/{path_b64}`。

实现要点：

- LAN Sync 使用默认并发（桌面 4 / 移动 2）：`src-tauri/src/infrastructure/sync_transfer.rs`
- TT-Sync 使用更高并发（桌面 16 / 移动 8）：`src-tauri/src/infrastructure/sync/job_executor.rs`
- 所有写入都走原子写入并保留 mtime：`src-tauri/src/infrastructure/sync_fs.rs`

### 6.3 bundle（bundle_v1：把 N 个文件合并为 1 个请求）

端点：

- pull：`GET /v2/plans/{plan_id}/bundle`
- push：`PUT /v2/plans/{plan_id}/bundle`

内容类型：

- `Content-Type: application/x-ttsync-bundle`

wire framing（见 TT-Sync 的 `ttsync_core::bundle` / `ttsync_client::bundle`）：

1. `path_len: u32`（大端）
2. `path: [u8; path_len]`（UTF-8；必须能构造为 `SyncPath`）
3. `content: [u8; size_bytes]`（`size_bytes` 来自 plan entry）
4. 结束帧：`path_len == 0`

约束：

- `path_len` 上限为 **16KiB**（避免异常请求造成内存放大）。
- 服务端必须拒绝“提前结束/缺文件/重复文件/不在 plan 内”的 bundle（保证 Mirror commit 不会在部分上传时发生）。
- TauriTavern v2 客户端当前显式偏向 HTTP/1.1。bundle 是单个长流，现有 reqwest/hyper HTTP/2 默认 flow-control 在局域网实测下不如 HTTP/1.1；协议本身仍只要求 HTTPS + SPKI pinning，不把 HTTP 版本暴露为外部契约。

### 6.4 zstd（zstd_v1：端到端流式压缩）

压缩只作用于 **bundle 流整体**：

- pull：客户端发送 `Accept-Encoding: zstd`；服务端返回 `Content-Encoding: zstd` 或 identity
- push：客户端仅在确认 `zstd_v1` 后才发送 `Content-Encoding: zstd`

当前 LAN Sync pull、TT-Sync v2 pull 与 TT-Sync v2 push 共用 TT-Sync shared engine 的 bundle framing helper。

---

## 7. 正确性与断线重试（稳定性边界）

当前实现 **不做 byte-range resume**，但保证“断线不会破坏数据”，并提供可接受的重试语义：

1. **每文件精确读取**：bundle 解包按 plan 的 `size_bytes` 精确读取；若底层流提前 EOF，会报错并中止（见 TT-Sync 的 `ExactSizeReader`）。
2. **原子写入**：每文件都走 `tmp → rename → set mtime`；断线发生在写入过程中只会留下 tmp，不会覆盖目标文件（`src-tauri/src/infrastructure/sync_fs.rs`）。
3. **自然续传**：失败后重新扫描 manifest 并重新计算 plan；已成功写入的文件会因为 `(size_bytes, modified_ms)` 匹配而不再出现在新 plan.transfer 中。

---

## 8. 明确不支持（避免误解的非目标）

- 同步 scope 内 **不支持 symlink**（扫描时直接报错，见 `src-tauri/src/infrastructure/tt_sync/fs.rs`）。
- v2 协议 **不提供** bundle 内的 byte-range/断点续传；重试依赖“自然续传”。
- 不允许 LAN Sync 与 TT-Sync v2 并发执行（全局 permit 设计即为此）。

---

## 9. 后续开发最容易误改的点（约束清单）

1. **不要把 sync state 纳入 scope**：`default-user/user/lan-sync/**` 必须长期保持 excluded。
2. **不要改变 Mirror delete 的时序**：删除只能在 Mirror 且 commit/删除阶段发生，避免数据不一致。
3. **不要破坏 mtime 语义**：增量 diff 依赖 `(size_bytes, modified_ms)`，写入必须保留 `modified_ms`。
4. **不要改动事件语义**：阶段划分与完成/错误时序对前端是契约。
5. **不要把 iOS policy 本地缓存纳入 scope**：`_tauritavern/.ios-policy.json` 属于 iOS-only 宿主本地状态，用于避免同步覆盖 `tauritavern-settings.json` 时丢失已解锁的 policy。
6. **不要在 v2 链路重新引入手写 scope 数组**：新增同步目录必须先进入 TT-Sync `DatasetPolicy`，再由 LAN Sync/TT-Sync v2 消费。
7. **不要把敏感/重型 Agent 数据默认并入无选择同步**：`model-responses/`、`checkpoints/` 与密钥文件需要保持独立数据集。
8. **不要绕过 Sync Panel 的持久化 selection**：前端显示、保存、命令参数必须围绕 `DatasetSelection`；不要在 UI 中复制路径规则或用 manifest omission 伪装范围选择。
