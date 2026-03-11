# 前端性能基线（Perf HUD）

> 目的：把“当前真实表现”固定成基线，用于重构 Phase 1–3 的回归与对比。  
> 数据来源：`.cache/tauritavern-perf-stat-worldinfo-final.json`（由 `src/tauri/main/perf/perf-hud.js` 导出）

---

## 1. 采样环境（来自报告 env）

- createdAt：`2026-03-11T20:26:10.685Z`
- userAgent：Android 12 WebView（Chrome 91）
- hardwareConcurrency：6
- deviceMemory：4（GB）
- viewport：412×915，dpr 2.625

> 注：不同设备/系统版本/冷启动状态会导致数值差异。本基线用于“同类环境下的相对对比”，不是绝对 KPI。

---

## 2. 启动关键指标（来自 perfEntries.measures）

| 指标 | 基线（ms） | Phase 3 目标（建议） |
|---|---:|---|
| `tt:init:total` | 2624.3 | 降低 ≥ 20% |
| `tt:init:import:lib` | 553.5 | 降低 ≥ 20% |
| `tt:init:import:tauri-main` | 2000.1 | 降低 ≥ 30%（或设定绝对预算） |
| `tt:init:import:app` | 70.2 | 保持不回归 |
| `tt:tauri:ready` | 109.7 | 保持不回归 |

---

## 3. Invoke 热点（来自 invokes.statsByCommand，按 totalMs 排序 Top 12）

| command | count | totalMs | maxMs |
|---|---:|---:|---:|
| `count_openai_tokens_batch` | 102 | 16568.6 | 866.9 |
| `get_world_infos_batch` | 19 | 2417.8 | 837.6 |
| `read_thumbnail_asset` | 19 | 2132.1 | 276.4 |
| `save_user_settings` | 10 | 1805.7 | 435.5 |
| `get_all_characters` | 1 | 941.8 | 941.8 |
| `get_sillytavern_settings` | 2 | 716.9 | 360.2 |
| `get_character` | 2 | 563.5 | 299.1 |
| `get_chat_completions_status` | 1 | 413.6 | 413.6 |
| `list_recent_chat_summaries` | 1 | 341.3 | 341.3 |
| `save_world_info` | 2 | 290.6 | 212.7 |
| `get_client_version` | 2 | 253.6 | 213.3 |
| `save_quick_reply_set` | 3 | 219.2 | 101.7 |

重构 Phase 3 的“优先收益点”：

- `count_openai_tokens_batch`：in-flight 去重 + 短 TTL 缓存 + 并发限制（必要时写后合并）。
- `read_thumbnail_asset`：LRU + in-flight 去重 + 并发限制；必要时预热。

---

## 4. 主线程卡顿信号（来自 longFrames / longTasks）

- longFrames：162 条，max delta ≈ 866.7ms
- longTasks：73 条，max duration 469ms，max blocking 419ms

建议 Phase 3 里把“启动期与关键交互”的长任务/长帧作为回归门槛，避免“结构变好但更卡”。

