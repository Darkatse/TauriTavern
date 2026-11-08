---
name: tt-app-guide
description: Finding your way around TauriTavern and helping a user with it — where each panel and setting lives, how to read chats, settings and other state from the page, how to show or change things in the live interface, and how to take a new user from a fresh install to a first chat. Read it before guiding a user through the app, reading their chat, or changing settings for them.
license: AGPL-3.0
---

# TauriTavern app guide

TauriTavern runs the SillyTavern 1.19.0 frontend on a Rust backend, as a native app for desktop, Android, iOS and HarmonyOS. Most of the interface is upstream SillyTavern; TauriTavern adds its own settings, data handling and platform behavior.

## How it differs from SillyTavern

- **Chat Completion only.** The API selector `#main_api` is fixed to Chat Completion and the Text Completion APIs are hidden. Requests are built from the Chat Completion preset's Prompt Manager; the instruct and context templates in Advanced Formatting apply only to Text Completion.
- **No Node server.** Server plugins and anything else that needs the SillyTavern server process do not exist here. Third-party extensions run in the page only (tt-extensions skill).
- **Platform features.** A movable data directory (desktop, needs a restart), LAN Sync and TT-Sync between devices, a one-time native approval for each custom API endpoint, and features turned off by policy in iOS TestFlight builds. The tt-docs skill has a document for each.
- **This assistant lives in the Advanced Formatting drawer.** The real Advanced Formatting settings sit behind it.

## Reading the current state

Read state from the page before inspecting the screen. It is exact and does not depend on what is visible.

