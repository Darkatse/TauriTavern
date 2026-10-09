# 前端共同控件与无障碍语义

功能提供名称、内容和业务状态；浏览器与共同控件负责共同行为。新增功能沿这些入口接入，避免局部重复实现。

## 普通控件与旧标记

新操作使用 `button type="button"`，导航使用带 `href` 的链接，字段使用原生表单与 label 关联。名称、说明和翻译由模板或组件提供；保留旧结构时，可用 `aria-labelledby`、`aria-describedby` 关联实际文字。

字段名称引用标题内只含标题文字的 span：id 为 `<控件 id>_label`，文本 `data-i18n` 写在 span 上，图标、链接和按钮放在 span 外，标题容器保持原样。原生 label 只含标题文字时直接关联，否则字段用 `aria-labelledby` 引用 span。代码读取名称用 [`getControlName`](../src/scripts/dom-handlers.js) 与 `getEditorName`；动态实例用 `setAriaRelation` 关联实际节点，所需 id 由 `ensureElementId` 分配。

图标帮助链接命名为“Docs + 对象”：`aria-label="Docs"`（`data-i18n="[aria-label]Docs"`），`aria-labelledby="<链接 id> <标题 span id>"`。

[`legacy-controls.js`](../src/scripts/legacy-controls.js) 统一补充旧控件的角色、焦点与键盘激活，保留 `a11y.js`、`keyboard.js` 的公开入口。原生行为和作者声明的角色优先；旧非原生按钮通过 click 激活，键盘只处理实际目标。

样式与业务状态不能单独决定控件语义：分组容器不是按钮，条目的 `.disabled` 也不代表其内部操作不可用。已有扩展的 DOM、角色与焦点约定应按实际用途保留。

角色卡脚本可以在同源 iframe 中创建控件，再挂入宿主页面。抽屉与旧控件适配层通过节点类型和 HTML 命名空间识别这些节点；挂载不会改变其 JavaScript 原型，不能用主窗口的 `instanceof HTMLElement` 排除它们。

## 配对数值控件

[`dom-handlers.js`](../src/scripts/dom-handlers.js) 管理已有 `.range-block-counter`、`.neo-range-input` 数字框与 `data-for` 指向的 range。数字框允许编辑草稿，`change`、Enter 和滚轮沿 range 的既有 `input` 业务路径提交；无效值留在字段中供修正，不写入设置，也不阻止离开字段。

数值都经 range 的 `input` 提交到业务值，数字框与 Zen 滑块只从 range 投影；改变 `min/max/step` 后同样发送 `input`。格式化与保存仍由各 API 负责。

恢复保存值（启动、切换预设、应用模型上下文）按“最终约束 → 写值 → `input`”进行，数值统一经 `restoreNumericInput` 写入。它只接受有限 number，缺省值由各 API 的数据层提供：Kobold、NovelAI 使用各自的默认表，Text Completion 预设只覆盖所提供的字段。超出最终范围的值会被调整，由操作入口用 `showNumericAdjustments` 按来源分组汇总提示一次；用户自己收紧约束时直接截断，不提示。

Zen 滑块创建和刷新时不写业务值。值不在档位上时显示真实值、位置取最近档位，用户拖动后才提交档位值。

## Select2

名称与说明写在原生 select 上：label、`aria-labelledby` 或 `aria-label`，以及 `aria-describedby`。[Select2 补丁](../src/scripts/select2-accessibility.js) 把这些关系连同当前值接到实际获得焦点的元素上，并让脚本 click 走原有的选择路径；trusted 激活已经过 mousedown/mouseup，不重复处理。上游发布等价修复后删除补丁。

## 可排序列表与控件外壳

[`sortable-list.js`](../src/scripts/sortable-list.js) 为列表提供拖拽和上移/下移按钮。DOM 顺序是唯一的位置事实，功能在 `onReorder` 中读取并保存；后加入的条目经返回的 `addItem` 接入。`setSortableListEnabled` 同时停用拖拽与排序按钮。

`.tt-control-shell` 是原生控件的可见外壳：控件本身用 `.sr-only`，仍参与键盘、读屏与 Agent 操作；外壳相对定位并显示内部焦点环。图标开关（checkbox，关联 label 加 `.not_focusable`）、头像上传与列表条目共用这一写法。legacy 适配层不为外壳推断角色或键盘行为；Agent 以外壳为可见点击表面，click 派发给原生控件。

列表条目保留原有容器，第一个子元素是原生主操作按钮（`:scope > .sr-only`，JS 用 `CONTROL_SHELL_CLASS`），点击冒泡到原有条目处理器。主操作名称引用条目标题，说明只引用日期、计数等简短事实；子操作是并列的原生按钮，读作“动作 + 对象”（`title` 提供动作，`aria-labelledby` 引用自身与条目名称）。列表与 listitem 角色由模板声明。

