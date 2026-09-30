# 前端共同控件与无障碍语义

功能提供名称、内容和业务状态；浏览器与共同控件负责共同行为。新增功能沿这些入口接入，避免局部重复实现。

## 普通控件与旧标记

新操作使用 `button type="button"`，导航使用带 `href` 的链接，字段使用原生表单与 label 关联。名称、说明和翻译由模板或组件提供；保留旧结构时，可用 `aria-labelledby`、`aria-describedby` 关联实际文字。

[`legacy-controls.js`](../src/scripts/legacy-controls.js) 统一补充旧控件的角色、焦点与键盘激活，保留 `a11y.js`、`keyboard.js` 的公开入口。原生行为和作者声明的角色优先；旧非原生按钮通过 click 激活，键盘只处理实际目标。

样式与业务状态不能单独决定控件语义：分组容器不是按钮，条目的 `.disabled` 也不代表其内部操作不可用。已有扩展的 DOM、角色与焦点约定应按实际用途保留。

## 配对数值控件

[`dom-handlers.js`](../src/scripts/dom-handlers.js) 管理已有 `.range-block-counter`、`.neo-range-input` 数字框与 `data-for` 指向的 range。数字框允许编辑草稿，`change`、Enter 和滚轮沿 range 的既有 `input` 业务路径提交；无效值留在字段中供修正，不写入设置，也不阻止离开字段。

range 提供 `min/max/step`，API 修改约束后发送 `input`，共同实现同步数字框。恢复设置时先准备约束再写入已加载的业务值；直接刷新数字框的加载路径同样发送 `input`，更新字段反馈。保留 API 自身的格式化与保存职责，不从 DOM 观察器反推业务变化。

## Drawer

[`drawers.js`](../src/scripts/drawers.js) 管理展开状态、ARIA 关联和变化订阅。

| 类型 | 状态与入口 |
| --- | --- |
| 顶层面板 | `.openDrawer` 表示展开；`isTopLevelDrawerOpen`、`setTopLevelDrawerOpen`、`subscribeDrawerState` 读取、修改与订阅 |
| Inline | 实际触发图标的 `up/down` 表示逻辑展开；`isInlineDrawerOpen` 读取，`toggleInlineDrawer` 用于动画点击，`setInlineDrawerOpen` 用于即时设置 |

展开状态以已有 DOM class 为准。共同实现接收扩展对已识别 drawer 的 class 修改，并保留移动面板与触发控件的关联。第一方 bundle 复用页面的同一模块实例及其状态订阅。

Inline 的 display 属于动画表现，即时设置会取消旧动画。`inline-drawer-toggle` 保留生态时序：点击在动画前发送带 `detail.open` 的通知；即时设置在更新 display 后发送无 detail 的通知。`utils.toggleDrawer()` 保留旧调用入口。

## Popup

[`popup.js`](../src/scripts/popup.js) 通过可选的 `label: string | HTMLElement` 接收名称或弹窗内的可见标题，统一建立 ARIA 关联。`Popup.show.*` helper 自动使用传入的 header。

主输入框可通过 `inputLabel`、`inputDescription` 接收字符串或弹窗内的元素，分别表达字段用途与说明。需要等待保存结果的业务复用异步 `onClosing`：成功允许关闭，失败返回 `false` 并保留输入；忙碌与结果文案由该业务提供。

自带按钮的键盘与指针操作共用 click 路径；文本输入保留多行、Ctrl+Enter 与 `.result-control` 提交规则。

自定义结果中，`undefined` 只执行 action，`null` 表示取消，数字表示对应结果。不可用控件阻止用户操作，显式 `complete*()` 仍可程序化关闭；异步 `onClosing`、嵌套焦点与弹窗生命周期由 Popup 负责。

## 聊天输入、菜单与消息操作

发送按钮、Enter 与全屏编辑器共用 `sendTextareaMessage()`。普通生成的互斥位于该入口；Agent 指引沿同一入口的独立分支提交，名称使用当前草稿与活动 Run 的共同判断。有指引草稿时仍沿现有显示规则隐藏停止按钮。生成状态说明只表达当前阶段，不把结束事件当作成功，也不逐 token 公告正文。

[`popup-menu.js`](../src/scripts/popup-menu.js) 负责聊天选项和扩展菜单的展开关系、焦点与关闭。面板提供普通操作，使用 Tab 导航；Escape、外部点击或执行操作关闭，Tab 离开不自动关闭。程序化操作使用初始化返回的关闭函数，同步显示、ARIA 与焦点；操作打开 Popup 前先将焦点交回可见触发器。业务、Popper 定位与保留展开的特殊交互由各使用者提供。

消息操作共用 click 激活，更多操作展开后保留触发器。消息实例关联已有作者与楼层文本，重编号继续更新同一文本；编辑框和删除选择关联这份上下文。确认、取消和删除只恢复当前操作的焦点，不改变保存队列、自动保存语义或 ChatSurface 驻留策略。
