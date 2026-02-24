# 沉浸全屏实施计划（Android 系统栏隐藏）

## 1. 背景与问题定义
当前移动端实现以 safe-area 适配为核心，系统栏通常可见，并通过 `--tt-safe-area-*` 与 `--tt-ime-bottom` 参与布局。

新需求明确为：
- 默认使用“沉浸全屏”模式；
- 沉浸全屏定义为 Android 隐藏系统栏（状态栏 + 导航栏），行为与视频类应用一致；
- 在前端 `#options_button` 菜单新增“切换全屏”入口，在“沉浸全屏 / 安全区模式”间切换。

## 2. 目标与非目标

### 2.1 目标
1. Android 启动后默认隐藏系统栏。
2. 前端新增菜单项 `切换全屏`，可即时切换模式。
3. 切换结果持久化到现有 `power_user` 设置。
4. 代码结构解耦：
   - Android 系统栏控制与 Insets 注入保持单点收敛；
   - 前端通过独立模块调用原生桥，不把平台细节散落在业务代码。

### 2.2 非目标
1. 不实现 iOS 原生系统栏隐藏（本次需求定义绑定 Android API）。
2. 不改造桌面端窗口全屏行为。
3. 不引入新的后端 Rust 命令（无需跨层扩展 IPC）。

## 3. 架构决策

### 3.1 模式定义
- `immersive fullscreen`：隐藏系统栏，safe-area 注入值按 `0` 处理。
- `safe-area mode`：显示系统栏，沿用现有 safe-area 适配逻辑。

### 3.2 状态单一真值
- 前端持久化字段：`power_user.mobile_immersive_fullscreen`（布尔，默认 `true`）。
- Android 运行态也维护同名语义状态（布尔），用于立即控制系统栏与 Insets 计算。

### 3.3 原生桥接方式
采用 Android `addJavascriptInterface` 增量桥接，不增加 Tauri/Rust 命令：
- `setImmersiveFullscreenEnabled(enabled: Boolean)`
- `isImmersiveFullscreenEnabled(): Boolean`

## 4. 详细改造方案

### 4.1 Android 侧

#### 4.1.1 `AndroidInsetsBridge` 扩展
在现有 `AndroidInsetsBridge` 中新增：
1. 运行态字段：`immersiveFullscreenEnabled`（默认 `true`）。
2. 公共方法：
   - `setImmersiveFullscreenEnabled(enabled: Boolean)`
   - `isImmersiveFullscreenEnabled(): Boolean`
3. 系统栏可见性控制：
   - `WindowInsetsControllerCompat.hide(Type.statusBars() | Type.navigationBars())`
   - `WindowInsetsControllerCompat.show(...)`
   - 行为设为 `BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE`。
4. Insets 计算分支：
   - 沉浸全屏：`systemBarInsets = Insets.NONE`；
   - 安全区模式：保留现有 `max(visible, stable)` 逻辑。

#### 4.1.2 新增 JS Bridge
新增 `AndroidSystemUiJsBridge.kt`，仅负责 JS -> `AndroidInsetsBridge` 调用转发。

#### 4.1.3 `MainActivity` 接线
- 在 `onWebViewCreate` 中注册 `AndroidSystemUiJsBridge`。
- 保持 `MainActivity` 仅做编排，不承载业务逻辑。

### 4.2 前端侧

#### 4.2.1 新增系统 UI 模块
新增 `src/scripts/mobile-system-ui.js`：
- 封装 Android JS 接口访问；
- 导出能力判断与读写函数：
  - `isMobileImmersiveFullscreenSupported()`
  - `setMobileImmersiveFullscreenEnabled(enabled)`
  - `getMobileImmersiveFullscreenEnabled()`

#### 4.2.2 菜单项
在 `src/index.html` 的 `#options` 菜单中新增：
- `id="option_toggle_fullscreen"`
- 文案：`切换全屏`

#### 4.2.3 `script.js` 集成
1. 初始化阶段：
   - 设置加载完成后，将 `power_user.mobile_immersive_fullscreen` 下发到原生桥。
   - 根据能力（Android + bridge 可用）显示/隐藏菜单项。
2. 菜单点击分支：
   - 点击 `option_toggle_fullscreen` 时翻转 `power_user.mobile_immersive_fullscreen`；
   - 立即调用原生桥应用；
   - `saveSettingsDebounced()` 持久化。

#### 4.2.4 `power-user.js` 持久化字段
- `power_user` 默认新增：`mobile_immersive_fullscreen: true`
- `loadPowerUserSettings()` 后兜底类型校正为布尔（仅当旧配置缺失时回到默认）。

## 5. 文件级改动清单
1. `src-tauri/gen/android/app/src/main/java/com/tauritavern/client/AndroidInsetsBridge.kt`
2. `src-tauri/gen/android/app/src/main/java/com/tauritavern/client/AndroidSystemUiJsBridge.kt`（新增）
3. `src-tauri/gen/android/app/src/main/java/com/tauritavern/client/MainActivity.kt`
4. `src/scripts/mobile-system-ui.js`（新增）
5. `src/scripts/power-user.js`
6. `src/script.js`
7. `src/index.html`

## 6. 验证计划

### 6.1 功能验证
1. Android 首次启动：系统栏默认隐藏。
2. 打开 `#options_button` -> 点击 `切换全屏`：
   - 沉浸全屏 -> 安全区模式：系统栏显示。
   - 安全区模式 -> 沉浸全屏：系统栏隐藏。
3. 重启应用：保留上次模式。

### 6.2 布局验证
1. 沉浸全屏下顶部/底部无遮挡异常（允许内容占满屏幕）。
2. 安全区模式下恢复现有 safe-area 行为。
3. 键盘弹出时输入区行为正常（`--tt-ime-bottom` 继续生效）。

### 6.3 回归验证
1. 扩展弹窗、侧边抽屉、聊天输入和消息滚动无行为回归。
2. 非 Android 环境无报错，菜单项不暴露或无效化。

## 7. 风险与控制
1. 风险：系统手势拉出临时系统栏后状态不一致。
   - 控制：统一由 `AndroidInsetsBridge` 单点管理可见性与 Insets 推送，`onResume/onConfigurationChanged` 重申状态。
2. 风险：前端与原生状态漂移。
   - 控制：前端设置加载后主动下发一次状态；菜单操作后立即下发并持久化。

## 8. 实施顺序
1. 先改 Android（桥接 + 模式控制 + insets 分支）。
2. 再改前端（模块 + 菜单 + 持久化）。
3. 本地构建与关键路径验证。
4. 最后补充变更说明。
