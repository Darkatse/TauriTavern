# 原生 API 格式（Custom）兼容现状

最后更新：2026-04-10

本文件描述 **TauriTavern 已落地** 的三家原生 API 格式兼容（OpenAI Responses / Claude Messages / Gemini Interactions）的当前实现快照与持续开发约束。

目标边界（回滚兼容）：
- “倒回 SillyTavern”后，ST 1.16.0 **能启动且设置不崩** 即可（不追求 ST 原生理解新字段）。

---

## 1. 当前解决了什么问题

在保持前端尽量沿用 SillyTavern 语义（`chat.completion` / tool loop / 事件流）前提下，为 `Custom` 入口新增三种“原生协议”变体：

- **OpenAI Responses**：`/v1/responses`（支持 stream + tool calling）
- **Claude Messages**：`/v1/messages`（Custom 变体默认不注入 `anthropic-beta`，适配第三方兼容端）
- **Gemini Interactions**：`/v1beta/interactions`（支持 stream + tool calling + thought signature/native blocks 回放）

核心原则：
- **协议复杂度集中在 Rust 后端 translator + normalizer**；前端尽量只做最小选择/回显与少量解析分支。
- **配置回滚友好**：落盘仍以 `chat_completion_source=custom` 为主，新能力通过新增字段 `custom_api_format` 扩展。

---

## 2. 端到端链路现在如何工作

### 2.1 配置与选择（前端）

UI：OpenAI 设置的 `Chat Completion Source` 增加 3 个选项：
- `Custom (OpenAI Responses)`
- `Custom (Claude Messages)`
- `Custom (Gemini Interactions)`

落盘语义（关键契约）：
- 任何 “Custom (*)” 变体最终都落到：
  - `oai_settings.chat_completion_source = "custom"`
  - `oai_settings.custom_api_format ∈ {"openai_compat","openai_responses","claude_messages","gemini_interactions"}`

这保证把配置文件拷回 ST 1.16.0 时：
- `chat_completion_source` 仍是 ST 已知的 `custom`
- `custom_api_format` 作为“未知字段”被 ST 忽略（但不应导致设置页崩溃）

Connection Profiles（Connection Manager 扩展）：
- profile 中的 `api` 对 Custom 统一记录为 `custom`，避免把 UI 变体值写入配置造成回滚风险。
- Custom 变体由单独字段 `custom-api-format` 记录与回放（等价于执行 `/custom-api-format <format>`）。

自定义端点预览（UI 文案）：
- 端点预览只展示 **当前所选格式** 的最终 endpoint（base URL + suffix），并保留“末尾加 `/v1` 试试”的提示。
- suffix 映射：OpenAI-compatible→`/chat/completions`，Responses→`/responses`，Claude→`/messages`，Gemini→`/interactions`。

### 2.2 请求构建（Rust payload builder）

后端入口仍按“OpenAI 兼容 generate payload”接收前端数据，在 `payload/custom.rs` 中按 `custom_api_format` 分流：
- `openai_compat` → 走现有 `/chat/completions` 兼容构造，并应用 include/exclude overrides
- `openai_responses` → 构造 `/responses`
- `claude_messages` → 复用 Claude Messages 构造，并应用 include/exclude overrides
- `gemini_interactions` → 构造 `/interactions`

### 2.3 HTTP 调用 + Stream 处理（Rust repository）

仓库层对 `ChatCompletionSource::Custom` 以 **endpoint_path** 再分流（`http_chat_completion_repository/mod.rs`）：
- `/responses` → OpenAI Responses repository（语义 SSE → 归一化 chunk）
- `/interactions` → Gemini Interactions repository（语义 SSE → 归一化 chunk）
- `/messages` → Claude repository（沿用 Claude 的事件流语义）
- 其他 → Custom OpenAI-compatible（`/chat/completions`）

> 备注：Claude 的 streaming 仍保持“Anthropic 事件流 JSON”语义；Responses/Interactions streaming 则统一归一化为 OpenAI `chat.completion.chunk`。

---

## 3. 已支持能力 / 明确不支持

### 3.1 能力矩阵（当前）

| Custom 变体 | 非流式 | 流式 | tool calling | thought signature / native blocks | 回滚 ST 启动 |
|---|---:|---:|---:|---:|---:|
| OpenAI-compatible (`/chat/completions`) | ✅ | ✅ | ✅（上游 ST 语义） | ✅（现有链路） | ✅ |
| OpenAI Responses (`/responses`) | ✅（normalize→chat.completion） | ✅（SSE→chat.completion.chunk） | ✅（含 follow-up） | ⚠️（未落盘 response_id/native） | ✅ |
| Claude Messages (`/messages`) | ✅（normalize→chat.completion） | ✅（Anthropic events） | ✅（沿用 Claude tool loop） | ✅（现有链路） | ✅ |
| Gemini Interactions (`/interactions`) | ✅（normalize→chat.completion，含 native） | ✅（SSE→chat.completion.chunk，末包带 native） | ✅ | ✅（`message.extra.native` 回放 outputs） | ✅ |

