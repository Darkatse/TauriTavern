# fastools upsync analyze summary

- Generated at: `2026-02-23T10:30:07.222331300+08:00`
- Base dir: `D:/Code/Github/TauriTavern/sillytavern-1.15.0/public`
- Target dir: `D:/Code/Github/TauriTavern/sillytavern-1.16.0/public`
- Local dir: `D:/Code/Github/TauriTavern/src`
- Route dir: `D:/Code/Github/TauriTavern/src/tauri/main/routes`
- Command registry: `D:/Code/Github/TauriTavern/src-tauri/src/presentation/commands/registry.rs`
- Out dir: `D:/Code/Github/TauriTavern/docs/upstream-sync-1.16-plan/reports`

## File Classification

- Upstream changed: **97**
- Local changed: **300**
- Both changed: **23**
- Upstream-only changed: **74**
- Local-only changed: **277**

### Both Changed (top 40)

- `css/extensions-panel.css`
- `index.html`
- `locales/zh-cn.json`
- `script.js`
- `scripts/autocomplete/AutoCompleteNameResultBase.js`
- `scripts/autocomplete/AutoCompleteOption.js`
- `scripts/backgrounds.js`
- `scripts/extensions.js`
- `scripts/extensions/tts/coqui.js`
- `scripts/group-chats.js`
- `scripts/itemized-prompts.js`
- `scripts/macros/engine/MacroRegistry.js`
- `scripts/personas.js`
- `scripts/power-user.js`
- `scripts/secrets.js`
- `scripts/slash-commands/SlashCommandClosure.js`
- `scripts/slash-commands/SlashCommandParser.js`
- `scripts/templates/welcomePanel.html`
- `scripts/util/AccountStorage.js`
- `scripts/utils.js`
- `scripts/welcome-screen.js`
- `scripts/world-info.js`
- `style.css`

## Injection Conflicts

- Total both-changed files: **23**
- Injection conflicts: **3**
- Conflict ratio: **13.04%**

### Injection Conflict Files (top 40)

- `index.html`: entrypoint_bootstrap, keyword_delta:init.js, keyword_delta:tauri-main.js
- `script.js`: keyword_delta:APP_INITIALIZED, keyword_delta:CHAT_LOADED, keyword_delta:chatLoaded, script_event_and_transport
- `scripts/extensions.js`: extension_runtime_integration

## Endpoint Gaps

- Endpoints in base: **308**
- Endpoints in target: **317**
- Route patterns: **98**
- New in target: **9**
- New in target but unhandled: **9**
- Target unhandled total: **231**

### New in Target but Unhandled

- `/api/backends/chat-completions/multimodal-models/moonshot`
- `/api/chats/group/info`
- `/api/image-metadata/all`
- `/api/openai/nanogpt/models/embedding`
- `/api/sd/comfy/rename-workflow`
- `/api/sd/sdcpp/generate`
- `/api/sd/sdcpp/ping`
- `/api/sd/zai/generate-video`
- `/api/volcengine/generate-voice`

## Command Gaps

- Invoked commands: **90**
- Registered commands: **137**
- Invoked but not registered: **0**

### Invoked but Not Registered

