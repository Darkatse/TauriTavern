# OpenHarmony / HarmonyOS NEXT development

This experimental port builds unsigned ARM64 and x86_64 HAPs. The Rust backend
and the native WebView host build separately from normal stable Tauri targets.
A successful build is not evidence of installation or device compatibility.

## Source and HAR baseline

[`scripts/ohos/tauri-pins.json`](../scripts/ohos/tauri-pins.json) pins the actual
Tauri core, Wry, Tao and Ability sources used by the application. The plugin
checkout is pinned separately in
[`plugins-pin.json`](../scripts/ohos/plugins-pin.json). The digest in the
[OHOS workflow](../.github/workflows/ohos.yml) fixes the SDK and preinstalled CLI;
the CLI's source revision is not the runtime Tauri revision.

The plugin preparation helper selects these dependencies in a disposable checkout.
The Rust Ability dependency and the ArkTS HAR are built from the same Ability
checkout. `install.py --sources-only` packages that HAR, copies the native plugin
sources and creates their Rust module type declarations. Python bytecode is
excluded. TT does not patch third-party compiler warnings.

`prepare.py` also adds `openHarmony` to the two existing mobile capability platform
lists: the stable schema does not recognize that experimental platform enum.
The filesystem write scope remains unchanged. Picker URI export integration is
pending the host export API; this port does not grant general external writes.

## Checked-in project

[`gen/ohos`](../src-tauri/crates/tauritavern/gen/ohos) owns the application ID,
icons, unsigned profile, supported ABIs, disabled cloud backup, native plugin
startup and back-navigation bridge. `select-target.py` persists `TARGET_TRIPLE` as
one ABI in `entry/build-profile.json5` before building and clears staged libraries.
Hvigor reads this same ABI for its release Rust callback. It must not read the
parent environment: Tauri's process launcher filters `TARGET_TRIPLE`, which
previously made x86_64 builds silently package an ARM64 application library.
`validate-hap.py` rejects missing libraries, mixed ABIs and wrong ELF machines
before CI uploads a package.
`sync-version.py` only synchronizes the version from `PACKAGE_VERSION` or
`package.json`; do not regenerate the project with `ohos init`.

[`tauri.ohos.conf.json`](../src-tauri/crates/tauritavern/tauri.ohos.conf.json)
removes desktop bundle resource copies through a configuration overlay.
`embedded_resources` selects built-in resources for Android and OHOS; portable
desktop builds retain their disk-first behavior with an embedded fallback.
Release optimization settings remain in the common Cargo release profile.

## Build

Use a disposable checkout in the pinned public
[tauri-harmony container](https://github.com/LeenHawk/tauri-harmony):

```sh
pnpm install --frozen-lockfile
python3 scripts/ohos/prepare.py
export TARGET_TRIPLE=x86_64-unknown-linux-ohos # or aarch64-unknown-linux-ohos
source scripts/ohos/env.sh
export PATH="$HARMONY_TOOLS_DIR/command-line-tools/tool/node/bin:$HARMONY_TOOLS_DIR/command-line-tools/bin:$PATH"
python3 scripts/ohos/select-target.py
python3 scripts/ohos/sync-version.py
python3 "$(cat src-tauri/crates/tauritavern/gen/ohos-plugins-source)/shared/ohos/install.py" src-tauri/crates/tauritavern --sources-only
pnpm run web:build
export TAURITAVERN_SKIP_WEB_BUILD=1
node scripts/tauri-app.mjs ohos build --ci --target "${TARGET_TRIPLE%%-*}" --ignore-version-mismatches --config src-tauri/crates/tauritavern/tauri.ohos.conf.json -- --lib
```

The canary workflow calls the OHOS job with its prepared package version and asset
date. The OHOS workflow can also be dispatched manually, without publishing a
release. There is no OHOS push trigger. Both architectures upload unsigned HAPs;
signing with your own certificate and profile is required for installation.

## Host defects and validation

The former beta.0 HAR disabled DOM storage, so `localStorage` was null and frontend
initialization stopped. The pinned HAR now enables DOM storage and IndexedDB,
uses the Ability sandbox, and does not clear browser data during ordinary startup
or shutdown. Persistence after process restart still requires device validation.

The former Wry host discarded main-frame script flags and attributed proxy IPC
to the main page. Main-frame initialization now runs only in the top frame;
postMessage takes its frame URL synchronously from ArkWeb. Custom protocols use
ArkWeb frame metadata and preserve rejection of opaque origins. The native body
reader retains asynchronous buffers and collects chunks through EOF.

ArkWeb's `javaScriptOnDocumentStart` executes separate script items in
lexicographical order, not insertion order. Wry therefore submits one combined
item, retaining each script's main-frame guard. Splitting the Tauri bootstrap
into separate items caused `Object.defineProperty called on non-object` and a
missing `__TAURI_INTERNALS__.invoke`, preventing TT's frontend router from loading.
The combined script restores the internals/invoke/plugin initialization order.

Native browser callbacks cover file inputs, alert/confirm/prompt, external HTTP(S)
windows and fullscreen. Keyboard handling resizes the visual viewport. The HAP
uses the system safe-area layout. The native back bridge calls TT's existing
`__TAURITAVERN_HANDLE_BACK__` handler and backgrounds the Ability if unhandled.

No phone or emulator is attached to the build environment. The following are
**device acceptance requirements, not completed test results**:

- ARM64 phone and x86_64 simulator reach the main interface.
- localStorage and IndexedDB values survive force-stop and restart.
- An opaque sandbox iframe cannot invoke native commands over either IPC path.
- Create, send, save and reopen a chat without losing text or binary request data.
- Import a character card through `<input type="file">`; exported cards/chats are
  accessible in the system file manager after the host export integration lands.
- TT settings open; Back closes each dialog/drawer before backgrounding the app.
- Native dialogs, external links, fullscreen, media Range responses, keyboard and
  safe areas behave correctly on both devices.

The upstream maintainer owns TT's host identity consolidation and file import /
export routing. The two provisional user-agent regex additions were removed; the
PR must be rebased after that work lands. Background generation, LAN sync,
notifications, scanning and package-size optimization are outside this acceptance
baseline. Previous validation was build and archive inspection only, with no
successful phone launch claimed.
