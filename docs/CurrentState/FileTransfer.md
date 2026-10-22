# 文件导入与导出

宿主提供两个能力：**选取**用户文件，以及**交付**已经生成完毕的文件。功能代码提供用途、内容和建议文件名；系统面板、URI 授权与文件复制集中在 host 的 `platform/file_transfer`。

## 1. 规则

- **谁产出文件，谁发起交付。** Rust 产出的文件由对应命令交付；前端发起的交付先暂存内容（WebView 字节，或宿主下载的远程资源），再调用 `deliver_staged_file`。
- **交付消费源文件。** 保存、取消或失败都会结束本次产物。
- **交付结果为 `{ delivered }`。** 完成目标写入或系统交接后为 `true`，取消为 `false`，失败抛错。目标是普通文件时，同步落盘后才算完成。
- **选取按用途过滤。** 角色卡、Skill 归档、数据归档分别映射为各平台的过滤条件；文件内容由导入方校验。

## 2. 平台实现

| 平台 | 选取 | 交付 |
| --- | --- | --- |
| Windows / macOS / Linux | dialog 插件返回路径，复制进暂存；数据归档原地使用 | 保存面板；同卷时移动，跨卷时复制 |
| Android / OpenHarmony | dialog 插件返回带授权的 URI，通过 fs 插件打开 fd 后复制进暂存 | 保存面板授权目标 URI，通过 fd 写入 |
| iOS | Document Picker 生成副本，移动进暂存 | Share Sheet |

Android 的 content URI 不带文件名，由 `ContentUriPlugin` 查询 `DISPLAY_NAME` 得到。

## 3. 暂存

| 暂存 | 位置 | 内容 |
| --- | --- | --- |
| 通用 | `app_cache/tauritavern-staging/<kind>/` | WebView 字节（上传与 Blob 导出）、宿主下载的远程资源、选取的角色卡与 Skill 归档、调试包 |
| 归档 | `archive_imports_root/incoming`、`archive_exports_root` | 数据归档、用户备份 |

- WebView 字节经 `stage_file_*` 进入宿主：Android 用 base64 帧，其余平台用原始字节。
- 前端经宿主 `safeInvoke` 调用暂存与交付命令，失败时得到可读的错误消息；只有二进制分块走原始 invoke。
- 远程资源经 `stage_file_from_url` 进入宿主：由 `ContentService::download_external_file` 下载，与外部内容导入共用 URL 规则和 `Download` 配置（含用户的请求代理），不带页面的 Cookie 与 Referer。
- 写入失败的暂存文件由写入方删除。通用暂存在宿主启动时清空，暂存后未进入交付的文件随之回收；归档暂存沿用过期清扫。
- 归档单独暂存：Android 的数据根与 cache 不在同一个卷，大型归档要放在数据卷上，才能直接移动进任务工作区。

## 4. 入口

| 产物或来源 | 入口 |
| --- | --- |
| `download()`、同源 `a[download]`、Android 图片长按（页面可读来源）、Skill 与 Agent 导出 | `deliverBlob()` |
| Android 图片长按（远程图片） | `deliverRemoteFile()`：`stage_file_from_url` 后调用 `deliver_staged_file` |
| 数据归档导出 | 任务完成后调用 `save_export_data_archive`：领取产物，交付，再标记为已处置 |
| 数据归档导入 | `import_data_archive_from_picker` |
| 用户备份 | `export_user_backup_archive` |
| 调试包 | `devlog_export_bundle` |
| 角色卡、Skill 归档选取 | `pick_import_files` |

`<input type="file">`、拖放和桌面目录选择沿用原有入口。新增的导入导出功能接入上述入口。

## 5. 已知限制

- 移动端 dialog 插件会把面板失败也报告为取消。
- 写入中途失败时，目标文件可能不完整；Android 与鸿蒙的 fs 插件不能删除 URI 目标。
- 内容要在保存面板出现前完成暂存，期间不显示进度；远程下载没有总超时，也不能取消。
