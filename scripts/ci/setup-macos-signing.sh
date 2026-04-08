#!/usr/bin/env bash

set -euo pipefail

: "${RUNNER_TEMP:?Missing RUNNER_TEMP}"
: "${GITHUB_ENV:?Missing GITHUB_ENV}"
: "${APPLE_CERTIFICATE:?Missing APPLE_CERTIFICATE}"
: "${APPLE_CERTIFICATE_PASSWORD:?Missing APPLE_CERTIFICATE_PASSWORD}"
: "${KEYCHAIN_PASSWORD:?Missing KEYCHAIN_PASSWORD}"
: "${APPLE_API_KEY:?Missing APPLE_API_KEY}"
: "${APPLE_API_KEY_P8:?Missing APPLE_API_KEY_P8}"

certificate_path="${RUNNER_TEMP}/tauritavern-signing-certificate.p12"
keychain_path="${RUNNER_TEMP}/tauritavern-signing.keychain-db"
api_key_path="${RUNNER_TEMP}/AuthKey_${APPLE_API_KEY}.p8"

printf '%s' "${APPLE_CERTIFICATE}" | openssl base64 -d -A -out "${certificate_path}"

security create-keychain -p "${KEYCHAIN_PASSWORD}" "${keychain_path}"
security list-keychains -d user -s "${keychain_path}"
security default-keychain -s "${keychain_path}"
security unlock-keychain -p "${KEYCHAIN_PASSWORD}" "${keychain_path}"
security set-keychain-settings -lut 21600 "${keychain_path}"
security import "${certificate_path}" \
  -k "${keychain_path}" \
  -P "${APPLE_CERTIFICATE_PASSWORD}" \
  -T /usr/bin/codesign \
  -T /usr/bin/security
security set-key-partition-list \
  -S apple-tool:,apple:,codesign: \
  -s \
  -k "${KEYCHAIN_PASSWORD}" \
  "${keychain_path}"

signing_identity="$(
  security find-identity -v -p codesigning "${keychain_path}" \
    | awk -F'"' '/"/ { print $2; exit }'
)"
: "${signing_identity:?No code signing identity found in imported certificate}"

umask 077
printf '%s' "${APPLE_API_KEY_P8}" > "${api_key_path}"

{
  printf 'APPLE_SIGNING_IDENTITY=%s\n' "${signing_identity}"
  printf 'APPLE_API_KEY_PATH=%s\n' "${api_key_path}"
} >> "${GITHUB_ENV}"

echo "Configured macOS signing identity: ${signing_identity}"
