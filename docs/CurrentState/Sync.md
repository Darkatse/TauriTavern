# 同步（LAN Sync / TT-Sync v2）

LAN Sync 与远端 TT-Sync 共用 v2 的数据集、计划、传输和提交语义。本文说明职责与稳定契约，具体字段、参数和默认值以源码为准。

## 架构与数据方向

| 拓扑 | 连接与传输 |
| --- | --- |
| LAN Sync | 设备间通过 HTTPS 连接；支持附近发现、手动地址和配对链接。上传通过请求对端回拉完成。 |
| TT-Sync | 通过服务端邀请链接绑定远端服务，支持直接上传和下载。 |

应用层负责配对、偏好与作业编排，`tt-adapter-sync` 承载网络、存储和共享传输引擎，Tauri host 负责命令、事件与平台能力。分层遵循[后端结构](../BackendStructure.md)。

## 数据范围与写入约束

- 两种拓扑使用同一份 `DatasetPolicy`，作业范围由用户的 `DatasetSelection` 决定。新增数据类型应进入统一策略；面板只选择数据集，不维护路径规则。
- 同步自身的身份、配对记录和配置，以及本机平台策略缓存，始终排除在同步范围之外。用户密钥、模型响应等敏感或重型数据需要用户显式选择；Agent 运行历史只同步终态运行。
- 本地同步写入与扩展安装、更新等变更共用互斥门禁，冲突时直接失败；只发送远端回拉请求的作业不占用本地写入许可。
- `Exact` 保持同步源权威；`PreferNewer` 保护目标端修改时间严格更新的同路径文件，依赖设备时钟基本同步。覆盖策略归逻辑发起方所有，贯穿手动与自动同步。
- Mirror 删除按计划在删除或提交阶段执行，目录清理只能发生在对应的数据集边界内。同步范围不支持符号链接。
- 每个文件原子发布并保留修改时间；增量判断依赖文件大小和修改时间。失败后已完成的文件保留，重试重新扫描并计算计划，不提供 bundle 字节级断点续传。
- 成功或部分失败导致本地数据变化时，必须刷新相关运行时缓存。能力、权限或数据集版本不满足要求时，在用户数据写入前失败；Sync Panel 要求 bundle 与 zstd 支持。

## LAN 发现与信任

发现只提供设备位置和公开信息，不建立信任，也不触发同步。附近设备、手动地址和配对链接共用配对流程，按需启动本机接收服务，并由对端用户确认；二维码是辅助入口。配对链接保持 v2 格式，旧 LAN v1 设备需要重新配对。

设备名称、平台和地址是可变信息，设备身份与信任独立保存。未固定证书的连接只能读取公开设备信息；配对和数据传输使用 SPKI 固定证书校验。已配对设备更换地址时仍须验证保存的指纹与 DeviceId，保留原有授权和身份。

LAN 上传保留发起方的覆盖策略，但实际回拉使用目标设备的有效 Sync mode；Mirror 删除的执行权在目标端。“请求已接受”与“数据同步完成”是不同结果。

本机广播随接收服务启停，只下载时可以独立发现。发现失败不删除配对或停用仍可用的 HTTPS 连接；停止接收服务不取消已接受的本地下载作业。

LAN 功能受宿主 `sync.lan` 能力门禁和平台权限约束。移动端权限、签名与生命周期要求见 [Android](../AndroidDevelopment.md) 和 [iOS](../iOSDevelopment.md)。

## 面板、事件与自动同步

面板通过宿主边界调用后端，传输与调度由 Rust 执行。已保存配置与编辑草稿分离，刷新不会提交或覆盖草稿；设备卡片和自动同步目标共用展示数据，本地备注不改变对端身份。

两种拓扑共用 `sync:job` 表达作业进度和结果。手动作业由命令返回的报告驱动完成提示，后台作业由事件通知，避免重复提示。发现变化与配对完成分开通知；配对完成通知只能在记录保存后发布。

自动同步只在应用进程运行时发起上传，使用独立保存的目标、范围和模式。TT-Sync 目标需要相应写入、删除权限；LAN 目标沿用对端回拉语义，只能将请求已接受记为已发出请求。自动任务不打开手动进度弹窗，也不强制刷新页面。

接收服务随应用启动和自动上传是独立设置；启用接收服务及发现不代表批准新设备配对。

## 源码入口

| 职责 | 入口 |
| --- | --- |
| 配对与配置用例 | [LAN 服务](../../src-tauri/crates/tt-application/src/services/lan_sync_service.rs)、[TT-Sync 服务](../../src-tauri/crates/tt-application/src/services/tt_sync_service.rs) |
| 作业与自动调度 | [作业协调器](../../src-tauri/crates/tt-application/src/services/sync_job_coordinator.rs)、[自动同步服务](../../src-tauri/crates/tt-application/src/services/sync_automation_service.rs) |
| 作业契约 | [共享 DTO 与事件](../../src-tauri/crates/tt-contracts/src/sync.rs) |
| 网络、存储与数据范围 | [共享同步实现](../../src-tauri/crates/tt-adapter-sync/src/sync/)、[TT-Sync 适配](../../src-tauri/crates/tt-adapter-sync/src/tt_sync/) |
| 发现协议与运行参数 | [LAN 发现](../../src-tauri/crates/tt-adapter-sync/src/sync/lan/peer_discovery.rs) |
| Tauri 集成 | [服务装配](../../src-tauri/crates/tauritavern/src/app/composition/services/sync.rs)、[事件与批准适配](../../src-tauri/crates/tauritavern/src/app/composition/adapters/tauri_sync.rs) |
| 前端交互 | [面板与 Controller](../../src/scripts/tauri/setting/sync-app/)、[宿主适配](../../src/scripts/tauri/setting/setting-panel/sync-popup.js) |