### 3.2 明确的当前限制

- **Custom OpenAI Responses：`previous_response_id` 映射为内存缓存**（call_id → response_id）。应用重启后无法继续依赖旧 call_id 做 follow-up（需要重新生成 tool call）。
- **Custom 的 model list / status check** 仍以 OpenAI-compatible 的方式为主，Responses/Interactions 没有单独的 list_models 路由（后续如需可做 Phase 4 增强）。
- **Claude streaming 不做 chunk 归一化**：前端需走 Anthropic events 分支解析（现状就是如此，优先复用既有 Claude 语义）。

---

## 4. 三家实现要点（对持续开发最关键的部分）

### 4.1 OpenAI Responses（/responses）

请求侧（payload）：
- `messages[]` → `input[]` items；`system` → `developer`
- 仅将 **尾部连续的 tool messages** 映射为 `function_call_output`（需要 `tool_call_id` → `call_id`）
- 固定 `store=false`；tool follow-up 依赖 `previous_response_id`（现按实现假定在 `store=false` 仍可用）

流式侧（repository）：
- 解析 Responses 语义事件（如 `response.output_text.delta` / `response.output_item.added` / `response.function_call_arguments.delta`）
- 输出 OpenAI `chat.completion.chunk`：
  - 文本 delta → `choices[0].delta.content`
  - 推理 delta → `choices[0].delta.reasoning_content`
  - tool call delta → `choices[0].delta.tool_calls[]`（`id` 使用 Responses 的 `call_id`）

tool follow-up（关键契约）：
- 从 tool outputs（`function_call_output`）提取 call_id → 查内存缓存得到 `previous_response_id`
- follow-up 请求仅发送 “上一条 assistant 后的 tail items”，并注入 `previous_response_id`

### 4.2 Gemini Interactions（/v1beta/interactions）

URL 与鉴权：
- 若 `custom_url` 末尾不含 `/v1` 或 `/v1beta`，后端自动补 `.../v1beta`
- streaming 自动加 `?alt=sse`
- 未提供 `Authorization` 时使用 `x-goog-api-key`，并（当 key 非空）同时追加 query `key=...`

signature / native blocks（关键契约）：
- 后端在 streaming 完成事件 `interaction.complete` 时，将聚合后的 `outputs[]` 放入：
  - `choices[0].delta.native = { gemini_interactions: { outputs } }`
- 前端在保存消息时将其落到 `message.extra.native`
- 后续构造 stateless history 时：若 `extra.native.gemini_interactions.outputs` 存在，则 **原样回放** outputs（满足 thought-signatures 相关要求）

流式归一化：
- Interactions SSE 的 `content.delta`：
  - `text` → `delta.content`
  - `thought_summary` → `delta.reasoning_content`
  - `function_call` → `delta.tool_calls`（arguments 为 JSON 字符串；tool_call_id 视为不透明字符串）

### 4.3 Claude Messages（/messages，Custom 变体）

header 策略（关键契约）：
- **Custom Claude Messages 默认不自动添加 `anthropic-beta`**，避免第三方兼容端报错。
- 用户可通过“附加参数/附加标头”自行传入 `anthropic-beta`（如确实需要）。

streaming 语义：
- 后端沿用 Claude 的 SSE `data:` JSON 事件透传（不做 chunk 归一化）
- 前端对 `custom_api_format=claude_messages` 走 Claude streaming 分支解析（提取 `delta.text`/`delta.thinking`）

---

## 5. 最容易误改的契约（请勿破坏）

1. **回滚兼容**：Custom 变体落盘必须保持 `chat_completion_source="custom"`，不要把 UI 选择值（如 `custom_openai_responses`）写入设置文件。
2. **tool_call_id 透明性**：tool loop 不应假设 tool_call_id 是 OpenAI UUID；必须把它当作不透明字符串传递与存储。
3. **native metadata 保真**（Gemini）：`message.extra.native` 必须端到端保存/回放，且不得“清洗未知字段”（否则签名链会断）。
4. **Custom Claude 不注入 anthropic-beta**：该行为是为了兼容第三方；如需默认开启，应以显式配置而不是硬编码回退。
