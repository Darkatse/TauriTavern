# 模块文档

这里记录各模块的工作方式与源码入口。Agent 的整体说明集中在 [Agent](../Agent/README.md)，后端分层见 [后端结构](../BackendStructure.md)。

| 模块 | 文档内容 |
| --- | --- |
| [第三方扩展](ThirdPartyExtensions.md) | 加载、发现与资源访问 |
| [移动端样式](MobileStyleAdaptation.md) | WebView、safe area 与浮层适配 |
| [消息内嵌运行时](EmbeddedRuntime.md) | iframe 的生命周期与渲染 |
| [启动流程](StartupOptimization.md) | Shell、Core、Full 阶段 |
| [Bootstrap](BootstrapOptimization.md) | 启动输入与内存开销 |
| [聊天存储](ChatPayload.md) | 完整历史、分页读取和保存 |
| [记忆扩展](MemoryExtensionApi.md) | 聊天查询与扩展存储 |
| [媒体资源](MediaAssetContract.md) | 浏览器资源与 Range 请求 |
| [同步](Sync.md) | LAN Sync 与 TT-Sync |
| [数据目录](DataDirectorySelection.md) | 桌面目录选择与迁移 |
| [原生模型 API](NativeApiFormats.md) | Responses、Claude、Gemini 等格式 |
| [iOS 策略](iOSPolicy.md) | 平台能力与分发设置 |
| [角色身份](CharacterIdentityContract.md) | 角色、聊天目录与重命名 |
| [日志](LoggingObservability.md) | tracing、Dev API 与 LLM 请求日志 |
| [资源缓存](HostResourceCaching.md) | 资源版本与条件请求 |
| [更新渠道](UpdateChannels.md) | Stable、Canary 与更新检测 |
| [Linux 分发](LinuxRepository.md) | APT、RPM 与 Nix |
| [Windows 分发](WindowsDistribution.md) | WinGet 与安装包 |
| [MCP](MCP.md) | 服务器、工具目录与模型调用 |
| [向量 API](VectorApi.md) | 索引、embedding 与查询 |
| [自有界面](FirstPartyUI.md) | React、TypeScript 与构建流程 |
