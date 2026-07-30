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

### 项目哲学

本节说明 TauriTavern 处理需求和技术取舍的基本方法。它们适用于需求分析、设计、实现和评审，也帮助新维护者理解项目长期坚持什么。每次决定仍需结合用户需求、SillyTavern 生态、平台条件和能够验证的事实。

#### 1. 通用大于特判

特判是为某个入口、平台或单一场景单独增加一套规则。通用方案会先找出同类需求共有的语义，再由一处机制统一处理。这样可以减少重复实现和行为漂移。同类问题只需修复一次，维护者也只需理解一套规则。

提出新功能时，先看看仓库是否已有相近能力，再考虑扩展既有机制。真实存在的差异应当清楚保留。为了形式统一而提前搭建万能框架，同样会增加维护成本。

- 正例：多个入口需要完成同一件事，由一套共同机制处理，各入口只提供自身的数据。
- 反例：每个入口各写一套相似逻辑，后续修复时再逐处复制修改。

#### 2. 从第一性原理出发

第一性原理要求我们暂时放下当前实现和既有结论，从原始需求重新推导。分析问题时可以依次确认：用户希望得到什么，哪些行为已经成为外部契约，现有判断有多少证据，以及哪些条件必须长期成立。完成这些确认后，再选择具体技术。

TauriTavern 同时连接 SillyTavern 前端生态、WebView 和原生宿主。问题显现的位置与原因所在的位置可能相距很远。因此，源码、测量和可复现实验应当优先于直觉。

- 正例：先定义成功结果和不可接受的后果，通过证据找到原因，最后选择改动位置。
- 反例：从熟悉的技术或最初的猜测出发，直接修改表面症状并扩大改动范围。

#### 3. Fail fast，避免静默降级

Fail fast 指约束已经不成立，或者结果无法确认时，及时停止相关操作，并把错误和必要上下文交给调用者。静默降级、吞掉错误或猜测默认值，会让一次清楚的故障变成更晚出现的不一致，也可能增加数据损坏的风险。

含义明确且预期内的异常可以有恢复路径。恢复条件、结果和责任需要清楚表达，调用者也应当知道发生了恢复。面对未知状态时，不要以“防御性编程”为由替系统猜一个结果。

- 正例：已知的可恢复情况返回明确结果；状态无法判断时直接报错，并保留诊断信息。
- 反例：捕获所有异常后改走默认路径，最终仍向调用者报告成功。

#### 4. 可维护性优先

软件工程没有魔法。今天省下的理解成本，往往会在后续修改中以更高代价回来。可维护性包括清楚的命名和责任、单一的事实来源、明确的边界，以及可以验证的行为。新维护者应当能够依靠当前代码和文档理解系统，无需掌握未写下来的历史。

对于长期持续开发的项目，可维护性和代码可读性放在首位。在正确性、用户数据和兼容契约得到保障的前提下，可以接受一定的局部性能或开发效率损失。真实且严重的性能问题可以引入额外复杂度，复杂度应当与测量结果相称，并留在合适的边界内。

- 正例：选择容易理解和验证的结构，接受已经评估过的少量性能开销。
- 反例：为了未经测量的局部收益，引入平行实现、隐藏状态或依赖时序的行为。

#### 5. KISS 与 60/95 原则

KISS 要求我们选择能够完整解决当前问题的最简单方案。判断简洁程度时，应看整个系统需要多少概念、依赖和隐含知识。代码行数只是一个局部指标。

60/95 是一项取舍原则，即我们的目标是用 60% 的复杂度覆盖 95% 的边界。其中的数字表达方向，无需作为验收指标计算。缺少现实依据的罕见场景可以留到真正出现时再处理。

- 正例：先完整解决常见且影响较大的场景，写清当前边界，等真实需求出现后再扩展。
- 反例：为想象中的极端情况进行层层兜底，或者选择复杂度更多的实现，长期承担很少产生价值的复杂度。

#### 6. 简洁与优雅

项目所说的优雅，是对整体实现的判断。概念应当清楚，责任应当放在拥有相关事实的地方，数据流和错误路径应当容易追踪，方案也应尊重用户选择和既有生态。一个优雅的设计通常可以用简短的话说明，新增同类需求时也能沿现有结构自然扩展。

简洁高于繁琐，优雅高于杂乱。现代软件工程实践以及 Rust、Tauri 对显式语义、类型和所有权、清晰边界与平台原生能力的重视，都是实现这种整体质量的常用方法。具体做法仍应服务于问题本身。

- 正例：解决方案的责任归属清楚，数据和错误沿一条容易说明的路径流动。
- 反例：各层不断补偿上一层的特殊情况，每段代码都能单独解释，组合后的行为却难以说明。

#### 7. 遵循，但不拘泥

这些原则之间会出现张力。通用方案可能影响兼容性，fail-fast 需要与合理恢复区分，简单方案也可能无法满足已经测得的性能要求。每项选择都有所得失。遇到冲突时，应回到原始目标和已有证据，说明取舍，再选择适合当前现实的方案。也就是，坚持第一性原理，坚持实践中的判断。

维护者可以用这些原则提出问题和解释决定，无需逐条打勾。新证据推翻旧判断时，应当修改设计。项目哲学的价值在于帮助我们持续作出判断。

- 正例：事实发生变化后重新评估方案，记录理由，并尊重用户已经明确作出的选择。
- 反例：机械引用某条原则结束讨论，或为了保持原有设计而忽略新的事实。

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

