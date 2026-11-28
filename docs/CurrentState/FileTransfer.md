# 文件导入与导出

宿主提供两个能力：**选取**用户文件，以及**交付**已经生成完毕的文件。功能代码提供用途、内容和建议文件名；系统面板、URI 授权与文件复制集中在 host 的 `platform/file_transfer`。

## 1. 规则

- **谁产出文件，谁发起交付。** Rust 产出的文件由对应命令交付。前端发起的交付有两种：页面可读的字节先暂存，再调用 `deliver_staged_file`；远程图片由 `download_remote_file` 下载进宿主区，页面按响应类型命名后凭句柄调用 `deliver_remote_file`。
- **交付消费源文件。** 保存、取消或失败都会结束本次产物。
- **交付结果为 `{ delivered }`。** 完成目标写入或系统交接后为 `true`，取消为 `false`，失败抛错。目标是普通文件时，同步落盘后才算完成。
- **选取按用途过滤。** 角色卡、Skill 归档、数据归档分别映射为各平台的过滤条件；文件内容由导入方校验。目录只在桌面选取：Skill 目录与数据目录。
- **暂存文件的扩展名就是格式。** 由产出者确定一次：选取看文件名，Android 分享由 Kotlin 认定 PNG，URL 下载的文件先看 `Content-Type` 再看文件名，宿主写成的卡是 PNG。名称只是标签。
- **URL 导入先识别站点。** 已识别站点的链接与 ID 换成站点提供的内容，站点分开提供卡片与图片时由宿主写成 PNG 卡；其余 URL 按直链下载，视为角色卡。站点规则在 `tt-adapter-external-import`。
- **导入消费暂存文件。** 页面把暂存路径交给导入命令，由 Rust 读取，导入结束后删除。世界书与 ST 兼容路由需要整份内容时，经 `open_staged_file` 读回。

## 2. 平台实现

| 平台 | 选取 | 交付 |
| --- | --- | --- |
| Windows / macOS / Linux | dialog 插件返回路径，复制进暂存；数据归档与数据目录原地使用；Skill 目录只复制其中的 Skill 包 | 保存面板；同卷时移动，跨卷时复制 |
| Android / OpenHarmony | dialog 插件返回带授权的 URI，通过 fs 插件打开 fd 后复制进暂存 | 保存面板授权目标 URI，通过 fd 写入 |
| iOS | Document Picker 生成副本，移动进暂存 | Share Sheet；源文件以交付名移入宿主区的独立目录后交接 |

Android 的 content URI 不带文件名，由 `ContentUriPlugin` 查询 `DISPLAY_NAME` 得到。分享给应用的 PNG 卡由 Kotlin 记下 URI，页面只收到分享 id；`stage_shared_file` 凭 id 向 `ContentUriPlugin` 取回 URI，在分享授权有效期间复制进暂存。

Android 与鸿蒙的保存面板只接收文件扩展名，类型由系统按自己的表转换；系统不认识的扩展名不声明具体类型。保存面板会按类型推导后缀，自行指定的类型可能与系统的表不一致，例如 ColorOS 会把 `application/octet-stream` 的 `.jsonl` 存成 `.bin`。

iOS 的 Share Sheet 交接文件本身，接收方按文件名命名，不采用 `NSItemProvider` 的建议名。

## 3. 暂存

| 暂存 | 位置 | 内容 |
| --- | --- | --- |
| 页面区 | `app_cache/tauritavern-staging/page/<kind>/` | WebView 字节（上传与 Blob 导出）、选取与分享的文件、Skill 目录副本、经来源授权下载的 URL 内容 |
| 宿主区 | `app_cache/tauritavern-staging/host/` | 保存图片时的下载、调试包、iOS 交接中的文件 |
| 归档 | `archive_imports_root/incoming`、`archive_exports_root` | 数据归档、用户备份 |

- 页面持有路径的文件都在页面区，页面区只放页面可读的内容。页面交来的路径须是页面区内不含 `..` 的绝对路径；读取、导入、交付与删除命令先按此校验（`infrastructure/staging.rs` 的 `validate_staged_path`）。
- 不交给页面路径的文件放在宿主区。保存图片的下载以资源句柄交给页面，句柄关闭时删除。

- WebView 字节经 `stage_file_*` 进入宿主：Android 用 base64 帧，其余平台用原始字节。
- 前端经宿主 `safeInvoke` 调用暂存与交付命令，失败时得到可读的错误消息；只有二进制分块走原始 invoke。
- 宿主下载使用 `Download` 配置（含用户的请求代理），不带页面的 Cookie 与 Referer。导入用的下载受[导入来源](UserEndpointAccess.md#导入来源)授权约束，保存图片不受。
- 写入失败的暂存文件由写入方删除。页面区与宿主区在宿主启动时清空，暂存后未进入交付的文件随之回收；归档暂存沿用过期清扫。
- 归档单独暂存：Android 的数据根与 cache 不在同一个卷，大型归档要放在数据卷上，才能直接移动进任务工作区。

## 4. 入口

| 产物或来源 | 入口 |
| --- | --- |
| `download()`、同源 `a[download]`、Android 图片长按（页面可读来源）、Skill 与 Agent 导出 | `deliverBlob()` |
| Android 图片长按（远程图片） | `deliverRemoteFile()`：`download_remote_file` 后调用 `deliver_remote_file` |
| 数据归档导出 | 任务完成后调用 `save_export_data_archive`：领取产物，交付，再标记为已处置 |
| 数据归档导入 | `import_data_archive_from_picker` |
| 用户备份 | `export_user_backup_archive` |
| 调试包 | `devlog_export_bundle` |
| 角色卡导入（选取、Android 分享、URL） | `api.characterCards` 得到 `StagedFile`，再经 `processDroppedFiles` 导入 |
| 世界书导入（URL） | `api.characterCards.contentFromUrl` 得到 `File`，再经 `importWorldInfo` 导入 |
| Skill 归档选取 | `pick_import_files` |
| Skill 目录选取（桌面） | `pick_skill_import_directories` |
| 数据目录（桌面） | `pick_data_root` |

`<input type="file">` 与拖放沿用原有入口。新增的导入导出功能接入上述入口。

## 5. 已知限制

- 移动端 dialog 插件会把面板失败也报告为取消。
- 写入中途失败时，目标文件可能不完整；Android 与鸿蒙的 fs 插件不能删除 URI 目标。
- 内容要在保存面板出现前完成暂存，期间不显示进度；远程下载没有总超时，也不能取消。
