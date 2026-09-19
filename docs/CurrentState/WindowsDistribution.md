# Windows 分发现状

TauriTavern 的 Windows Stable Release 以 GitHub Release 为唯一安装包来源。WinGet
只发布指向该 Release 的元数据，不重新构建或托管二份安装包。

## WinGet 契约

- Package ID：`TauriTavern.TauriTavern`
- 安装包：`TauriTavern-<version>-windows-x64-setup.exe`
- 类型：NSIS（WinGet manifest 中为 `nullsoft`）
- 架构与范围：`x64`、`user`
- 静默参数：`/S`；带进度的无人值守参数：`/P`
- 升级行为：`install`
- 仅发布 Stable；Canary、MSI 和 portable zip 不进入这个 Package ID

用户目录安装与当前 NSIS 默认行为一致。Portable 版本依赖 Scoop 的 `persist`
语义，不能把它等价映射为 WinGet portable package。

## 发布链路

`.github/workflows/stable-release.yml` 在全部 Release 资产上传成功后调用
`.github/workflows/winget-release.yml`。后者：

1. 校验 tag、仓库版本与已发布的非 prerelease Release 一致；
2. 精确选择一个 x64 NSIS setup 资产；
3. 若目标版本已合入或已有开放 PR，则幂等成功；
4. 下载固定版本的 WingetCreate 并校验 SHA256；
5. 从官方仓库中的上一版 manifest 生成更新，先上传为 Actions artifact，再提交 PR。

工作流可以按 `release_tag` 手动运行，因此 WinGet 失败时只重试 manifest 发布，
不需要重新构建 Stable Release。Stable 的资产上传仍保留 `--clobber`；发布契约是
每个 Stable tag 只构建、上传一次，WinGet 始终读取上传完成后的最终资产。

## 一次性启用

自动更新依赖官方 `microsoft/winget-pkgs` 仓库中已有 Package ID。首次启用需要：

1. 以一个已经发布的 Stable 版本提交并合入
   `TauriTavern.TauriTavern` multi-file seed manifest；
2. 确认 seed manifest 满足上面的 installer 契约，并在真实 Windows 用户环境验证
   install、silent install、upgrade 与 uninstall；
3. 使用完成 Microsoft CLA 的专用 GitHub 账号创建 classic PAT，仅授予
   `public_repo`，保存为仓库 secret `WINGET_CREATE_GITHUB_TOKEN`；
4. 手动运行 `WinGet Release` 验证更新 PR，再让后续 Stable 自动调用。

seed manifest 合入并同步到 WinGet source 后，安装命令为：

```powershell
winget install --id TauriTavern.TauriTavern --exact
```

在官方 source 尚未可查询前，不应把该命令加入面向用户的 README。
