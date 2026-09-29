# Experimental OpenHarmony / HarmonyOS NEXT

This uses Tauri's experimental `feat/open-harmony` implementation, not an Android APK.
The native application is an ARM64 HAP. This is not stable Tauri support.

Use a **disposable checkout**: `prepare.py` replaces Cargo dependencies and capabilities
for OHOS only. Do not commit its generated manifest, lockfile, or capability changes.
Normal builds continue to use the existing stable Tauri dependencies.

## Build

On Linux, install Rust with `aarch64-unknown-linux-ohos`, Node 24+, pnpm, Python 3.11+
with `json5`, and Huawei command-line tools / SDK 6.0 (API 20). Set `DEVECO_SDK_HOME`
to the SDK directory and put the tools' `bin` on `PATH`.
Keep the experimental source directory **outside** this workspace.

```sh
export OHOS_TAURI_SOURCES=/absolute/path/to/ohos-tauri
export OHOS_HOME="$DEVECO_SDK_HOME/default/openharmony"
export TARGET_TRIPLE=aarch64-unknown-linux-ohos
python3 scripts/ohos/prepare-toolchain.py
cargo install --path "$OHOS_TAURI_SOURCES/tauri/crates/tauri-cli"
cargo install --locked ohrs --version 1.5.0
pnpm install --frozen-lockfile
python3 scripts/ohos/prepare.py
source scripts/ohos/env.sh
pnpm run web:build
export TAURITAVERN_SKIP_WEB_BUILD=1
node scripts/tauri-app.mjs ohos init --ci --skip-targets-install
python3 scripts/ohos/configure-hap.py
node scripts/tauri-app.mjs ohos build --ci --target aarch64 --ignore-version-mismatches -- --lib
```

`prepare-toolchain.py` fetches the revisions in `tauri-pins.json` into a new directory,
including the Ability revision used by upstream. It also fixes the experimental CLI's
SDK-directory lookup for the vendor layout. These sources and installed tools can be
cached between builds; `prepare.py` must run once in each fresh application checkout.
Hvigor uses the SDK's bundled Node executable; keep that directory on `PATH` during
`init` / `build` if required by your SDK. SDK resource tools also require `libGL.so.1`.

`configure-hap.py` sets the bundle identity, API level, ARM64 architecture and release
Rust callback, and disables signing and cloud backup in the generated project.
The output is under
`src-tauri/crates/tauritavern/gen/ohos/entry/build/default/outputs/default/`.
An unsigned HAP needs your own signing certificate and device profile before installation.

## Current boundaries

- Reuses the existing Rust application, frontend and embedded default resources.
- Data and logs live under the Ability's private `filesDir`, captured before Tauri
  consumes the Ability during runtime creation. Desktop portable/migration controls
  and desktop window/tray plugins are excluded.
- Upstream clipboard, notification, opener, file-dialog/filesystem and barcode plugins
  are not ported here. Plugin IPC is unavailable; notification commands return an error.
  Import/export flows requiring those plugins are not supported yet.
- New custom endpoints require a trusted native confirmation dialog. Since that dialog
  is unavailable, authorization is rejected instead of granting access silently.
- No background-generation service, device installation, lifecycle or runtime acceptance
  is implied by a successful build. HarmonyOS device validation is still required.
