# 应用内助手

`in-app-agent` 是默认启用的系统扩展，在 A 抽屉提供独立 Session 对话，复用现有 Agent Runtime、工作区与工具链。用户禁用选择沿用扩展加载机制，不依赖写作 Agent System 扩展。

## 生命周期与配置

- 入口等待 Host ready、注册工具，再通过 APP_READY 回调挂载。不能在扩展顶层等待 APP_READY：上游在扩展加载完成后才发出该事件。
- 一个 React root 对应一个 controller；首次打开读取配置和历史，隐藏保留视图与订阅，卸载释放订阅但不取消原生 Run。
- 初次配置引用已有 `openai/Default`，由用户选择模型；默认只开启 `app.evaluate`、`app.read_logs`，工作区与 Shell 工具按需选入。保存 Profile 影响下次发送，不改变运行中的冻结输入。
- 会话选择保存在 `localStorage`：null 表示新对话，未保存或目标已删除时打开最近会话。`extension.store` 保存宽度偏好；Session 数据与共享 Profile 由 Session API 管理。
- Skill 使用助手 Profile 的作用域，与 Skill Manager 共用[导入准入](../API/Skill.md#预览与安装)。取消设置释放待确认来源，但不撤销已完成的安装；选择 Skill 不自动启用 Shell。

## 运行与历史

当前会话与活动任务独立；切换不取消任务，助手同时只发送一个任务，公共 Session API 仍按会话准入。各会话草稿仅存于内存；新对话共用一个空白草稿，首次发送才创建 Session。

受理成功才清除源草稿；发送结果不确定时保留会话与草稿，核对后端状态，不自动重发。取消须等待后端终态，不回滚已发生的操作。

历史、进度和预览各有一个来源：

- `messages` 是后端保存的正式记录，按 Session seq 分页、补齐缺口和合并；`events` 只提供进度与终态，不重建对话。
- `responses` 是临时正文与可见思考，正式记录按模型回合身份接替预览，协议见 [Agent API](../API/Agent.md#控制与订阅)。
- 工具结果使用同一回合身份加 `callId` 关联；调用 ID 可以跨轮复用。无 origin 的记录按消息顺序关联，不能按整个 Run 的 callId 覆盖结果。

运行状态以后端活动 Run 为准；没有活动 Run 且缺少终态证据时显示 interrupted，不重放工具。

## 界面与工具边界

保留原高级格式节点和抽屉行为，包括主题透明度、模糊与移动端约束。桌面宽度默认 100%，设置只影响内部内容；移动端与窄窗口铺满。

历史页只读目录，正文按所选会话分页；旧会话的读取结果不能覆盖新视图。键盘事件隔离于角色聊天；Markdown 使用独立 Showdown + DOMPurify，不进入聊天宏、regex 或脚本执行链。工具结果仅按明确的外部化路径读取。

| Session 工具 | 契约 |
| --- | --- |
| `app.evaluate` | 在当前 WebView 执行一次 async function body，注入 `api`、`context`，显式返回 JSON；异常沿共同工具链传播，不换包装重跑。同步 JS 无法强制终止，超时或取消不撤销效果。 |
| `app.read_logs` | 读取保留日志，先按等级筛选再取尾部，并返回采集开关状态；不修改采集设置或清除日志。 |

## 维护入口

源码位于 [in-app-agent/src](../../src/scripts/extensions/in-app-agent/src)：`index.ts`、`drawer.ts` 负责接入；`controller.ts`、`session-state.ts` 负责会话状态；`host.ts` 组合公共 API 与视图 actions。界面分层见 [First-party UI](FirstPartyUI.md)，验证入口见 [Agent 测试](../Agent/TestingStrategy.md)。
