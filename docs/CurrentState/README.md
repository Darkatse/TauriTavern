# 当前现状说明

本目录用于记录 **已经落地** 的模块现状，而不是方案讨论或未来规划。

它解决的是一个很具体的问题：

> 当我们要继续开发某个模块时，首先需要知道系统现在实际上怎样工作、边界在哪、哪些约束不能轻易打破。

因此，本目录下的文档应保持简短，并优先回答以下问题：

1. 当前模块解决了什么问题
2. 端到端链路现在如何工作
3. 哪些能力已经支持，哪些明确不支持
4. 后续开发时最容易误改的契约是什么

## 与其他文档目录的分工

- `docs/CurrentState/`：当前实现快照与持续开发约束

## 当前条目

1. `docs/CurrentState/ThirdPartyExtensions.md`
   - 第三方前端扩展兼容的当前状态
   - 包含前端加载链路、后端资源端点、目录语义与开发约束

2. `docs/CurrentState/MobileStyleAdaptation.md`
   - 移动端样式适配现状（edge-to-edge / safe-area / 沉浸模式 / 第三方浮层兜底）
   - 包含 Android 原生注入链路、CSS 变量契约、前端消费与回归要点

3. `docs/CurrentState/EmbeddedRuntime.md`
   - 消息内 iframe runtime（JSR/LWB）的生命周期管控现状（budget/park/hydrate/自愈/渲染事务）
   - 包含端到端链路、支持/不支持边界与持续开发约束

4. `docs/CurrentState/StartupOptimization.md`
   - 开屏启动优化（Shell/Core/Full 分阶段启动）的当前实现快照
   - 包含前端启动编排、bootstrap 快照、扩展发现/激活、按需加载与可观测性约束

5. `docs/CurrentState/ChatPayload.md`
   - 聊天 payload 现状：canonical 完整历史、受限 DOM、完整原子提交与独立只读分页
   - 包含上游索引契约、扩展 API、fail-fast 错误边界与回归重点

6. `docs/CurrentState/MemoryExtensionApi.md`
   - 记忆类扩展 API（`window.__TAURITAVERN__.api.chat`）的当前落地状态：楼层语义、按需历史、后端定位、纯文本检索与持久化

7. `docs/CurrentState/BootstrapOptimization.md`
   - bootstrap / 启动链路中与冷启动内存基线相关的优化现状（如 tokenCache 避免 whole-load）

8. `docs/CurrentState/MediaAssetContract.md`
   - `<video>/<audio>` 依赖的全平台媒体资源契约现状（`Range`/`Content-Range`/Android WebView workaround）

9. `docs/CurrentState/Sync.md`
   - 同步（LAN Sync / TT-Sync v2）当前实现快照：链路、状态目录、协议与事件语义约束
   - 包含 TT-Sync bundle/zstd、断线重试语义与最易误改的契约清单

10. `docs/CurrentState/DataDirectorySelection.md`
   - 桌面端数据目录选择 / 启动期迁移的当前实现快照
   - 包含运行时引导配置、迁移恢复语义、effectively-empty 目录契约与持续开发约束

11. `docs/CurrentState/NativeApiFormats.md`
   - Custom 原生 API 格式兼容现状（OpenAI Responses / Claude Messages / Gemini Interactions）
   - 包含端到端链路、支持/不支持边界与持续开发约束（尤其回滚 ST、Responses continuation 与 thought-signatures）

12. `docs/CurrentState/iOSPolicy.md`
   - iOS/iPadOS-only 分发 Policy（profile + capabilities snapshot）当前实现快照
   - 包含 settings 契约、baseline 矩阵、后端裁决点与前端 UI 投影的维护约束
13. `docs/CurrentState/AgentFramework.md`
   - Agent 框架实时开发进度跟踪
   - 当前记录 canonical model IR、provider native metadata 保真、上下文只读工具、workspace 读改工具循环、前端 dryRun adapter、Host ABI、验证命令与后续限制；具体架构与细节设计见 `docs/AgentArchitecture.md`、`docs/AgentContract.md`、`docs/AgentImplementPlan.md` 与 `docs/Agent/`

14. `docs/CurrentState/AgentProviderState.md`
   - Agent `provider_state` 当前契约
   - 包含 run-scoped continuation、OpenAI Responses persistent WebSocket / incremental input / `previous_response_id`、内部字段剥离、native metadata fail-fast 与可观测性约束

15. `docs/CurrentState/CharacterIdentityContract.md`
   - 角色身份契约当前实现快照
   - 包含 `avatar_url` exact filename 契约、Rust stem key、chat directory alias/lazy resolver、rename/delete 当前语义与持续开发约束

16. `docs/CurrentState/LoggingObservability.md`
   - Logging / Dev Observability 当前实现快照
   - 包含 tracing 普通日志、用户可见 backend error、DevObservabilityHub、LLM API log 与边界守卫契约

17. `docs/CurrentState/HostResourceCaching.md`
   - Host Resource 的 opened source、representation revision、条件请求和平台 delivery 契约
   - 包含 weak ETag/Last-Modified/HEAD/Range/If-Range、Android 304 限制与持续开发约束

18. `docs/CurrentState/UpdateChannels.md`
   - Stable / Canary 更新检测与统一发布契约
   - 包含用户时间、机器 SHA、默认渠道、产物命名和可选 AI release notes 的边界

19. `docs/CurrentState/LinuxRepository.md`
   - APT、RPM 与 Nix 的分发现状
   - 包含支持范围、签名身份、安装入口、缓存配置和维护边界