图标来自 Font Awesome 6.7.2，`content: var(--fa) / ""` 让装饰字形不参与可访问名称（旧 WebKit 仍显示字形），图标伪元素从 Font Awesome 的字体变量取字体，不继承控件的文本字体；核心自写的图标伪元素同此规则。名称以浏览器原生可访问树核对。

## 状态与重绘焦点

ARIA 是既有业务状态的投影，由该状态的写入者同步更新：选择与开关用 `aria-pressed`，当前项用 `aria-current`（分页为 `"page"`），非当前时移除；展开用 `aria-expanded`。状态只由状态属性表达，名称不随状态变化；附加说明（三态标签的包含/排除、角色连接的“本聊天用过”）用 `aria-describedby`。打开偏好对话框的按钮不声明切换状态。

列表整体替换用 `keepFocus(getItems, keyOf, write)`：异步准备在外面完成，`write` 一次同步完成移除与插入，写入后控件即可聚焦，身份键在条目存续期间稳定。它只恢复原本在列表内的焦点，依次取同一条目、同一序号、最近的条目；没有目标时返回 `false`，由调用者聚焦搜索或新建入口。插件的同步写入钩子用 `captureFocus`。

文件操作在动作开始时确定目标（所有者身份与文件名），之后不再从当前选择或数组序号读取。对外导出保持上游签名：以角色序号为参数的入口在调用时同步解析出头像，序号不跨越等待。聊天条目（最近聊天、聊天记录）以 `data-avatar / data-group / data-file` 携带目标，内部只读这些属性，`file_name` 保留给外部 DOM 消费者。

导航函数（`selectCharacterById`、`openGroupById`）负责聚焦目的地，聊天读取函数只负责加载；移动端不聚焦，焦点已在可见的 input 或 textarea 中时不移动。导航进度不用模态，从聊天记录导航时先关闭面板。列表修改操作的阻塞浮层只覆盖文件操作，关闭后再重绘列表。

角色行以头像为焦点键，保留 id 与 `data-chid` 的外部契约。进出文件夹与回到第 1 页合成一次渲染意图，由 `printCharacters` 消费一次：显式聚焦目标，目标不在本页时取首项。

[`initPagination`](../src/scripts/pagination.js) 拥有分页配置、条数持久化、语义与导航焦点，列表内容由调用者渲染。原位重绘保留页码，换数据时调用者指定目标页；条目所在页用 `getPageForItem` 计算。

## 动作 select 与非模态浮层

角色、Persona 和群聊的 More... 保留原生 select。处理器先执行所选 option 的动作，或等待 `CHARACTER_MANAGEMENT_DROPDOWN`，末尾才复位到首项，扩展同步 change 监听器仍能读到所选项。不拦截原生方向键。

高级定义、聊天记录与群组成员浮层各由一个开关函数管理显隐，关闭仅在焦点仍在浮层内时归还触发控件。高级定义与聊天记录是有名称的非模态 `role="dialog"`，打开时聚焦浮层内的入口（移动端聊天记录聚焦浮层本身，以免弹出键盘）；群组成员浮层是可拖动的浮动面板，打开按钮与其他浮动面板一样不声明展开状态。

## API 连接

使用密钥的 Connect 先读取密钥状态，再保存新输入并连接；失败只结束本次操作，不重写或回滚已保存的密钥。`startStatusLoading` / `stopStatusLoading` 统一管理忙碌状态、Cancel 的显隐（`displayNone`）以及 Connect 与 Cancel 之间的焦点；`displayOnlineStatus` 是状态文字的唯一写入者。

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

[`popup-menu.js`](../src/scripts/popup-menu.js) 负责聊天选项、扩展、导出与批量操作菜单的展开关系、焦点与关闭。面板用 Tab 导航；Escape、外部点击或执行操作关闭，Tab 离开不关闭。程序化操作使用初始化返回的 `{ open, close, isOpen }`，同步显示、ARIA 与焦点；打开 Popup 前先把焦点交回可见触发器。上下文菜单的 Shift+F10 与菜单键由使用者显式处理，WebView 不一定产生 `contextmenu`。业务、Popper 定位与保留展开的特殊交互由各使用者提供。

消息操作共用 click 激活，更多操作展开后保留触发器。消息实例关联已有作者与楼层文本，重编号继续更新同一文本；编辑框和删除选择关联这份上下文。确认、取消和删除只恢复当前操作的焦点，不改变保存队列、自动保存语义或 ChatSurface 驻留策略。
