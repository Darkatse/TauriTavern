# AI Horde

AI Horde 是志愿者运行的众包网络，主机固定为 `aihorde.net`。没有密钥时匿名使用，排在最后；有密钥时按 kudos 排队。宿主实现文本主 API（`koboldhorde`）；生图与图片描述的 Horde 来源尚未实现。页面的提示词排布、预设与采样器沿用 KoboldAI 的设置。

## 宿主的职责

- Horde 的协议规则只在 `horde_service`：轮询间隔与上限、状态判定、列表缓存、模型参考表的合并。端口按 Horde 的资源命名方法，路径、请求头与 HTTP 状态的含义在适配器。
- 每个请求带 `Client-Agent`，写 TauriTavern 的名字、版本与仓库地址。密钥取当前选中的一条，没有时用匿名密钥。
- 主机固定，不经[用户端点](UserEndpointAccess.md)确认。

## 路由

- `text-workers`、`text-models`、`status`、`user-info` 的形状与上游相同。worker 与模型列表缓存 60 秒，`force` 时重新读取；模型参考表只用来标记白名单，取不到时照常列出模型。`user-info` 查共享密钥时，Horde 答 400、401 或 404 表示这个密钥不是共享密钥，其他失败照常报错。
- 文本生成由 `/api/backends/koboldhorde/generate` 提供，即页面 `getGenerateUrl` 为 Horde 写的地址。上游的 `generate-text`、`task-status`、`cancel-task` 不提供，轮询在宿主。

## 生成

- 命令先提交任务，提交失败时在流开始前失败。之后生成运行时的流会话每 2.5 秒查一次状态，把 Horde 报告的状态原样作为事件；带生成结果的状态之后是 `[DONE]`。任务出错、没有 worker 能接、完成却没有结果、20 分钟未完成，各自以错误结束。
- 取消时删除任务。流会话、后台租约与完成通知与[文本补全](TextCompletion.md)共用；Horde 的生成不记入 LLM API 日志。
- `generateHorde` 读状态事件，按首个排队位置显示进度。错误块写成带生成结果的最终状态，所以非 quiet 的失败成为角色回复；quiet 的失败让流本身出错，`generateHorde` 随之抛错。

## iOS

`ai.horde` 关闭时，服务拒绝全部 Horde 请求，主 API 选择器去掉 Horde。见 [iOS 策略](iOSPolicy.md)。

## 修改入口

- 协议、缓存与轮询：[`horde_service/`](../../src-tauri/crates/tt-application/src/services/horde_service)，文本任务在 `text.rs`
- 传输：[`http_horde_repository.rs`](../../src-tauri/crates/tt-adapter-provider-http/src/http_horde_repository.rs)
- 路由：[`horde-routes.js`](../../src/tauri/main/routes/horde-routes.js)；页面：[`horde.js`](../../src/scripts/horde.js)
- 主 API 的可用集合：[`main-api-selector-option-parking.js`](../../src/tauri/main/adapters/st/main-api-selector-option-parking.js)
