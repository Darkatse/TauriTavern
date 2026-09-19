# 系统 TTS 现状

Android WebView 缺少 Web Speech Synthesis 时，宿主在文档开始加载时补齐 `window.speechSynthesis` 与 `window.SpeechSynthesisUtterance`，让现有脚本直接使用标准调用。其他平台沿用 WebView 自带实现。网络 TTS provider 与 `/api/*` 路由不经过此兼容层。

## 接入位置

- `src-tauri/crates/tauritavern/src/platform/speech_synthesis.rs`：注册 Android 插件，通过 Tauri 的 all-frame 初始化脚本安装兼容对象。
- `src/tauri/main/compat/android-speech-synthesis.js`：提供 Web API 对象、事件和状态，串行提交原生命令。
- `src-tauri/crates/tauritavern/gen/android/app/src/main/java/com/tauritavern/client/SpeechSynthesisPlugin.kt`：连接系统默认 TTS 引擎，使用原生队列播放并回传进度。

插件命令权限在 host 的 `build.rs` 中声明，由 `capabilities/android-speech-synthesis.json` 仅向 Android 主窗口授予。

## 行为与边界

- 支持 `getVoices()`、`voiceschanged`、`speak()`、`cancel()`、`pending/speaking/paused`，以及 utterance 的 `text/lang/voice/rate/pitch/volume`、`start/end/error` 事件。事件支持属性处理器和 `addEventListener()`。
- 主页面和同源 iframe 共用一个系统语音队列，`cancel()` 会停止整个队列。跨源 frame 不获得此桥接。旧 WebView 缺少 `DOCUMENT_START_SCRIPT` 时，不能保证 `srcdoc` 等 frame 的首段脚本已获得兼容对象。
- 声音列表首次读取可能为空，初始化成功后通过 `voiceschanged` 更新；声音标识只在当前引擎中有意义。初始化失败不阻塞应用启动，后续调用可以重试。
- Android 原生队列负责播放顺序，进度回调决定 `start/end/error`。取消产生 `canceled` 或 `interrupted` 错误，已取消任务的迟到回调不再分发。
- `pitch=0` 映射到 Android 可接受的最低正值，具体音色范围由系统引擎决定。超过系统输入长度限制通过 `text-too-long` 报错。
- 首批不支持暂停/续播，`paused` 为 `false`，`pause()/resume()` 抛出 `NotSupportedError`。不承诺 SSML、词边界事件或后台持续播放。

## 内置 System provider

`src/scripts/extensions/tts/system.js` 使用相同的标准 API；`index.js` 监听声音列表变化刷新选项。分段朗读显式复制所需字段，失败向当前任务传播；短文本不会被丢弃，无空格长文本在无法匹配自然断点时按段长继续朗读。
