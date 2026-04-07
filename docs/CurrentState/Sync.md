# 同步（LAN Sync v1 / TT-Sync v2）当前落地状态

本文档描述 **当前已经落地** 的同步能力现状：它解决什么问题、端到端链路如何工作、明确支持/不支持的边界、以及后续开发最容易误改的契约。

> 性能与协议演进背景见：`docs/TT-SyncPerformanceOptimization.md`

---

## 1. 范围与结论

TauriTavern 当前存在两套同步链路：

- **LAN Sync（v1）**：局域网内设备间同步（协议与事件语义对齐上游 SillyTavern）。
- **TT-Sync（v2）**：公网远端同步（TauriTavern ⇄ TT-Sync 服务端），语义与 LAN Sync 对齐，但协议独立演进。

关键结论（后续改动优先守住这些）：

1. **同步语义以“用户数据一致性”为中心**：scope/exclude、`(size_bytes, modified_ms)` 增量判定、Mirror delete 的时序、原子写入与 mtime 保留。
2. **同步作业全局串行**：LAN Sync 与 TT-Sync 共用同一个 `Semaphore(1)`（即同一时刻只能跑一个同步作业），避免两条链路并发写入相同数据目录导致破坏性竞态（见 `src-tauri/src/app/bootstrap.rs`）。
3. **TT-Sync 已落地 Bundle 传输形态**：在公网高 RTT 场景下把 N 个 per-file 请求收敛为 1 个 bundle 请求，并可选 zstd 压缩；旧的 per-file 端点仍保留作为 fallback。

---

## 2. 状态目录（Sync State）与“永不入库”的排除规则

同步本身会产生状态文件（identity / paired devices / paired servers 等）。**这些状态文件必须永远不进入同步 scope**，否则会出现自我同步/循环变更/权限泄露等问题。

当前目录结构（默认用户目录下）：

- LAN Sync 状态：`default-user/user/lan-sync/`
  - `config.json` / `identity.json` / `paired-devices.json`（见 `src-tauri/src/infrastructure/lan_sync/store.rs`）
- TT-Sync v2 状态：`default-user/user/lan-sync/tt-sync-v2/`
  - `identity.json` / `paired-servers.json`（见 `src-tauri/src/infrastructure/tt_sync/store.rs`）

TT-Sync 的 manifest 扫描严格遵循 `ttsync_core::scope`，并且会排除 LAN/TT 同步状态目录（见 `src-tauri/src/infrastructure/tt_sync/fs.rs`）。

---

## 3. 事件语义（前端可观测契约）

两套同步都对前端暴露“阶段（phase）+ 进度（files/bytes）”事件，语义上保持一致：

- LAN Sync：
  - pairing 请求事件：`lan_sync:pair_request`
  - 进度/完成/错误：`lan_sync:progress` / `lan_sync:completed` / `lan_sync:error`
  - runtime：`src-tauri/src/infrastructure/lan_sync/runtime.rs`
- TT-Sync：
  - 进度/完成/错误：`tt_sync:progress` / `tt_sync:completed` / `tt_sync:error`
  - runtime：`src-tauri/src/infrastructure/tt_sync/runtime.rs`

**不要破坏事件时序**：允许提升并发与吞吐，但不应改动“哪个阶段会发什么事件、完成/错误何时发”的外部语义。

---

## 4. TT-Sync v2：端到端链路（现在如何工作）

### 4.1 Pair（绑定服务端）

入口：`tt_sync_pair`（`src-tauri/src/presentation/commands/tt_sync_commands.rs`）→ `TtSyncService::pair`（`src-tauri/src/application/services/tt_sync_service.rs`）。

链路要点：

1. 前端传入 `pair_uri`（包含 `url` / `token` / `spki_sha256` / `expires_at_ms` 等）。
2. 客户端校验过期时间；加载/生成 TT-Sync 身份（Ed25519 seed）。
3. 调用服务端 `POST /v2/pair/complete?token=...`，保存 `paired-servers.json`。

契约：

- `base_url` **必须是 https**，并进行 **SPKI pinning**（见 `src-tauri/src/infrastructure/tt_sync/v2_api.rs`）。
- Pair 只建立信任与权限，不传输用户数据。

### 4.2 Push / Pull（同步）

入口：`tt_sync_push` / `tt_sync_pull`（`src-tauri/src/presentation/commands/tt_sync_commands.rs`）。

共同步骤：

1. **全局 permit**：尝试获取同步许可；失败则发 error 事件并直接返回（见 `src-tauri/src/application/services/tt_sync_service.rs`）。
2. `POST /v2/session/open`：用 Ed25519 对 canonical request 签名，获得 `session_token` 与 `granted_permissions`。
3. Scanning：扫描本地 manifest（`src-tauri/src/infrastructure/tt_sync/fs.rs`）。
4. Diffing：请求 plan：
   - pull：`POST /v2/sync/pull-plan`
   - push：`POST /v2/sync/push-plan`
