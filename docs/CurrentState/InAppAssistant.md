# 应用内助手

`in-app-agent` 是默认启用的系统扩展，在 A 抽屉提供独立 Session 对话，复用现有 Agent Runtime、工作区与工具链。用户禁用选择沿用扩展加载机制，不依赖写作 Agent System 扩展。

## 生命周期与配置

- 入口等待 Host ready、注册工具，再通过 APP_READY 回调挂载。不能在扩展顶层等待 APP_READY：上游在扩展加载完成后才发出该事件。
- 一个 React root 对应一个 controller；首次打开读取配置和历史，首次发送才创建 Session。隐藏抽屉保留视图与订阅，卸载释放订阅，不取消原生 Run。
- 初次配置引用已有 `openai/Default`，由用户选择模型；默认只开启 `app.evaluate`、`app.read_logs`，工作区与 Shell 工具按需选入。保存 Profile 影响下次发送，不改变运行中的冻结输入。
- `extension.store` 的 `in-app-agent` 命名空间保存 `currentSessionId` 与界面偏好 `contentWidthPercent`；Session 历史和共享 Profile 由 Session API 管理。缺失走各自初始状态，损坏数据与 IO 错误须暴露。
- Skill 使用助手 Profile 的作用域，与 Skill Manager 共用[导入准入](../API/Skill.md#预览与安装)。取消设置释放待确认来源，但不撤销已完成的安装；选择 Skill 不自动启用 Shell。

## 运行与历史

发送前先持久化 Session 指针；指针保存失败时保留已创建 ID。发送结果不确定时核对后端状态，不自动重发；输入草稿保留到发送受理成功。取消须等待后端终态，已发生的操作不回滚。

历史、进度和预览各有一个来源：

- `messages` 是后端保存的正式记录，按 Session seq 分页、补齐缺口和合并；`events` 只提供进度与终态，不重建对话。
- `responses` 是临时正文与可见思考。正式回复按 `runId + invocationId + round` 接替所有对应 attempt 的预览，不依赖事件与 Channel 的到达顺序。协议见 [Agent API](../API/Agent.md#控制与订阅)。
- 工具结果使用同一回合身份加 `callId` 关联；调用 ID 可以跨轮复用。无 origin 的记录按消息顺序关联，不能按整个 Run 的 callId 覆盖结果。

重开时以 `sessions.read().activeRun` 判断是否仍在运行；没有活动 Run 且缺少终态证据时显示 interrupted，不猜测成功或重放工具。

## 界面与工具边界

助手保留原高级格式节点和抽屉行为，包括主题透明度、模糊与移动端布局约束。桌面内容宽度默认 100%，比例只影响内部内容；移动端与窄窗口铺满。宽度是独立界面偏好，拖动预览、保存持久化、取消恢复。

界面拥有输入、展开状态和未保存设置，controller 拥有 Session IO 与订阅。键盘事件隔离于角色聊天；Markdown 使用独立 Showdown + DOMPurify，不进入聊天宏、regex 或脚本执行链。工具返回的资源引用不能直接当作工作区路径，完整结果仅按明确的外部化路径读取。

| Session 工具 | 契约 |
| --- | --- |
| `app.evaluate` | 在当前 WebView 执行一次 async function body，注入 `api`、`context`，显式返回 JSON；异常沿共同工具链传播，不换包装重跑。同步 JS 无法强制终止，超时或取消不撤销效果。 |
| `app.read_logs` | 读取保留日志，先按等级筛选再取尾部，并返回采集开关状态；不修改采集设置或清除日志。 |

## 维护入口

源码位于 [in-app-agent/src](../../src/scripts/extensions/in-app-agent/src)：`index.ts`、`drawer.ts` 负责接入；`controller.ts`、`session-state.ts` 负责会话状态；`host.ts` 组合公共 API 与视图 actions。界面分层见 [First-party UI](FirstPartyUI.md)，验证入口见 [Agent 测试](../Agent/TestingStrategy.md)。
