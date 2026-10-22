# 应用内助手

`in-app-agent` 是默认启用的系统扩展，在 A 抽屉提供独立 Session 对话，复用现有 Agent Runtime、工作区与工具链。用户禁用选择沿用扩展加载机制，不依赖写作 Agent System 扩展。

## 生命周期与配置

- 入口等待 Host ready、注册工具，再通过 APP_READY 回调挂载。不能在扩展顶层等待 APP_READY：上游在扩展加载完成后才发出该事件。
- 一个 React root 对应一个 controller；首次打开读取配置和历史，隐藏保留视图与订阅，卸载释放订阅但不取消原生 Run。
- 首次选择模型时保存 Profile，引用内置 `openai/Agent` 预设，默认开启 `app.snapshot`、`app.interact`、`app.capture`、`app.evaluate`、`app.read_logs` 及 `workspace.read_file`、`workspace.list_files`、`workspace.search_files`；其他工具按需选入。预设取值与取舍见[内置 Agent 预设](../Agent/ProfilesAndPreset.md#内置-agent-预设)。已有 Profile 保留用户选择与指令，新工具从助手设置显式启用。
- 输入框中的模型与推理强度选择立即保存，并同步到未保存的设置草稿；修改影响下次发送，不改变运行中的冻结输入。
- 会话选择保存在 `localStorage`：null 表示新对话，未保存或目标已删除时打开最近会话。`extension.store` 保存宽度偏好；Session 数据与共享 Profile 由 Session API 管理。
- Skill 来自内置与助手 Profile 作用域，同名时 Profile 覆盖内置包。新 Profile 默认 `visible: ['*']`，取消勾选加入 `deny`；已有显式名单保持原样，设置按相同的可见性规则显示和修改。安装与 Skill Manager 共用[导入准入](../API/Skill.md#预览与安装)。取消设置释放待确认来源，但不撤销已完成的安装；选择 Skill 不自动启用 Shell。
- 内置知识位于 [default/skills/](../../default/skills/)，正文按需读取；内置包只读，来源与宏规则见 [Skill](../Agent/Skill.md)。

## 运行与历史

当前会话与活动任务独立；切换不取消任务，助手同时只发送一个任务，公共 Session API 仍按会话准入。各会话草稿仅存于内存；新对话共用一个空白草稿，首次发送才创建 Session。

受理成功才清除源草稿；发送结果不确定时保留会话与草稿，核对后端状态，不自动重发。取消须等待后端终态，不回滚已发生的操作。

历史、进度和预览各有一个来源：

- `messages` 是后端保存的正式记录，包含压缩记录，按 Session seq 分页、补齐缺口和合并；`events` 只提供进度与终态，不重建对话。
- `responses` 是临时正文与可见思考，正式记录按模型回合身份接替预览，协议见 [Agent API](../API/Agent.md#控制与订阅)。
- 工具结果使用同一回合身份加 `callId` 关联；调用 ID 可以跨轮复用。无 origin 的记录按消息顺序关联，不能按整个 Run 的 callId 覆盖结果。

运行状态以后端活动 Run 为准；没有活动 Run 且缺少终态证据时显示 interrupted，不重放工具。当前 Run 是活动 Run，否则是最新消息所属的 Run；刚观察结束、晚于最新消息的 Run（如失败的压缩）在出现更新的消息之前仍是当前 Run。

上下文达到预设预算的压缩点或服务商报告超窗时，由 runtime 自动压缩，规则见[运行循环](../Agent/Runtime.md#session-上下文压缩)。状态条显示正在压缩；Transcript 在所属 Run 的工作条目之间显示“已压缩上下文”和前后 token 数，展开可读摘要。压缩记录不显示为用户气泡，也不触发新用户消息的自动跟随；原始对话仍保留在历史中。输入框的上下文用量环显示当前 Run 最近一次计数占压缩点的比例，计数来自运行事件，前端不分词；菜单中的“立即压缩”在助手空闲时启动一次压缩 Run。Run 中途压缩需要页面在线；页面重载后，宿主会恢复仍活动 Run 的组装应答。

## 界面与工具边界

保留原高级格式节点和抽屉行为，包括主题透明度、模糊与移动端约束。桌面助手向下覆盖聊天输入区，内容宽度默认 100%；宽度设置只影响内部内容，移动端与窄窗口铺满。

模型菜单随 Connection Manager 的 Model Target 变化更新；绑定不可用时必须重新选择。推理强度展示配置档位，参数语义见 [Profile](../Agent/ProfilesAndPreset.md#选择提示词和模型)。

消息按 Run 分组，中间回合、工具与思考可折叠，最终答案始终可见；当前与最新 Run 默认展开。

历史页只读目录，正文按所选会话分页；旧会话的读取结果不能覆盖新视图。键盘事件隔离于角色聊天，内层控件以 `preventDefault` 消费 Escape，抽屉不重复处理。Markdown 使用独立 Showdown + DOMPurify，不进入聊天宏、regex 或脚本执行链。工具结果按明确的外部化路径读取；图片按 `result.images` 中的路径与 MIME 类型读取，工具详情展开时加载缩略图，卸载时释放 object URL。

| Session 工具 | 契约 |
| --- | --- |
| `app.snapshot` | 按需读取主页面及同源 iframe 的有限 DOM 语义快照，支持区域下钻、已知 CSS selector 直接定位与分页；直接定位沿用观察边界且要求唯一可观察匹配，不展开、滚动或唤醒页面。 |
| `app.capture` | 原生截取内容视口，在页面内裁剪、遮挡与标号；敏感字段与宿主 omit 区域沿用 `app.snapshot` 的判定，CodeMirror 等技术省略项仍可见。仅在模型接受图片时提供，标号来自最近一页快照。 |
| `app.interact` | 用最新 ref 或已知 selector 操作控件，定位后共用检查与执行链路；返回派发情况与即时状态。拒绝操作时区分隐藏、视口外与检测点未命中，提供实际命中元素及坐标所属页面；可恢复错误不结束任务，读回不证明异步保存完成。 |
| `app.evaluate` | 在当前 WebView 执行一次 async function body，注入 `api`、`context`，显式返回 JSON；异常沿共同工具链传播，不换包装重跑。同步 JS 无法强制终止，超时或取消不撤销效果。 |
| `app.read_logs` | 读取保留日志，先按等级筛选再取尾部，并返回采集开关状态；不修改采集设置或清除日志。 |
| `workspace.read_file` / `workspace.list_files` / `workspace.search_files` | 通过统一工作区视图读取、列出和搜索会话材料及有效 Skill；不授予写入或 Shell 权限。 |

读取和操作界面的引擎来自 [@tauritavern/app-use](../../packages/app-use/README.md)。扩展通过既有 Extension Provider 注册工具，并把 TT 特有的部分交给引擎：敏感标记 `[data-tt-sensitive]`、要省略的助手历史区、控件外壳，以及 `#chat` 的滚动。`evaluate` 的执行和 `read_logs` 由扩展实现。正文与思考按区域合并为有界预览，保留其中的控件，省略的文本不进入分页；快照不提供完整业务数据。

交互光标及其样式也来自这个包。光标展示实际通过命中检查的位置，沿用主题并尊重减少动态效果设置；动画不阻塞工具执行，生命周期跟随助手活动任务，结束或页面卸载时移除。

iframe 默认只展示入口，使用其 ref 作为 root 进入；`snapshot({})` 返回主页面。每页属于一个文档，交互逐层检查宿主遮挡并将光标位置映射到主页面；暂停恢复入口由 Embedded Runtime 提供。

引用只对当前 Run 最近一页有效；新快照、Run 切换、页面或祖先 iframe 重载后旧引用失效。分页读取实时界面，interact/evaluate 会使旧续读位置失效；关闭助手抽屉不影响工具运行。具体参数、支持范围与恢复指引以包内的[工具定义](../../packages/app-use/src/tools.ts)为准，TT 专属说明由扩展的 [tools.ts](../../src/scripts/extensions/in-app-agent/src/tools.ts) 补充。

## 维护入口

源码位于 [in-app-agent/src](../../src/scripts/extensions/in-app-agent/src)：`index.ts`、`drawer.ts` 负责接入；`controller.ts`、`session-state.ts` 负责会话状态；`host.ts` 适配公共 API 与页面能力；`ComposerMenu.tsx` 统一输入框菜单交互。界面分层见 [First-party UI](FirstPartyUI.md)，验证入口见 [Agent 测试](../Agent/TestingStrategy.md)。
