#!/usr/bin/env bash

set -euo pipefail

readonly BUILDER_TOOLS_REPOSITORY="https://github.com/flatpak/flatpak-builder-tools.git"
readonly BUILDER_TOOLS_COMMIT="737c0085912f9f7dabf9341d4608e2a77a51a73a"
readonly PNPM_OUTPUT="packaging/flatpak/pnpm-sources.json"
readonly CARGO_OUTPUT="packaging/flatpak/cargo-sources.json"

die() {
    printf 'error: %s\n' "$*" >&2
    exit 1
}

mode="write"
case "${1:-}" in
    "")
        ;;
    --check)
        mode="check"
        ;;
    *)
        die "usage: $0 [--check]"
        ;;
esac

for command_name in git uv; do
    command -v "$command_name" >/dev/null 2>&1 ||
        die "$command_name is required"
done

repo_root="$(git rev-parse --show-toplevel 2>/dev/null)" ||
    die "run this script from the TauriTavern Git checkout"
cd "$repo_root"

temp_dir="$(mktemp -d "${TMPDIR:-/tmp}/tauritavern-flatpak-sources.XXXXXX")"
cleanup() {
    rm -rf "$temp_dir"
}
trap cleanup EXIT HUP INT TERM

tools_dir="$temp_dir/flatpak-builder-tools"
git init -q "$tools_dir"
git -C "$tools_dir" remote add origin "$BUILDER_TOOLS_REPOSITORY"
git -C "$tools_dir" fetch -q --depth 1 origin "$BUILDER_TOOLS_COMMIT"
git -C "$tools_dir" checkout -q --detach FETCH_HEAD

pnpm_generated="$temp_dir/pnpm-sources.json"
cargo_generated="$temp_dir/cargo-sources.json"

UV_LINK_MODE=copy uv run \
    --project "$tools_dir/node" \
    flatpak-node-generator \
    pnpm \
    --pnpm-store-version v10 \
    --node-sdk-extension org.freedesktop.Sdk.Extension.node22//25.08 \
    --output "$pnpm_generated" \
    pnpm-lock.yaml

UV_LINK_MODE=copy uv run \
    --project "$tools_dir/cargo" \
    python "$tools_dir/cargo/flatpak-cargo-generator.py" \
    src-tauri/Cargo.lock \
    --output "$cargo_generated"

if [[ "$mode" == "check" ]]; then
    status=0
    for pair in \
        "$pnpm_generated:$PNPM_OUTPUT" \
        "$cargo_generated:$CARGO_OUTPUT"; do
        generated="${pair%%:*}"
        committed="${pair#*:}"
        if [[ ! -f "$committed" ]] || ! cmp -s "$generated" "$committed"; then
            printf 'stale Flatpak source manifest: %s\n' "$committed" >&2
            status=1
        fi
    done
    exit "$status"
fi

install -Dm0644 "$pnpm_generated" "$PNPM_OUTPUT"
install -Dm0644 "$cargo_generated" "$CARGO_OUTPUT"
printf 'Updated %s and %s\n' "$PNPM_OUTPUT" "$CARGO_OUTPUT"
