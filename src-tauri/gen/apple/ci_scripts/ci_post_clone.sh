#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname "$0")" && pwd)"
REPO_ROOT="${CI_WORKSPACE:-$(CDPATH= cd -- "$SCRIPT_DIR/../../../.." && pwd)}"

export HOMEBREW_NO_AUTO_UPDATE=1
export PATH="/opt/homebrew/bin:/opt/homebrew/sbin:/usr/local/bin:/usr/local/sbin:$HOME/.cargo/bin:$PATH"

if ! command -v brew >/dev/null 2>&1; then
  echo "error: Homebrew is required to prepare the Xcode Cloud environment." >&2
  exit 1
fi

if ! command -v node >/dev/null 2>&1; then
  brew install node
fi

if ! command -v corepack >/dev/null 2>&1; then
  npm install -g corepack
fi

corepack enable
corepack prepare pnpm@9 --activate

if ! command -v rustup >/dev/null 2>&1; then
  curl --proto '=https' --tlsv1.2 -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal
fi

export PATH="$HOME/.cargo/bin:$PATH"

rustup toolchain install stable --profile minimal
rustup default stable
rustup target add aarch64-apple-ios x86_64-apple-ios aarch64-apple-ios-sim

if ! command -v pod >/dev/null 2>&1; then
  brew install cocoapods
fi

cd "$REPO_ROOT"
pnpm install --frozen-lockfile

node -v
pnpm -v
cargo -V
rustup -V
pod --version
