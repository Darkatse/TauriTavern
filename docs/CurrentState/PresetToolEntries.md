# 预设工具条目

Prompt Manager 的条目可以直接编排请求中的工具回合：声明工具，预填工具调用，或把变量、状态与记忆等资料作为工具结果放进上下文。条目使用 TT 的请求中间格式，由 payload 构建器转换成各家协议；Legacy 生成、Agent Chat 与 Session 共用同一份预设。

## 条目

条目由角色、正文与 `fields` 组成。`fields` 是 JSON 对象，写正文以外的线路字段（包括签名、推理与原生内容），并入条目产出的对象；各格式读取哪些字段见[原生 API 格式](NativeApiFormats.md#3-工具回合与原生内容)。角色与正文由各自的控件负责，`fields` 不写 `role` 与 `content`。

| 条目 | 角色 | 产出 | 必需字段 |
| --- | --- | --- | --- |
| 消息 | `system`、`user`、`assistant` | `{ role, content, ...fields }` | — |
| 工具调用 | `assistant` | 同消息 | `tool_calls`：调用数组，每项有 `id` 与 `function.name` |
| 工具响应 | `tool` | 同消息 | `tool_call_id` |
| 工具定义 | `tool_definition` | `tools` 中的 `{ type: "function", function: { description: content, ...fields } }` | `name` |

工具调用是 `tool_calls` 非空的 assistant 条目。工具定义的正文就是工具描述，所以它的 `fields` 也不写 `description`。

```json
[
  { "identifier": "a1", "name": "读取状态", "role": "tool_definition", "content": "读取 {{char}} 的状态。",
    "fields": { "name": "read_state", "parameters": { "type": "object", "properties": { "target": { "type": "string" } } } } },
  { "identifier": "a2", "name": "预填调用", "role": "assistant", "content": "",
    "fields": { "tool_calls": [{ "id": "call_1", "type": "function", "function": { "name": "read_state", "arguments": { "target": "{{char}}" } } }] } },
  { "identifier": "a3", "name": "状态结果", "role": "tool", "content": "{{getvar::stat_data}}", "fields": { "tool_call_id": "call_1" } }
]
```

## 组装

`preparePrompt` 先求正文，再求 `fields`，两者共享本次组装的变量。`fields` 的键与字符串值展开宏，对象形式的 `arguments` 序列化为 JSON 字符串。角色卡覆盖 Main 或 Jailbreak 时只替换正文，`fields` 照常求值，`{{original}}` 只用于覆盖的正文。

请求无法表达条目时，组装停止并报出条目名称：`fields` 不是 JSON 对象、写了角色与正文的键、展开宏后键重复、缺少上表的必需字段，或带 `fields` 的条目放在“附着到已有消息”位置。其余情况照常发送，由服务商判定。试运行的错误显示在 Prompt Manager 顶部。

带 `fields` 的消息原样发送，不与其他消息合并，也不被改写。In-chat 中同深度、同顺序的带 `fields` 条目按列表顺序排在合并后的消息之前，作为一个预算单元，历史截断时一起保留或一起舍弃。工具定义不产生消息，按列表顺序进入 `tools`，启用状态、触发条件与角色专属顺序照常生效。

## 工具清单

预设定义与本次请求的可执行工具按模型调用名称合并：同名时在原位置替换，执行仍归原工具；其余追加在后，模型调用它们时得到工具错误。可执行工具在 Legacy 中是注册的函数工具与 Legacy MCP 工具，一次性工具请求（如输出修订）用它自带的工具；在 Agent 中是 Profile 绑定的工具。预设定义不受“启用函数调用”与模型能力判定的限制，也不设置 `tool_choice`。

Legacy 把工具数据放在 `generate_data.toolData` 交给发送，`GENERATE_AFTER_DATA` 的监听者可以读写它；Agent 快照把它放在 `generationParameters.tools`，并入规则见[工具](../Agent/ToolSystem.md#身份与调用名称)。

## 编辑

角色下拉在 System、User、AI Assistant 之外还有工具调用、工具响应与工具定义，工具调用保存为 `role: "assistant"`。标记以外的条目都有字段编辑框。

- 在消息、工具调用、工具响应与工具定义之间切换时，字段换成新类别的模板，原有字段非空时先确认。
- 编辑器用组装的检查即时校验（不展开宏），有错误时不能保存。
- 工具响应可以从本预设的工具调用中选择 `tool_call_id`。

## 兼容性

- 原版 SillyTavern 把工具调用条目当作普通 Assistant 条目，工具响应与工具定义会产生服务商拒绝的消息或被丢弃。
- JS-Slash-Runner 的预设接口重建条目时丢失 `fields`；它的 `generate` 只取消息，不带预设的工具定义。

## 修改入口

- 条目求值与报错：[`prompt-entry-fields.js`](../../src/scripts/prompt-entry-fields.js)，由 [`PromptManager.js`](../../src/scripts/PromptManager.js) 的 `preparePrompt` 调用；角色卡覆盖在 `openai.js` 的 `preparePromptsForChatCompletion`
- 编辑表单：`PromptManager.js` 的 `loadPromptIntoEditForm` 与 `updatePromptWithPromptEditForm`，表单在 [`index.html`](../../src/index.html) 的 `completion_prompt_manager_popup_edit`
- 消息、注入与预算：[`openai.js`](../../src/scripts/openai.js) 的 `Message`、`populationInjectionPrompts`、`populateChatHistory`
- 工具数据：`openai.js` 的 `createToolData`；交接在 [`script.js`](../../src/script.js) 的 `generate_data` 与 [`agent-prompt-snapshot.js`](../../src/tauri/main/api/agent-prompt-snapshot.js) 的 `createCurrentPromptSnapshotSeed`
