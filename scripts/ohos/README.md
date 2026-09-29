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
`prepare.py` checks the source commits against `tauri-pins.json` before applying the
disposable dependency overlay.

To use the same image manually, run these commands from a disposable checkout mounted
inside the container (see the image repository for Docker usage):

```sh
pnpm install --frozen-lockfile
python3 scripts/ohos/prepare.py
export TARGET_TRIPLE=aarch64-unknown-linux-ohos
source scripts/ohos/env.sh
pnpm run web:build
export TAURITAVERN_SKIP_WEB_BUILD=1
export PATH="$HARMONY_TOOLS_DIR/command-line-tools/tool/node/bin:$HARMONY_TOOLS_DIR/command-line-tools/bin:$PATH"
node scripts/tauri-app.mjs ohos init --ci --skip-targets-install
python3 scripts/ohos/configure-hap.py
node scripts/tauri-app.mjs ohos build --ci --target aarch64 --ignore-version-mismatches -- --lib
```

`configure-hap.py` sets the bundle identity, API level, ARM64 architecture and release
Rust callback, and disables signing and cloud backup in the generated project.
The output is under
`src-tauri/crates/tauritavern/gen/ohos/entry/build/default/outputs/default/`.
An unsigned HAP needs your own signing certificate and device profile before installation.

## Current boundaries

- Reuses the existing Rust application, frontend and embedded default resources.
- Data, caches and logs live under the Ability's private `filesDir`, captured before Tauri
  consumes the Ability during runtime creation. Desktop portable/migration controls
  and desktop window/tray plugins are excluded.
- Upstream clipboard, notification, opener, file-dialog/filesystem and barcode plugins
  are not ported here. Plugin IPC is unavailable; notification commands return an error.
  Import/export flows requiring those plugins are not supported yet.
- New custom endpoints require a trusted native confirmation dialog. Since that dialog
  is unavailable, authorization is rejected instead of granting access silently.
- No background-generation service, device installation, lifecycle or runtime acceptance
  is implied by a successful build. HarmonyOS device validation is still required.
