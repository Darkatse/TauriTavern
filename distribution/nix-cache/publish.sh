#!/usr/bin/env bash

set -euo pipefail

export AWS_REQUEST_CHECKSUM_CALCULATION="${AWS_REQUEST_CHECKSUM_CALCULATION:-when_required}"
export AWS_RESPONSE_CHECKSUM_VALIDATION="${AWS_RESPONSE_CHECKSUM_VALIDATION:-when_required}"

: "${NIX_CACHE_PRIVATE_KEY_BASE64:?Missing NIX_CACHE_PRIVATE_KEY_BASE64}"
: "${NIX_CACHE_PUBLIC_KEY:?Missing NIX_CACHE_PUBLIC_KEY}"
: "${R2_ENDPOINT:?Missing R2_ENDPOINT}"
: "${NIX_R2_BUCKET:?Missing NIX_R2_BUCKET}"

NIX_STORE_PATH=${NIX_STORE_PATH:-./result}
NIX_CACHE_URL=${NIX_CACHE_URL:-}
NIX_CACHE_URL=${NIX_CACHE_URL%/}

for required_command in aws base64 curl nix; do
    command -v "$required_command" >/dev/null 2>&1 || {
        echo "Missing required command: $required_command" >&2
        exit 1
    }
done

work_dir=$(mktemp -d "${RUNNER_TEMP:-/tmp}/tauritavern-nix-cache.XXXXXX")
trap 'rm -rf "$work_dir"' EXIT
key_file="$work_dir/private-key"
cache_dir="$work_dir/cache"
mkdir -p "$cache_dir"

umask 077
printf '%s' "$NIX_CACHE_PRIVATE_KEY_BASE64" | base64 --decode >"$key_file"
actual_public_key=$(nix key convert-secret-to-public <"$key_file")
test "$actual_public_key" = "$NIX_CACHE_PUBLIC_KEY" || {
    echo "Nix cache private key does not match NIX_CACHE_PUBLIC_KEY" >&2
    exit 1
}

store_roots=("$NIX_STORE_PATH")
for extra_path in "$@"; do
    nix path-info "$extra_path" >/dev/null
    store_name=$(basename "$extra_path")
    store_hash=${store_name%%-*}
    [[ "$store_hash" =~ ^[0-9a-df-np-sv-z]{32}$ ]] || {
        echo "Invalid Nix store path: $extra_path" >&2
        exit 1
    }
    : "${NIX_CACHE_URL:?Missing NIX_CACHE_URL for additional store paths}"
    response_code=$(curl \
        --silent \
        --show-error \
        --location \
        --retry 3 \
        --output /dev/null \
        --write-out '%{http_code}' \
        "$NIX_CACHE_URL/$store_hash.narinfo")
    case "$response_code" in
        200)
            echo "Already cached: $extra_path"
            ;;
        404)
            store_roots+=("$extra_path")
            ;;
        *)
            echo "Unable to query $extra_path in the public cache: HTTP $response_code" >&2
            exit 1
            ;;
    esac
done

nix path-info --recursive "${store_roots[@]}" |
    sort -u |
    nix store sign --stdin --key-file "$key_file"
nix path-info --sigs "${store_roots[@]}"
nix copy --to "file://$cache_dir" "${store_roots[@]}"

aws_r2() {
    aws --endpoint-url "$R2_ENDPOINT" s3 "$@"
}

aws_r2 sync \
    "$cache_dir/nar" \
    "s3://$NIX_R2_BUCKET/nar" \
    --cache-control 'public, max-age=31536000, immutable'
aws_r2 sync \
    "$cache_dir" \
    "s3://$NIX_R2_BUCKET" \
    --exclude '*' \
    --include '*.narinfo' \
    --cache-control 'public, max-age=31536000, immutable'
aws_r2 cp \
    "$cache_dir/nix-cache-info" \
    "s3://$NIX_R2_BUCKET/nix-cache-info" \
    --cache-control 'no-cache'
