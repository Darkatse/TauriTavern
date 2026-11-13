# 预设工具条目

Prompt Manager 有三种工具条目：工具定义、工具调用与工具响应，用来直接编排请求中的 `tools` 与工具消息。条目使用 TT 的请求中间格式，由 payload 构建器翻译成各家协议；Legacy 生成、Agent Chat 与 Session 共用同一份预设。编辑器尚未提供这三种角色，目前通过导入预设 JSON 创建。

## 条目与 fields

| 条目 | 存储 | 产出 |
| --- | --- | --- |
| 工具定义 | `role: "tool_definition"` | `tools` 中的 `{ type: "function", function: { name, description: content, ... } }` |
| 工具调用 | `role: "assistant"`，`fields.tool_calls` 非空 | `{ role: "assistant", content, ...fields }` |
| 工具响应 | `role: "tool"` | `{ role: "tool", content, ...fields }` |

`fields` 是一个 JSON 对象，并入条目产出的对象。角色与正文由各自的控件负责，`fields` 不能包含 `role`、`content`，工具定义的 `fields` 也不能包含 `description`。签名、推理与原生内容同样写在 `fields` 中，各格式读取哪些字段见[原生 API 格式](NativeApiFormats.md#3-工具回合与原生内容)。

```json
[
  { "identifier": "a1", "name": "读取状态", "role": "tool_definition", "content": "读取 {{char}} 的状态。",
    "fields": { "name": "read_state", "parameters": { "type": "object", "properties": { "target": { "type": "string" } } } } },
  { "identifier": "a2", "name": "预填调用", "role": "assistant", "content": "",
    "fields": { "tool_calls": [{ "id": "call_1", "type": "function", "function": { "name": "read_state", "arguments": { "target": "{{char}}" } } }] } },
  { "identifier": "a3", "name": "状态结果", "role": "tool", "content": "{{getvar::stat_data}}", "fields": { "tool_call_id": "call_1" } }
]
```

## 求值与报错

`preparePrompt` 先求正文，再求 `fields`，两者共享本次组装的变量。`fields` 的键与字符串值展开宏，展开后键重复时报错；对象形式的 `arguments` 序列化为 JSON 字符串。

请求无法表达条目时，组装停止并报出条目名称：

- `fields` 不是 JSON 对象，或包含角色与正文负责的键；
- 工具调用的 `tool_calls` 不是调用数组，或某项缺少 `id`、`function.name`；
- 工具响应缺少 `tool_call_id`，工具定义缺少 `name`；
- 带 `fields` 的条目放在“附着到已有消息”位置。

其余情况照常发送，由服务商判定。试运行中的错误显示在 Prompt Manager 顶部。

角色卡覆盖 Main 或 Jailbreak 时只替换正文，`fields` 照常求值，`{{original}}` 只用于覆盖的正文。

## 位置与预算

- 带 `fields` 的消息不参与合并或改写：In-chat 不与其他条目合并，合并 system 消息与 Moonshot 隐藏预填推理也跳过它们。
- In-chat 同深度、同顺序的带 `fields` 条目按列表顺序排在合并后的消息之前，并作为一个预算单元，历史被截断时一起放入或一起舍弃。
- 工具定义不产生消息，按列表顺序进入 `tools`；启用状态、触发条件与角色专属顺序照常生效。

## 工具清单

可执行工具在 Legacy 中是注册的函数工具与 Legacy MCP 工具，一次性工具请求（如输出修订）使用它自带的工具；在 Agent 中是 Profile 绑定的工具。预设定义按模型调用名称与之合并：同名时在原位置替换，执行仍归原工具；其余追加在后，模型调用它们时得到工具错误。预设定义不受“启用函数调用”与模型能力判定的限制，也不设置 `tool_choice`。

工具数据与消息同属组装结果：Legacy 由 `generate_data.toolData` 交给发送，`GENERATE_AFTER_DATA` 的监听者可以读写它；Agent 快照把它放在 `generationParameters.tools`，合并规则见[工具](../Agent/ToolSystem.md#身份与调用名称)。

## 写法建议

工具结果适合呈现资料，例如把变量、状态与记忆作为读取工具的结果放进上下文；指令仍放在 system 或 user 消息里。服务商对工具回合的额外要求，如思维签名、思考模式要求回传的 `reasoning_content`，由创作者写在 `fields` 中。

## 兼容性

- 原版 SillyTavern 不认识这三种条目：工具调用条目按普通 Assistant 条目处理，另两种会产生服务商拒绝的消息或被丢弃。
- JS-Slash-Runner 的预设接口重建条目时丢失 `fields`；它的 `generate` 只取消息，不带预设的工具定义。

## 修改入口

- 条目求值与报错：[`prompt-entry-fields.js`](../../src/scripts/prompt-entry-fields.js)，由 [`PromptManager.js`](../../src/scripts/PromptManager.js) 的 `preparePrompt` 调用；角色卡覆盖在 `openai.js` 的 `preparePromptsForChatCompletion`
- 消息、注入与预算：[`openai.js`](../../src/scripts/openai.js) 的 `Message`、`populationInjectionPrompts`、`populateChatHistory`
- 工具数据：`openai.js` 的 `createToolData`；交接在 [`script.js`](../../src/script.js) 的 `generate_data` 与 [`agent-prompt-snapshot.js`](../../src/tauri/main/api/agent-prompt-snapshot.js) 的 `createCurrentPromptSnapshotSeed`
