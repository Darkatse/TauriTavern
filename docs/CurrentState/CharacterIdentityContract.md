# 角色身份契约现状

本文档记录 TauriTavern 当前已经落地的角色身份契约，重点覆盖角色头像文件名、聊天目录、旧错误目录兼容、以及后续开发时不能误改的边界。

目标读者：后续继续修改角色导入、编辑、聊天保存、聊天导出、Agent profile/skill 导入链路的开发者。

---

## 1. 范围与结论

TauriTavern 前端来源于 SillyTavern。角色相关接口必须遵循上游契约：

> `/api/characters/get` 返回和前端传回的 `avatar_url` 是一个精确的头像文件名，不是 URL，不是路径，也不是可被解析的资源地址。

例如：

- 合法身份值：`Alice#1.png`
- 对应 Rust 内部 character stem：`Alice#1`
- 对应 canonical chat directory key：`Alice#1`

因此任何角色身份链路都不能再对 `avatar_url` 做以下处理：

- `trim`
- `decodeURIComponent` / percent decode
- 去掉 `?query` 或 `#hash`
- 按 `/` 或 `\` 取 basename
- 把它当成 `/thumbnail?file=...` 一类资源 URL

这些变换只允许出现在后端 legacy resolver 内部，用来寻找历史版本已经错误创建的聊天目录。

---

## 2. 前端/API 边界

JS 侧的边界规则：

- `avatar_url` 表示 exact avatar filename。
- 当前只接受 `.png` 角色卡文件名。
- 文件名不能包含路径分隔符、查询/片段语义、控制字符或平台非法字符。
- 从 `avatar_url` 得到 character stem 时，只移除最后一个 `.png` 扩展名。

核心实现位置：

- `src/tauri/main/services/characters/character-identity.js`

持续开发约束：

- 路由、service、adapter、Agent 导入链路都应共用该身份模块。
- 不要在调用点重新写一套“宽松解析”。
- 如果调用方拿到的是资源 URL，应先回到产生该 URL 的业务上下文，不能把 URL 传进角色身份模块。

---

## 3. Rust 内部 key

Rust chat repository 当前接收的是 character stem，而不是 avatar filename。

也就是说：

- JS/API 边界：`Alice#1.png`
- Rust chat repository 参数：`Alice#1`
- 磁盘 canonical chat directory：`default-user/chats/Alice#1/`

核心实现位置：

- `src-tauri/src/infrastructure/repositories/chat_directory_identity.rs`
- `src-tauri/src/infrastructure/repositories/file_chat_repository/chat_dir_resolver.rs`
- `src-tauri/src/infrastructure/repositories/file_character_repository/helpers.rs`

持续开发约束：

- Rust resolver 的输入名虽然叫 `character_name`，实际语义是内部 stem/key。
- Rust resolver 不负责解析 avatar filename，也不负责解析 URL/path。
- Rust resolver 内部的 legacy candidate 生成，只是为了读取旧错误目录，不是新身份规则。

---

## 4. 聊天目录解析

当前角色聊天目录解析顺序：

1. 计算 canonical key。
2. 如果 alias store 已经记录该 key 的 legacy directory，并且目录仍存在，优先使用 alias。
3. 如果 canonical directory 已存在，使用 canonical directory。
4. 如果 canonical 不存在，尝试 safe legacy discovery。
5. 如果没有 legacy 命中，返回 canonical directory。

alias 优先于 canonical 是有意设计：

- 一旦系统确认 `Alice#1 -> Alice` 是旧错误目录，后续读写要继续落到同一个物理目录。
- 这样可以避免 canonical directory 后来出现时把历史聊天分裂成两份。

---

## 5. Legacy Discovery

legacy discovery 只服务于历史兼容。

它复现的是旧错误 normalizer 可能造成的目录名：

- `trim`
- 去掉 `?` 或 `#` 后的内容
- percent decode 后再去 query/hash
- percent decode 后再取 basename

命中条件：

- candidate directory 必须存在。
- candidate directory 至少包含一个 `.jsonl` 聊天文件。
- candidate 不能等于 canonical。
- 如果 `characters/<candidate>.png` 存在，则认为该目录属于另一个真实角色，不能抢占。
- 多个 legacy candidate 同时命中时 fail-fast，返回错误，不猜测。

这条规则避免了升级时全量扫描和批量迁移，对低端 Android/iOS 更安全：只有用户实际访问相关角色时才做一次小范围解析。

---

## 6. Alias Store

alias 文件位置：

- `default-user/user/cache/chat_aliases_v1.json`

alias key 是 Rust 内部 character stem，不是 avatar filename。

示例：

```json
{
  "version": 1,
  "aliases": {
    "Alice#1": {
      "dir": "Alice",
      "reason": "legacy-avatar-url-normalizer",
      "created_at": "2026-06-06T00:00:00Z"
    }
  }
}
```

当前生产 bootstrap 会把同一个 shared alias store 注入 character repository 和 chat repository：

- `FileCharacterRepository::with_chat_aliases(...)`
- `FileChatRepository::with_chat_aliases(...)`

这样两个 repository 不再各自持有独立 alias cache，避免并发懒发现时互相覆盖 alias 文件。

独立测试或单仓库使用仍可调用默认 `new(...)` 构造函数。默认构造只保证单 repository 自洽；生产路径应使用 shared store。

---

## 7. Rename / Delete 当前语义

角色 rename：

- 新身份使用新 canonical key。
- 如果旧聊天目录通过 canonical/alias/legacy resolver 找到，并且新 canonical 目录不存在，则尝试把旧目录 rename 到新 canonical 目录。
- 如果文件系统 rename 失败，错误直接暴露。

角色 delete：

- 删除角色卡。
- 当 `delete_chats=true` 时，通过 resolver 找到当前身份对应的物理聊天目录并删除。
- 这包括已经确认的 alias 目录，也包括 safe legacy discovery 找到的历史错误目录。

这意味着 delete 语义偏向用户意图：“删除这个角色及其聊天”，而不是只删除 canonical directory。

---

## 8. 不支持的行为

当前不做启动期全量迁移：

- 不扫描全部 `characters/` 和 `chats/`。
- 不在升级时批量 rename 目录。
- 不自动合并多个可能相关的聊天目录。
- 不在 ambiguous legacy 情况下静默选择一个目录。

如需批量清理或显式迁移，应单独实现维护工具，并展示冲突项让用户确认。

---

## 9. 最容易误改的契约

后续开发时尤其不要做这些事：

1. 把 `avatar_url` 当成 URL/path 解析。
2. 在路由或 service 里临时写 `trim/decode/basename`。
3. 在 Rust resolver 里把 legacy candidate 当作新规则。
4. 让 character repository 和 chat repository 在生产路径各自创建 alias store。
5. 升级时递归扫描并重命名大量聊天目录。
6. ambiguous legacy 时静默降级到某个目录。

正确方向是：

- 新数据严格使用 exact avatar filename -> stem -> canonical directory。
- 旧错误目录只通过 alias/lazy resolver 兼容。
- 发现歧义时 fail-fast。
