# Flatpak packaging

The manifest builds TauriTavern from source inside the GNOME SDK. JavaScript
and Rust dependencies are downloaded by `flatpak-builder`, then consumed
offline during the build.

## Build

Install `flatpak-builder`, add Flathub, then run:

```bash
pnpm run flatpak:build
```

The wrapper records the current Git branch and revision, installs the matching
SDK dependencies, and writes the result to `.flatpak-build`. Run the result
without installing it:

```bash
flatpak-builder --run \
  .flatpak-build \
  packaging/flatpak/com.tauritavern.client.yml \
  tauritavern
```

Set `TAURITAVERN_FLATPAK_USER=1` to install missing SDK dependencies into the
current user's Flatpak installation. CI uses this mode so its SDK and source
downloads can be cached without elevated permissions.

Set `TAURITAVERN_BUILD_BRANCH` and `TAURITAVERN_BUILD_REVISION` explicitly for
CI or detached checkouts. A dirty local checkout receives the explicit
`flatpak-local` / `000000000000` identity instead of claiming to represent the
current commit exactly.

## Dependency manifests

`pnpm-sources.json` and `cargo-sources.json` are generated from the repository
lockfiles with a pinned revision of `flatpak-builder-tools`. Regenerate them
after either lockfile changes:

```bash
pnpm run flatpak:sources
```

Generation requires `git` and `uv`. CI can verify that committed manifests are
current with `pnpm run flatpak:sources:check`.

## Sandbox

The application uses desktop portals for user-selected files and directories.
The manifest deliberately does not grant direct access to the home directory.
It temporarily grants read-write access to the XDG Downloads directory so
desktop WebView exports remain visible outside the sandbox; remove this grant
after all desktop exports use the save-file portal. Network, graphics, audio,
Wayland/X11 fallback, and notifications are enabled because they are part of
the current desktop feature set.