- **`SillyTavern.getContext()`**:
  - `chat`: every message of the open chat, indexed from 0. The number shown on a message (`#200`) is its index, so that message is `chat[200]`. Read messages here, because the page renders only part of a long chat. When historical swipes load on demand (TauriTavern settings, Performance), a message's other swipes can be `null` until they are opened.
  - `characterId` (index into `characters`), `groupId`, `getCurrentChatId()`, `name1` (the user's persona), `name2` (the character).
  - `onlineStatus`: `'no_connection'` until a model is connected.
  - `chatCompletionSettings`: the connection source, model fields and active preset values. `getChatCompletionModel()` gives the model in use; `getPresetManager('openai').getSelectedPresetName()` gives the preset.
  - `powerUserSettings`, `extensionSettings`, `chatMetadata`, `getCurrentLocale()`.
  - Each call returns a new object. Plain values such as `characterId` are taken at that moment, so call it again after anything changes. `chat` and the settings objects are the app's live objects: read them freely, but change settings through the interface, because assigning into them neither updates the page nor saves the change.
- **TauriTavern's page API** is `window.__TAURITAVERN__.api`. What each module does: `references/API/README.md` in the tt-docs skill.
- **Saved settings**, as opposed to what the page holds in memory:
  - SillyTavern settings: `fetch('/api/settings/get', { method: 'POST', headers: SillyTavern.getContext().getRequestHeaders(), body: '{}' })` returns JSON whose `settings` field is the saved settings as a JSON string.
  - TauriTavern settings: `(await import('/tauri-bridge.js')).getTauriTavernSettings()`.

## Where things are

Each top bar drawer is a `.drawer` holding a `.drawer-toggle` and a `.drawer-content` panel.

| Drawer | Drawer element | Panel | What it holds |
|---|---|---|---|
| AI Response Configuration | `#ai-config-button` | `#left-nav-panel` | Chat Completion presets (`#settings_preset_openai`), response length (`#openai_max_tokens`) and sampling, the Prompt Manager (`#completion_prompt_manager`) |
| API Connections | `#sys-settings-button` | `#rm_api_block` | Connection profiles (`#connection_profiles`), source (`#chat_completion_source`), API key, endpoint, model, Connect (`#api_button_openai`), Test (`#test_api_button`) |
| Advanced Formatting | `#advanced-formatting-button` | `#AdvancedFormatting` | This assistant; behind it the context, instruct, system prompt, tokenizer and reasoning settings |
| World Info | `#WI-SP-button` | `#WorldInfo` | Active global lorebooks (`#world_info`), the lorebook editor (`#world_editor_select`) |
| User Settings | `#user-settings-button` | `#user-settings-block` | Language (`#ui_language_select`), settings search (`#settingsSearch`), theme and interface options, the TauriTavern settings button (`#tauritavern_settings_button`) |
| Backgrounds | `#backgrounds-button` | `#Backgrounds` | Chat backgrounds |
| Extensions | `#extensions-settings-button` | `#rm_extensions_block` | Manage extensions (`#extensions_details`), install (`#third_party_extension_button`), each extension's settings, the TauriTavern version panel |
| Persona Management | `#persona-management-button` | `#PersonaManagement` | The user's personas |
| Characters | `#rightNavHolder` | `#right-nav-panel` | Character list, create (`#rm_button_create`), import (`#character_import_button`), group chats |

Elsewhere:

- **TauriTavern settings** open as a popup from `#tauritavern_settings_button`.
  - Sections: Performance, Chat Backups, System (data directory, request proxy), Models, Misc and Development (logs, reload frontend). Windows adds Interface, and Sync appears when LAN sync is allowed.
  - The controls have no stable ids. Find a section by its `aria-label` and a row by its label, both in the user's language.
  - Most changes take effect when the popup is saved, and some of those reload the page.
  - Some act at once: choosing a data directory (after its own confirmation), the Development buttons and Sync.
  - The data directory and the API key exposure option (Misc) need an app restart.
- **Updates and the debug bundle.** They are in the TauriTavern version panel in the Extensions drawer, which is collapsed by default.
  - The update channel is the radio group `input[name="tauritavern_update_channel"]` (`stable` or `canary`); `#ttv-channel-toggle` only expands it.
  - The update check is `#tauritavern_check_update`, and the debug bundle export is `#tauritavern_export_debug_bundle`.
  - iOS builds may hide the update controls.
- **The extensions menu.** `#extensionsMenuButton`, next to the chat input, opens `#extensionsMenu`.

## Working in the live interface

- **Opening a drawer.** Click its `.drawer-toggle`. This closes every other unpinned drawer, including the Advanced Formatting drawer this assistant lives in.
  - When `<html>` has `data-tt-drawer-bar`, the system draws the drawer buttons beside the page and the toggles are hidden, so interact cannot reach them. Click the drawer's icon with evaluate instead, using the drawer element from the table above: `document.querySelector('#ai-config-button .drawer-icon').click()`.
  - Your run continues; the user sees the drawer you opened and gets your reply when they come back.
  - Read state instead of opening drawers when the user doesn't need to see the screen.
- **Detached panels.** With TauriTavern's Panel Runtime setting on, a closed drawer's content may be detached from the page. Open the drawer before looking inside it.
- **The real Advanced Formatting settings.** Switch this assistant's panel with its Advanced button (`.ttia-advanced`); `.ttia-return` switches back.
- **Labels are localized.** Locate controls by id, `data-i18n` key or structure. When you tell the user where to click, use the label they actually see.
- **Sliders.** Many settings pair a range slider with a number field whose id ends in `_counter` (for example `#font_scale` and `#font_scale_counter`).
  - Setting the slider saves the value, but some apply it only when the pointer is released.
  - Set the number field and commit it the way the user would, by changing it and pressing Enter.
- **Themes.** Appearance settings such as font scale and colors also belong to the current theme. They are saved as settings at once, but the theme file changes only when the theme is updated (`#ui-preset-update-button`), and switching themes brings back the theme's values. Ask whether the change should go into the theme.
- **Presets.** An edited Chat Completion preset stays unsaved until it is updated (`#update_oai_preset`). Ask whether to overwrite it or save it under a new name.
- **Saving settings.** SillyTavern settings save about a second after a change; `settings_updated` fires after a successful save. Read the saved settings back to confirm.
  - On disk, TauriTavern splits them into `default-user/settings.json` and files under `default-user/settings/`. The theme fields, such as font scale and colors, are in `appearance.json`; the others are `presets.json`, `layout.json` and `persona-state.json`.
  - `/api/settings/get` returns them merged in SillyTavern's format.
- **Popups.** A popup is a `dialog.popup`; the one in front is `dialog.popup[open]:not([closing])`. The button with `data-result="1"` confirms and `data-result="0"` cancels.
- **Things only the user can do.**
  - File pickers and native dialogs: importing a card or preset, choosing a folder, saving an export, approving a custom endpoint.
  - API keys and passwords: ask the user to type them into the field themselves. Never ask for them in this conversation, because everything here is sent to the model and logged.
- **Reloads.** These reload the page, so say so before doing them:
  - changing the interface language;
  - enabling or disabling an extension;
  - saving any TauriTavern Performance setting, or the original avatar images option.
- **Phones.** On narrow screens drawers fill the screen, and the pin buttons of the character and AI Response Configuration panels are hidden.

## From a fresh install to a first chat

1. **Connect a model** (API Connections).
   - If the user has set up a model for this assistant, it is already listed in `#connection_profiles` under Models; selecting it applies the same source, model and key.
   - Otherwise choose the source, have the user enter the API key (or the custom endpoint URL and key), click Connect, pick a model, then Test.
   - The first request to a custom endpoint opens a native dialog that only the user can approve.
   - Done when `onlineStatus` is not `'no_connection'` and the test succeeds.
2. **Choose a Chat Completion preset** (AI Response Configuration). The default works. Community presets change the Prompt Manager and sampling; importing one needs the user to pick the file.
3. **Pick a character.** A sample character is installed with the app. The user can also import a card file, or you can create one together. Open it from the character list. Done when `characterId` is set and a chat is open.
4. **Set the persona** name and description (Persona Management). This is who the character talks to.
5. **Send a message.** If the reply fails, follow the tt-troubleshooting skill.
6. **Then, as the user wants:**
   - world info (lorebooks);
   - extensions (tt-extensions skill);
   - themes (User Settings);
   - bringing data over from SillyTavern with the Data Migration panel in the Extensions drawer.
