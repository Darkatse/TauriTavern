# 更新渠道当前契约

TauriTavern 只维护 `stable` 与 `canary` 两个更新渠道。更新功能只负责检测和引导下载，不在应用内安装更新。

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

`.github/workflows/canary-release.yml` 从 `dev` 的同一提交构建桌面端与移动端，完整产物通过后才更新固定的 `Canary` Release 和 tag。产物统一命名为：

```text
TauriTavern-<YYYYMMDD-HHmm>-canary-<platform>-<arch>-<kind>.<ext>
```

其中时间以 `Asia/Shanghai` 计算；Release 标题使用 `Canary Release <YYYY.MM.DD>`。tag 最后移动到已发布提交，因此客户端不会先看到尚未完成的构建。

Release notes 先由 Git 历史生成确定性上下文和回退正文。独立的只读 Codex job 使用 `CANARY_CODEX_API_KEY`、`CANARY_CODEX_RESPONSES_ENDPOINT` 与 `CANARY_CODEX_MODEL` secrets 检查实际 diff，再通过项目专用 Skill 撰写中英双语正文。Skill 源文件保存在不会被本地 Codex 自动发现的 `.github/codex/skills/`，CI 只把它们复制到 runner 临时 `CODEX_HOME`。Codex 调用失败或输出不符合结构时直接使用确定性正文，不影响构建和发布。

## 维护约束

1. 不要用显示时间判断更新；时间只服务用户认知，提交 SHA 才是 Canary 身份。
2. 不要让桌面与移动端各自推进 Canary tag；一个 Release 必须对应一个源码提交。
3. 不要让 AI 决定版本、产物、发布条件或 tag；AI 只能改写已经生成的事实。
4. 修改渠道 DTO、settings 或 command 时，保持 Rust serde 名称与前端字符串 `stable` / `canary` 一致。
