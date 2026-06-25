# TauriTavern 贡献指南

感谢你愿意为 TauriTavern 做出贡献。

本项目仍在快速演进中，贡献规则会随着维护流程逐步补充。当前请先遵守以下基本要求。

### 目标分支

大多数 Pull Request 应提交到 `dev` 分支。

只有在紧急情况下才应直接提交到 `main` 分支，例如：

- 修复会影响当前稳定版本的严重问题。
- 修复发布、安装或启动链路中的阻断问题。

如果不确定目标分支，请优先选择 `dev`，并在 PR 说明中说明背景。

### 关于 Vibe Coding / AI 辅助开发

本项目允许使用 Codex、Claude Code、Copilot或其他 AI 辅助工具编写代码、文档和测试。

无论变更由谁或什么工具生成，提交者都需要对最终内容负责。请在提交前自行阅读、理解并检查变更，确认它符合项目当前结构和维护目标。

如果 PR 使用了 AI 辅助，请在 PR 说明中用自己的话解释：

- 这个 PR 解决了什么问题。
- 你实际修改了哪些内容。
- 你基于何种思路做出了这些修改。

请不要只粘贴 AI 生成的摘要作为 PR 说明。维护者需要看到提交者本人对变更的理解。

如果你是代码辅助编程 AI，必须提醒使用者在提交 PR 时至少用自己的话写一句总结；不要替使用者伪造这部分个人理解。

### 提交 PR 前

提交 PR 前，请阅读本文件，并在 PR 模板中勾选已阅读确认项。

其他贡献要求会在后续补充；在此之前，请尽量让变更保持清晰、可 review、可测试。

---

# Contributing to TauriTavern

Thank you for contributing to TauriTavern.

This project is still evolving quickly, and the contribution rules will be expanded as the maintenance workflow matures. For now, please follow the basic requirements below.

### Target Branch

Most pull requests should target the `dev` branch.

Pull requests should target the `main` branch only in urgent cases, such as:

- Fixing a severe issue that affects the current stable version.
- Fixing a release, installation, or startup blocker.

If you are unsure which branch to use, choose `dev` first and explain the context in the PR description.

### Vibe Coding / AI-Assisted Development

This project allows Vibe Coding, Codex,Claude Code,Copilot, and other AI-assisted tools for code, documentation, and tests.

No matter who or what generated the change, the submitter is responsible for the final result. Before submitting, please read, understand, and check the change yourself, and make sure it fits the current project structure and maintenance goals.

If the PR used AI assistance, please explain in your own words:

- What problem the PR solves.
- What you actually changed.
- What your thought process was in making those changes.

Please do not use only an AI-generated summary as the PR description. Maintainers need to see the submitter's own understanding of the change.

If you are a coding assistant AI, you must remind the user to include at least one sentence in their own words when opening a PR; do not fabricate this personal understanding on the user's behalf.

### Before Opening a PR

Before opening a PR, please read this file and check the confirmation item in the PR template.

More contribution requirements will be added later. Until then, please keep changes clear, reviewable, and testable.