5. Transfer：
   - **优先 bundle**（需服务端 `features` 声明支持；见 5.x）
   - 否则 fallback 到 per-file 并发传输
6. Deleting（仅 Mirror）：
   - pull：本地按 plan.delete 删除
   - push：在 commit 后由服务端应用删除（Mirror 语义）

pull 的额外步骤：

- pull 完成后会刷新运行时缓存（避免前端继续使用旧索引/缓存），见 `TtSyncService::pull`。

push 的额外步骤：

- push 在上传完毕后 `POST /v2/plans/{plan_id}/commit`，Mirror delete 只在 commit 阶段生效（保持语义一致性）。

---

## 5. TT-Sync v2：传输形态（per-file vs bundle）

### 5.1 能力协商（features）

客户端会先调用 `GET /v2/status` 获取 `features`（失败则当作不支持，走 fallback）：

- `bundle_v1`：支持 bundle 端点
- `zstd_v1`：支持 bundle 的 zstd 编解码

客户端策略（见 `src-tauri/src/infrastructure/tt_sync/push.rs`、`src-tauri/src/infrastructure/tt_sync/pull.rs`）：

- 仅当存在 `bundle_v1` 才启用 bundle。
- 仅当同时存在 `bundle_v1` + `zstd_v1` 才启用 zstd（push 不能“试一试”，必须先确认）。

### 5.2 per-file（fallback，兼容路径）

端点：`GET/PUT /v2/plans/{plan_id}/files/{path_b64}`。

实现要点：

- LAN Sync 使用默认并发（桌面 4 / 移动 2）：`src-tauri/src/infrastructure/sync_transfer.rs`
- TT-Sync 使用更高并发（桌面 16 / 移动 8）：`src-tauri/src/infrastructure/tt_sync/transfer.rs`
- 所有写入都走原子写入并保留 mtime：`src-tauri/src/infrastructure/sync_fs.rs`

### 5.3 bundle（bundle_v1：把 N 个文件合并为 1 个请求）

端点：

- pull：`GET /v2/plans/{plan_id}/bundle`
- push：`PUT /v2/plans/{plan_id}/bundle`

内容类型：

- `Content-Type: application/x-ttsync-bundle`

wire framing（见 `src-tauri/src/infrastructure/tt_sync/bundle.rs`）：

1. `path_len: u32`（大端）
2. `path: [u8; path_len]`（UTF-8；必须能构造为 `SyncPath`）
3. `content: [u8; size_bytes]`（`size_bytes` 来自 plan entry）
4. 结束帧：`path_len == 0`

约束：

- `path_len` 上限为 **16KiB**（避免异常请求造成内存放大）。
- 服务端必须拒绝“提前结束/缺文件/重复文件/不在 plan 内”的 bundle（保证 Mirror commit 不会在部分上传时发生）。

### 5.4 zstd（zstd_v1：端到端流式压缩）

压缩只作用于 **bundle 流整体**：

- pull：客户端发送 `Accept-Encoding: zstd`；服务端返回 `Content-Encoding: zstd` 或 identity
- push：客户端仅在确认 `zstd_v1` 后才发送 `Content-Encoding: zstd`

---

## 6. 正确性与断线重试（稳定性边界）

当前实现 **不做 byte-range resume**，但保证“断线不会破坏数据”，并提供可接受的重试语义：

1. **每文件精确读取**：bundle 解包按 plan 的 `size_bytes` 精确读取；若底层流提前 EOF，会报错并中止（见 `ExactSizeReader`：`src-tauri/src/infrastructure/tt_sync/bundle.rs`）。
2. **原子写入**：每文件都走 `tmp → rename → set mtime`；断线发生在写入过程中只会留下 tmp，不会覆盖目标文件（`src-tauri/src/infrastructure/sync_fs.rs`）。
3. **自然续传**：失败后重新扫描 manifest 并重新计算 plan；已成功写入的文件会因为 `(size_bytes, modified_ms)` 匹配而不再出现在新 plan.transfer 中。

---

## 7. 明确不支持（避免误解的非目标）

- 同步 scope 内 **不支持 symlink**（扫描时直接报错，见 `src-tauri/src/infrastructure/tt_sync/fs.rs`）。
- TT-Sync v2 **不提供** bundle 内的 byte-range/断点续传；重试依赖“自然续传”。
- 不允许 LAN Sync 与 TT-Sync 并发执行（全局 permit 设计即为此）。

---

## 8. 后续开发最容易误改的点（约束清单）

1. **不要把 sync state 纳入 scope**：`default-user/user/lan-sync/**` 必须长期保持 excluded。
2. **不要改变 Mirror delete 的时序**：删除只能在 Mirror 且 commit/删除阶段发生，避免数据不一致。
3. **不要破坏 mtime 语义**：增量 diff 依赖 `(size_bytes, modified_ms)`，写入必须保留 `modified_ms`。
4. **不要改动事件语义**：阶段划分与完成/错误时序对前端是契约。

