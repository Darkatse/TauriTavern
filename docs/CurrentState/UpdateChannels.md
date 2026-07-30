# 更新渠道当前契约

TauriTavern 只维护 `stable` 与 `canary` 两个更新渠道。更新功能只负责检测和引导下载，不在应用内安装更新。

Linux 系统软件源默认使用 `stable`，并提供独立的 `canary` 套件。APT/DNF/Zypper 根据系统的软件包数据库安装更新，与应用内 Stable/Canary 偏好相互独立；软件源地址、签名身份和安装方式见 `docs/CurrentState/LinuxRepository.md`。

## 身份与默认渠道

- 面向用户：Stable 显示版本号；Canary 显示中国标准日期，例如 `Canary Release 2026.06.14`。
- 面向程序：Stable 按 SemVer 比较；Canary 把当前构建与远端 Git 提交都规范化为 12 位短哈希后精确比较。
- 构建分支为精确的 `main` 时默认 `stable`，其他已知分支默认 `canary`；缺失分支信息时保守默认为 `stable`。
- 用户可在版本扩展设置中覆盖默认渠道。该选择进入现有 settings 持久化链路。

构建身份由 `TAURITAVERN_BUILD_BRANCH` 和 `TAURITAVERN_BUILD_REVISION` 注入；未显式提供时，构建脚本依次读取 GitHub Actions 环境和本地 Git。Canary 构建缺失或包含非法 revision 时必须明确失败，不能退回版本号比较。

## 检测链路

前端把有效渠道传给 `check_for_update` command，application service 决定比较语义，GitHub adapter 只负责读取渠道对应的数据：

- Stable：`GET /repos/Darkatse/TauriTavern/releases/latest`
- Canary：`GET /repos/Darkatse/TauriTavern/releases/tags/Canary` 与 `GET /repos/Darkatse/TauriTavern/commits/Canary`

Canary Release 必须是 prerelease；Stable latest 不能是 prerelease。返回给前端的 `release_token` 是机器去重键：Stable 使用 tag，Canary 使用 `sha12`。弹窗主要展示 Release name，因此 Canary 的时间格式由发布流水线统一产生。

## Canary 发布链路

`.github/workflows/canary-release.yml` 从 `dev` 的同一提交构建桌面端与移动端，完整产物通过后才更新固定的 `Canary` Release 和 tag。Stable 与 Canary 共用面向用户的产物命名契约：

```text
TauriTavern-<release-id>-<platform>-<arch>[-<variant>][.<ext>]
```

Stable 的 `release-id` 是版本号；Canary 使用 `<YYYYMMDD>-canary`，日期以 `Asia/Shanghai` 计算。平台统一使用 `windows`、`macos`、`linux`、`android`、`ios`，桌面架构统一使用 `x64` / `arm64`，Android 使用标准 ABI 名称。扩展名已经能区分包格式时不重复添加 kind，只有 `setup`、`portable`、`TestFlight` 等必要变体保留后缀；Linux portable 是无扩展名的原生可执行文件。Release 标题使用 `Canary Release <YYYY.MM.DD>`。tag 最后移动到已发布提交，因此客户端不会先看到尚未完成的构建。

Release notes 先由 Git 历史生成确定性上下文和回退正文。独立的只读 Codex job 使用 `CANARY_CODEX_API_KEY`、`CANARY_CODEX_RESPONSES_ENDPOINT`、`CANARY_CODEX_MODEL` 与 `CANARY_CODEX_EFFORT` secrets 检查实际 diff，再通过项目专用 Skill 撰写中英双语正文。Skill 源文件保存在不会被本地 Codex 自动发现的 `.github/codex/skills/`，CI 只把它们复制到 runner 临时 `CODEX_HOME`。Codex 调用失败或输出不符合结构时直接使用确定性正文，不影响构建和发布。

## Public TestFlight

Stable 与 Canary 原有的自签 iOS IPA 保持原构建参数和 Release 文件名。iOS job 在普通 IPA 完成后，复用已生成的前端资源额外构建 `ios_external_beta` + App Store Connect 版本，并以 `-TestFlight.ipa` 后缀作为独立产物。完整 GitHub Release 产物发布成功后，共享的 `.github/workflows/public-testflight.yml` 才上传并分发该专用 IPA；成功进入 TestFlight 流程后，再从 GitHub Release 删除临时的 `-TestFlight.ipa`，普通自签 IPA 始终保留。

Stable 的专用 IPA 只进入 `TauriTavern Beta Test`，Canary 的专用 IPA 只进入 `TauriTavern Canary Test`；流水线会校验目标确实是当前 App 的公开外测组。每次 TestFlight 构建使用按时间单调变化的 `CFBundleVersion`，避免两个渠道共用营销版本时发生构建号冲突。

上传前，隔离且只读的 Codex job 检查实际提交范围与 `ios_external_beta` capability 边界，只生成公开包中可见、值得测试的英文 `What to Test` 内容。AI 不参与构建、版本、分组或审核决策；调用失败或正文非法时使用确定性的通用测试提示，因此 AI 不会阻塞上传。IPA 上传前先按 App、营销版本与构建号检查 App Store Connect；重跑遇到已上传的同一构建时跳过重复上传。随后由确定性脚本等待 Apple 完成处理、写入 `en-US` build localization、开启自动通知、关联唯一目标组，并在需要时提交 Beta App Review。Apple 审核是异步外部状态：提交被接受即结束 CI，不等待审核通过。

## 维护约束

1. 不要用显示时间判断更新；时间只服务用户认知，提交 SHA 才是 Canary 身份。
2. 不要让桌面与移动端各自推进 Canary tag；一个 Release 必须对应一个源码提交。
3. 不要让 AI 决定版本、产物、发布条件或 tag；AI 只能改写已经生成的事实。
4. 修改渠道 DTO、settings 或 command 时，保持 Rust serde 名称与前端字符串 `stable` / `canary` 一致。
5. Public TestFlight 始终使用 `ios_external_beta`；不要把 GitHub Release 的完整更新日志直接复制成 `What to Test`。
