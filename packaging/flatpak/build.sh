#!/usr/bin/env bash

set -euo pipefail

readonly APP_ID="com.tauritavern.client"
readonly MANIFEST="packaging/flatpak/${APP_ID}.yml"
readonly IDENTITY_FILE="packaging/flatpak/build-identity.env"
readonly BUILD_DIR="${TAURITAVERN_FLATPAK_BUILD_DIR:-.flatpak-build}"

die() {
    printf 'error: %s\n' "$*" >&2
    exit 1
}

command -v flatpak-builder >/dev/null 2>&1 ||
    die "flatpak-builder is required"
command -v git >/dev/null 2>&1 ||
    die "git is required"

repo_root="$(git rev-parse --show-toplevel 2>/dev/null)" ||
    die "run this script from the TauriTavern Git checkout"
cd "$repo_root"

[[ ! -e "$IDENTITY_FILE" ]] ||
    die "$IDENTITY_FILE already exists; remove it or preserve it elsewhere"

build_branch="${TAURITAVERN_BUILD_BRANCH:-}"
build_revision="${TAURITAVERN_BUILD_REVISION:-}"
if [[ -n "$build_branch" || -n "$build_revision" ]]; then
    [[ -n "$build_branch" && -n "$build_revision" ]] ||
        die "set both TAURITAVERN_BUILD_BRANCH and TAURITAVERN_BUILD_REVISION"
elif [[ -n "$(git status --porcelain=v1 --untracked-files=normal)" ]]; then
    build_branch="flatpak-local"
    build_revision="000000000000"
    printf 'Building a dirty checkout with local-only build identity.\n' >&2
else
    build_branch="$(git branch --show-current)"
    build_revision="$(git rev-parse --verify HEAD)"
fi
[[ "$build_branch" =~ ^[A-Za-z0-9._/-]+$ ]] ||
    die "cannot determine a valid build branch; set TAURITAVERN_BUILD_BRANCH"
[[ "$build_revision" =~ ^[0-9A-Fa-f]{7,64}$ ]] ||
    die "cannot determine a valid Git revision; set TAURITAVERN_BUILD_REVISION"

cleanup() {
    rm -f "$IDENTITY_FILE"
}
trap cleanup EXIT HUP INT TERM

printf "TAURITAVERN_BUILD_BRANCH='%s'\nTAURITAVERN_BUILD_REVISION='%s'\n" \
    "$build_branch" "$build_revision" >"$IDENTITY_FILE"

installation_args=()
case "${TAURITAVERN_FLATPAK_USER:-0}" in
    0) ;;
    1) installation_args+=(--user) ;;
    *) die "TAURITAVERN_FLATPAK_USER must be 0 or 1" ;;
esac

flatpak-builder \
    --assumeyes \
    --ccache \
    --force-clean \
    --install-deps-from=flathub \
    "${installation_args[@]}" \
    "$BUILD_DIR" \
    "$MANIFEST"

printf '\nFlatpak build completed.\n'
printf 'Run it without installing:\n'
printf '  flatpak-builder --run %q %q tauritavern\n' "$BUILD_DIR" "$MANIFEST"