### Project Philosophy

These principles explain how TauriTavern approaches requirements and technical tradeoffs. They apply to analysis, design, implementation, and review. They also give new maintainers a clear account of the values behind the project. Each decision still depends on user needs, the SillyTavern ecosystem, platform constraints, and evidence.

#### 1. Prefer general solutions to special cases

A special case adds a separate rule for one entry point, platform, or scenario. A general solution identifies the meaning shared by similar requirements and handles it in one place. This reduces duplicate implementations and prevents related paths from drifting apart. One fix can then cover the whole class of problems, and maintainers have fewer rules to learn.

Before adding a feature, check for a related capability in the repository and consider extending it. Preserve genuine differences clearly. A universal framework built before there is a shared need carries its own maintenance cost.

- Good: Several entry points perform the same operation through one shared mechanism and provide only their own data.
- Bad: Each entry point has similar private logic, so every later fix must be copied to several places.

#### 2. Start from first principles

First-principles reasoning sets aside the current implementation and earlier conclusions long enough to derive the decision from the original need. Establish what the user expects, which behavior is already an external contract, how much evidence supports the current diagnosis, and which conditions must continue to hold. Choose the technology after answering those questions.

TauriTavern connects the SillyTavern frontend ecosystem, WebView, and a native host. A symptom may appear far from its cause. Source code, measurements, and reproducible experiments therefore carry more weight than intuition.

- Good: Define the intended result and unacceptable outcomes, find the cause through evidence, and then choose where to make the change.
- Bad: Begin with a familiar technology or an early guess, patch the visible symptom, and expand the scope without verification.

#### 3. Fail fast and avoid silent degradation

Fail fast means stopping the affected operation when a required condition has failed or the result cannot be confirmed. Return the error with enough context for the caller to understand it. Silent degradation, swallowed errors, and guessed defaults turn a clear failure into a later inconsistency and may put user data at risk.

Expected conditions with clear meaning may have an explicit recovery path. Its trigger, result, and owner should be clear, and the caller should know that recovery occurred. "Defensive programming" should not invent an answer for an unknown state.

- Good: Return an explicit result for a known recoverable condition; report an error with diagnostic context when the state cannot be determined.
- Bad: Catch every error, switch to a default path, and still report success to the caller.

#### 4. Put maintainability first

There is no magic in software engineering. Work avoided by leaving code hard to understand returns during later changes, usually at a higher cost. Maintainability includes clear names and responsibilities, one source of truth, explicit boundaries, and behavior that can be verified. New maintainers should be able to understand the system from its current code and documentation without relying on unwritten history.

Maintainability and readability come first in a project intended for continued development. We may accept a modest local performance cost or a slower implementation when correctness, user data, and compatibility contracts remain protected. A serious measured performance problem may justify more complexity. Keep that complexity proportional to the evidence and contained within the appropriate boundary.

- Good: Choose a structure that is easy to understand and verify, accepting a small performance cost that has been evaluated.
- Bad: Add parallel implementations, hidden state, or timing dependencies for a local gain that has not been measured.

#### 5. Follow KISS and the 60/95 principle

KISS asks us to choose the simplest solution that fully addresses the current problem. Judge simplicity across the whole system: how many concepts, dependencies, and hidden assumptions must be maintained. Line count is only one local measure.

The 60/95 principle is a rule of thumb: aim to cover roughly 95% of real-world cases with roughly 60% of the complexity. The numbers describe the direction of the tradeoff and are not acceptance criteria. Rare cases without supporting evidence can wait until they occur.

- Good: Solve the common, high-impact cases completely, document the current limits, and extend the design when a real need appears.
- Bad: Add layers of safeguards for imagined extremes, or choose a more complex implementation without a clear benefit. The project then carries complexity that rarely provides value.

#### 6. Seek simplicity and elegance

Elegance is a judgment about the whole implementation. Concepts should be clear. Responsibility should stay with the part of the system that owns the relevant facts. Data and error paths should be easy to follow. The solution should also respect user choices and the existing ecosystem. An elegant design can usually be explained briefly, and related requirements can extend it without a new set of exceptions.

The project values simplicity over needless complication, and elegance over disorder. Modern software engineering offers useful guidance here. Rust and Tauri emphasize explicit semantics, types and ownership, clear boundaries, and native platform features. Each method should still serve the problem at hand.

- Good: Ownership is clear, and data and errors follow a path that a maintainer can explain without hidden context.
- Bad: Each layer compensates for special behavior in another layer; the individual pieces make sense, while their combined behavior is difficult to explain.

#### 7. Apply principles with judgment

These principles can pull in different directions. A general solution may affect compatibility. Fail-fast behavior must be distinguished from a valid recovery path. A simple solution may fall short of a measured performance requirement. Every choice has a cost. Return to the original goal and the available evidence, describe the tradeoff, and choose the solution that fits the current situation. Keep returning to first principles, and use judgment grounded in practice.

Use these principles to ask questions and explain decisions. No decision needs to satisfy them as a checklist. Revisit the design when new evidence overturns an earlier judgment. This is how the principles continue to help as the project changes.

- Good: Reassess a design when the facts change, record the reasoning, and respect choices the user has made explicitly.
- Bad: Cite one principle to end discussion or ignore new evidence in order to preserve an existing design.

### Before Opening a PR

Before opening a PR, please read this file and check the confirmation item in the PR template.

More contribution requirements will be added later. Until then, please keep changes clear, reviewable, and testable.
