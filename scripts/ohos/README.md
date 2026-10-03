# Experimental OpenHarmony / HarmonyOS NEXT

This uses Tauri's experimental `feat/open-harmony` implementation, not an Android APK.
The native application is an ARM64 HAP. This is not stable Tauri support.

Use a **disposable checkout**: `prepare.py` replaces Cargo dependencies and capabilities
for OHOS only. Do not commit its generated manifest, lockfile, or capability changes.
Normal builds continue to use the existing stable Tauri dependencies.

## Build

The [OpenHarmony workflow](../../.github/workflows/ohos.yml) runs in this repository
using the public [tauri-harmony toolchain](https://github.com/LeenHawk/tauri-harmony),
pinned by digest. It needs no registry login or application signing secrets.
SDK and experimental Tauri installation are maintained in that independent repository.
`prepare.py` checks the image sources against `tauri-pins.json`, then loads the
[pinned native plugin fork](plugins-pin.json) and its pinned OHOS core bridge.
Their source stays outside the application workspace; the overlay is disposable.
The overlay applies compiler-warning fixes to the actual core/plugin sources and
includes only application runtime/build dependencies. HAP builds emit only a
`cdylib`, disable the desktop executable, and use fat LTO with one codegen unit.

To use the same image manually, run these commands from a disposable checkout mounted
inside the container (see the image repository for Docker usage):

```sh
pnpm install --frozen-lockfile
python3 scripts/ohos/prepare.py
export TARGET_TRIPLE=aarch64-unknown-linux-ohos
export CARGO_PROFILE_RELEASE_LTO=fat CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1
source scripts/ohos/env.sh
pnpm run web:build
export TAURITAVERN_SKIP_WEB_BUILD=1
export PATH="$HARMONY_TOOLS_DIR/command-line-tools/tool/node/bin:$HARMONY_TOOLS_DIR/command-line-tools/bin:$PATH"
node scripts/tauri-app.mjs ohos init --ci --skip-targets-install
python3 scripts/ohos/configure-hap.py
ohos_plugins_dir="$(cat src-tauri/crates/tauritavern/gen/ohos-plugins-source)"
python3 "$ohos_plugins_dir/shared/ohos/install.py" src-tauri/crates/tauritavern
node scripts/tauri-app.mjs ohos build --ci --target aarch64 --ignore-version-mismatches -- --lib
```

`configure-hap.py` sets the bundle identity, API level, ARM64 architecture and release
Rust callback, and disables signing and cloud backup in the generated project.
The output is under
`src-tauri/crates/tauritavern/gen/ohos/entry/build/default/outputs/default/`.
An unsigned HAP needs your own signing certificate and device profile before installation.

## Current boundaries

- Reuses the existing Rust application, frontend and embedded default resources.
- The OHOS core bridge resolves application/cache/temp paths from the native Ability.
  Desktop portable/migration controls and desktop window/tray plugins stay excluded.
- Clipboard text, native confirmation and file pickers, sandbox/picker-URI file IO,
  basic system notifications, external links and ScanKit QR scanning use the native
  plugin fork. New custom endpoints still require the original trusted confirmation.
- The fork's [capability matrix](https://github.com/LeenHawk/plugins-workspace/blob/feat/ohos-plugins/shared/ohos/README.md)
  documents mobile/API limits (such as rich clipboard content, scheduled notifications
  and ScanKit camera modes). ScanKit requires a compatible HarmonyOS device.
- No background-generation service, device installation, lifecycle or runtime acceptance
  is implied by a successful build. HarmonyOS device validation is still required.
