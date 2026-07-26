#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
ACTION="${1:-}"
PLATFORM="${2:-}"

if [[ "${ACTION}" != "enable" && "${ACTION}" != "disable" ]]; then
  echo "Usage: $0 <enable|disable> <android|ios>" >&2
  exit 1
fi

if [[ "${PLATFORM}" != "android" && "${PLATFORM}" != "ios" ]]; then
  echo "Usage: $0 <enable|disable> <android|ios>" >&2
  exit 1
fi

replace_once() {
  local file="$1"
  local from="$2"
  local to="$3"

  [[ -f "${file}" ]] || { echo "Missing file: ${file}" >&2; exit 1; }
  TT_FROM="${from}" TT_TO="${to}" perl -0pi -e '
    BEGIN {
      $from = $ENV{"TT_FROM"};
      $to = $ENV{"TT_TO"};
    }
    index($_, $from) >= 0 or die "expected source text not found\n";
    s/\Q$from\E/$to/;
  ' "${file}"
}

contains_block() {
  TT_TEXT="$2" perl -0ne 'exit(index($_, $ENV{"TT_TEXT"}) >= 0 ? 0 : 1)' "$1"
}

toggle_block() {
  local file="$1"
  local disabled="$2"
  local enabled="$3"

  [[ -f "${file}" ]] || { echo "Missing file: ${file}" >&2; exit 1; }
  if [[ "${ACTION}" == "enable" ]]; then
    contains_block "${file}" "${enabled}" && return
    replace_once "${file}" "${disabled}" "${enabled}"
    return
  fi

  contains_block "${file}" "${enabled}" || return
  replace_once "${file}" "${enabled}" "${disabled}"
}

configure_android() {
  local gradle="${ROOT_DIR}/src-tauri/crates/tauritavern/gen/android/app/build.gradle.kts"
  local activity="${ROOT_DIR}/src-tauri/crates/tauritavern/gen/android/app/src/main/java/com/tauritavern/client/MainActivity.kt"

  toggle_block \
    "${gradle}" \
    $'        getByName("release") {\n' \
    $'        getByName("release") {\n            manifestPlaceholders["usesCleartextTraffic"] = "true"\n'
  toggle_block \
    "${activity}" \
    $'  override fun onWebViewCreate(webView: WebView) {\n    this.webView = webView\n' \
    $'  override fun onWebViewCreate(webView: WebView) {\n    this.webView = webView\n    webView.settings.mixedContentMode = WebSettings.MIXED_CONTENT_ALWAYS_ALLOW\n'
  toggle_block \
    "${activity}" \
    $'import android.view.ViewGroup\nimport android.webkit.WebView\n' \
    $'import android.view.ViewGroup\nimport android.webkit.WebSettings\nimport android.webkit.WebView\n'
}

configure_ios() {
  local disabled=$'\t<key>ITSAppUsesNonExemptEncryption</key>\n\t<false/>\n'
  local enabled=$'\t<key>ITSAppUsesNonExemptEncryption</key>\n\t<false/>\n\t<key>NSAppTransportSecurity</key>\n\t<dict>\n\t\t<key>NSAllowsArbitraryLoadsInWebContent</key>\n\t\t<true/>\n\t</dict>\n'
  local file

  for file in \
    "${ROOT_DIR}/src-tauri/crates/tauritavern/Info.ios.plist" \
    "${ROOT_DIR}/src-tauri/crates/tauritavern/gen/apple/tauritavern_iOS/Info.plist"; do
    toggle_block "${file}" "${disabled}" "${enabled}"
    plutil -lint "${file}" >/dev/null
  done
}

"configure_${PLATFORM}"
