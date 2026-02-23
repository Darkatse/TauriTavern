# fastools upsync analyze summary

- Generated at: `2026-02-23T12:02:21.971657500+08:00`
- Base dir: `D:/Code/Github/TauriTavern/sillytavern-1.15.0/public`
- Target dir: `D:/Code/Github/TauriTavern/sillytavern-1.16.0/public`
- Local dir: `D:/Code/Github/TauriTavern/src`
- Route dir: `D:/Code/Github/TauriTavern/src/tauri/main/routes`
- Command registry: `D:/Code/Github/TauriTavern/src-tauri/src/presentation/commands/registry.rs`
- Out dir: `D:/Code/Github/TauriTavern/docs/upstream-sync-1.16-plan/reports`

## File Classification

- Upstream changed: **97**
- Local changed: **374**
- Both changed: **97**
- Upstream-only changed: **0**
- Local-only changed: **277**

### Both Changed (top 40)

- `css/backgrounds.css`
- `css/extensions-panel.css`
- `css/macros.css`
- `css/tags.css`
- `css/toggle-dependent.css`
- `css/user.css`
- `css/welcome.css`
- `error/forbidden-by-whitelist.html`
- `error/host-not-allowed.html`
- `error/unauthorized.html`
- `error/url-not-found.html`
- `global.d.ts`
- `index.html`
- `locales/fr-fr.json`
- `locales/zh-cn.json`
- `script.js`
- `scripts/PromptManager.js`
- `scripts/RossAscends-mods.js`
- `scripts/authors-note.js`
- `scripts/autocomplete/AutoComplete.js`
- `scripts/autocomplete/AutoCompleteNameResultBase.js`
- `scripts/autocomplete/AutoCompleteOption.js`
- `scripts/autocomplete/EnhancedMacroAutoCompleteOption.js`
- `scripts/autocomplete/MacroAutoComplete.js`
- `scripts/autocomplete/MacroAutoCompleteHelper.js`
- `scripts/backgrounds.js`
- `scripts/bookmarks.js`
- `scripts/cfg-scale.js`
- `scripts/chat-templates.js`
- `scripts/chats.js`
- `scripts/events.js`
- `scripts/extensions-slashcommands.js`
- `scripts/extensions.js`
- `scripts/extensions/assets/index.js`
- `scripts/extensions/caption/index.js`
- `scripts/extensions/caption/settings.html`
- `scripts/extensions/memory/index.js`
- `scripts/extensions/quick-reply/index.js`
- `scripts/extensions/quick-reply/src/SlashCommandHandler.js`
- `scripts/extensions/shared.js`

## Injection Conflicts

- Total both-changed files: **97**
- Injection conflicts: **3**
- Conflict ratio: **3.09%**

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

