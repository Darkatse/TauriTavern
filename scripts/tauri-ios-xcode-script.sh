#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname "$0")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)"

# Xcode GUI builds do not inherit the same PATH as an interactive shell.
export PATH="/opt/homebrew/bin:/opt/homebrew/sbin:/usr/local/bin:/usr/local/sbin:$HOME/.volta/bin:$HOME/.cargo/bin:$HOME/.local/share/pnpm:$HOME/Library/pnpm:/usr/bin:/bin:/usr/sbin:/sbin:$PATH"

if [ -x /opt/homebrew/bin/brew ]; then
  eval "$(/opt/homebrew/bin/brew shellenv)"
elif [ -x /usr/local/bin/brew ]; then
  eval "$(/usr/local/bin/brew shellenv)"
fi

ensure_node_on_path() {
  if command -v node >/dev/null 2>&1; then
    return 0
  fi

  if command -v fnm >/dev/null 2>&1; then
    eval "$(fnm env --shell bash)"
    (
      cd "$REPO_ROOT"
      fnm use --install-if-missing --silent-if-unchanged >/dev/null 2>&1
    ) || true
  fi

  if command -v node >/dev/null 2>&1; then
    return 0
  fi

  for node_dir in /opt/homebrew/bin /usr/local/bin; do
    if [ -x "$node_dir/node" ]; then
      export PATH="$node_dir:$PATH"
      return 0
    fi
  done

  return 1
}

run_tauri_ios_xcode_script() {
  cd "$REPO_ROOT"
  "$@" tauri ios xcode-script \
    -v \
    --platform "${PLATFORM_DISPLAY_NAME:?}" \
    --sdk-root "${SDKROOT:?}" \
    --framework-search-paths "${FRAMEWORK_SEARCH_PATHS:?}" \
    --header-search-paths "${HEADER_SEARCH_PATHS:?}" \
    --gcc-preprocessor-definitions "${GCC_PREPROCESSOR_DEFINITIONS:-}" \
    --configuration "${CONFIGURATION:?}" \
    ${FORCE_COLOR:-} \
    ${ARCHS:?}
}

if ! ensure_node_on_path; then
  echo "error: node was not found in Xcode's PATH." >&2
  echo "error: install Node in a stable location, or configure fnm/volta so GUI apps can resolve it." >&2
  echo "error: PATH=$PATH" >&2
  exit 127
fi

if command -v pnpm >/dev/null 2>&1; then
  run_tauri_ios_xcode_script pnpm
  exit 0
fi

if command -v corepack >/dev/null 2>&1; then
  run_tauri_ios_xcode_script corepack pnpm
  exit 0
fi

echo "error: pnpm was not found in Xcode's PATH, and corepack is unavailable." >&2
echo "error: install pnpm in a standard location, or launch Xcode from a shell where Node and pnpm are configured." >&2
echo "error: PATH=$PATH" >&2
exit 127
