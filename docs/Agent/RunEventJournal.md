# 运行日志

`events.jsonl` 是一个 Run 的追加日志。它保存已经发生的步骤，使 Timeline 和历史查询可以在页面关闭后重新建立运行轨迹。

## 一条事件

每行是一个 JSON 对象：

```json
{
  "seq": 1,
  "id": "evt_...",
  "runId": "run_...",
  "timestamp": "2026-09-06T12:00:00Z",
  "level": "info",
  "type": "run_started",
  "payload": {}
}
```

`seq` 从 1 连续递增，是分页和追踪新事件的游标。同一个 Run 的所有 Invocation 共用这条序列。追加由仓储串行处理，已有事件保持原样。

事件保存阶段信息和文件引用。工具参数、完整结果和模型响应单独保存，避免每次读取 Timeline 都搬运完整上下文。常用事件可按一次运行理解：

```text
run_started
  → workspace_initialized
  → model_request_created → model_completed
  → tool_call_requested → tool_call_completed / tool_call_failed
  → workspace_file_written / workspace_patch_applied
  → chat_commit_requested → chat_commit_completed
  → run_completed / run_partial_success / run_failed / run_cancelled
```

这张图说明主要步骤；一次运行通常有多轮调用，期间还会记录状态变化、委派、交接和用户补充指令。

## 读取与展示

`api.agent.readEvents({ runId, afterSeq, limit })` 读取游标之后的事件。`beforeSeq` 读取指定序号之前最近的一页，结果仍按序号升序排列。`api.agent.subscribe()` 在此基础上持续追踪新事件。

传入 `invocationId` 可以读取某个 Agent 的过程。事件的 `payload.eventScope` 指出主要和相关 Invocation，后端按这些归属筛选后再分页。

Timeline 还需要知道完整的 Agent 关系。`includeTimelineProjection: true` 会附带由 Invocation 和 Task 记录生成的关系图；它独立于当前事件页。任务详情通过 `readTaskDetail()` 读取，模型回合通过 `readModelTurn()` 读取，因此页面无需碰内部文件格式。

实时工具参数和推理文字预览由 `subscribeLiveProjection()` 提供。折叠时只渲染三行尾部预览，展开后显示完整已接收内容并自然增高；用户展开阅读时暂停自动贴底。历史事件继续按固定行高虚拟化，少量实时块独立使用自然高度。这是内存中的显示状态，最终执行结果仍从日志读取。Timeline 默认省略自动提交的进度项，原始日志继续保留这些记录。

## 可以重建什么

日志支持回看模型与工具步骤、关联文件、统计提交结果和解释终态。运行列表的摘要是可重建缓存，终态 Run 可以复用这份摘要。

完整正文依赖引用的文件。历史清理后，核心日志可能仍在，而模型响应或任务结果已被清理。当前日志也不用于重新执行工具、恢复挂起的 Run 或还原任意时刻的文件内容。

新增事件时，在拥有该动作的 runtime 路径记录，携带能定位这次动作的 Run、Invocation 或 Task 信息。较大的内容继续放到文件中。

## 源码

- [journal.rs](../../src-tauri/crates/tt-application/src/services/agent_runtime_service/journal.rs)：事件归属与状态记录。
- [run_store.rs](../../src-tauri/crates/tt-adapter-storage-userdata/src/repositories/file_agent_repository/run_store.rs)、[event_journal.rs](../../src-tauri/crates/tt-adapter-storage-userdata/src/repositories/file_agent_repository/event_journal.rs)：追加与分页读取。
- [agent_run_history_service.rs](../../src-tauri/crates/tt-application/src/services/agent_run_history_service.rs)：历史摘要和清理。
- [run-event-presenter.ts](../../src/scripts/extensions/agent-system/src/run-event-presenter.ts)：Timeline 的事件展示。
