#!/usr/bin/env bash

set -euo pipefail

export AWS_REQUEST_CHECKSUM_CALCULATION="${AWS_REQUEST_CHECKSUM_CALCULATION:-when_required}"
export AWS_RESPONSE_CHECKSUM_VALIDATION="${AWS_RESPONSE_CHECKSUM_VALIDATION:-when_required}"

readonly APP_ID="com.tauritavern.client"
readonly APP_ARCH="${FLATPAK_ARCH:-x86_64}"
readonly APP_BRANCH="stable"
readonly APP_REF="app/${APP_ID}/${APP_ARCH}/${APP_BRANCH}"

: "${FLATPAK_BUILD_DIR:?Missing FLATPAK_BUILD_DIR}"
: "${FLATPAK_R2_BUCKET:?Missing FLATPAK_R2_BUCKET}"
: "${FLATPAK_REPOSITORY_URL:?Missing FLATPAK_REPOSITORY_URL}"
: "${LINUX_REPOSITORY_KEY_FPR:?Missing LINUX_REPOSITORY_KEY_FPR}"
: "${LINUX_REPOSITORY_SIGNING_FPR:?Missing LINUX_REPOSITORY_SIGNING_FPR}"
: "${R2_ENDPOINT:?Missing R2_ENDPOINT}"
: "${RELEASE_COMMIT:?Missing RELEASE_COMMIT}"
: "${RELEASE_TAG:?Missing RELEASE_TAG}"
: "${RELEASE_VERSION:?Missing RELEASE_VERSION}"

readonly REPOSITORY_URL="${FLATPAK_REPOSITORY_URL%/}"
readonly GNUPG_HOME="${GNUPGHOME:-$(gpgconf --list-dirs homedir)}"

die() {
    printf 'error: %s\n' "$*" >&2
    exit 1
}

for required_command in aws base64 flatpak gpg gpgconf jq ostree; do
    command -v "$required_command" >/dev/null 2>&1 ||
        die "missing required command: $required_command"
done

[[ -d "$FLATPAK_BUILD_DIR/files" && -f "$FLATPAK_BUILD_DIR/metadata" ]] ||
    die "invalid Flatpak build directory: $FLATPAK_BUILD_DIR"
[[ "$APP_ARCH" =~ ^[A-Za-z0-9_]+$ ]] ||
    die "invalid FLATPAK_ARCH: $APP_ARCH"
[[ "$RELEASE_VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] ||
    die "invalid RELEASE_VERSION: $RELEASE_VERSION"
[[ "$RELEASE_COMMIT" =~ ^[0-9A-Fa-f]{40}$ ]] ||
    die "invalid RELEASE_COMMIT: $RELEASE_COMMIT"
[[ "$LINUX_REPOSITORY_KEY_FPR" =~ ^[0-9A-F]{40}$ ]] ||
    die "invalid LINUX_REPOSITORY_KEY_FPR"
[[ "$LINUX_REPOSITORY_SIGNING_FPR" =~ ^[0-9A-F]{40}$ ]] ||
    die "invalid LINUX_REPOSITORY_SIGNING_FPR"

gpg --batch --homedir "$GNUPG_HOME" --with-colons --with-subkey-fingerprint \
    --list-secret-keys "$LINUX_REPOSITORY_SIGNING_FPR" |
    awk -F: -v expected="$LINUX_REPOSITORY_SIGNING_FPR" \
        '$1 == "fpr" && $10 == expected { found = 1 } END { exit !found }' ||
    die "Flatpak signing subkey is unavailable"

work_dir="$(mktemp -d "${RUNNER_TEMP:-/tmp}/tauritavern-flatpak-repository.XXXXXX")"
remote_repository_dir="$work_dir/remote"
repository_dir=
public_key="$work_dir/tauritavern-repository.asc"
descriptor="$work_dir/tauritavern.flatpakrepo"
manifest="$work_dir/repository-manifest.json"

cleanup() {
    rm -rf "$work_dir"
}
trap cleanup EXIT HUP INT TERM

aws_r2() {
    aws --endpoint-url "$R2_ENDPOINT" s3 "$@"
}

mkdir -p "$remote_repository_dir"
aws_r2 sync \
    "s3://$FLATPAK_R2_BUCKET/repo" \
    "$remote_repository_dir" \
    --only-show-errors

if [[ -f "$remote_repository_dir/config" ]]; then
    repository_dir="$remote_repository_dir"
    repository_mode="$(ostree --repo="$repository_dir" config get core.mode)"
    [[ "$repository_mode" == "archive-z2" ]] ||
        die "unexpected remote OSTree repository mode: $repository_mode"
else
    unexpected_bootstrap_file="$(
        find "$remote_repository_dir" \
            -type f \
            ! -path "$remote_repository_dir/objects/*" \
            ! -path "$remote_repository_dir/deltas/*" \
            ! -path "$remote_repository_dir/summaries/*" \
            -print \
            -quit
    )"
    [[ -z "$unexpected_bootstrap_file" ]] ||
        die "remote repository has mutable metadata but no OSTree config: $unexpected_bootstrap_file"
    repository_dir="$work_dir/repo"
    mkdir -p "$repository_dir"
    ostree init --repo="$repository_dir" --mode=archive-z2
fi

# Object stores preserve files, not empty directories. OSTree creates this
# directory during init, and Flatpak requires it when listing refs for static
# delta generation on subsequent publishes.
mkdir -p "$repository_dir/refs/remotes"

release_subject="TauriTavern ${RELEASE_VERSION}"
release_tag_line="Release tag: ${RELEASE_TAG}"
source_commit_line="Source commit: ${RELEASE_COMMIT}"
release_body="${release_tag_line}"$'\n'"${source_commit_line}"

if ostree --repo="$repository_dir" rev-parse "$APP_REF" >/dev/null 2>&1; then
    current_commit="$(ostree --repo="$repository_dir" rev-parse "$APP_REF")"
    current_description="$(ostree --repo="$repository_dir" show "$current_commit")"
    if grep -Fqx "    $source_commit_line" <<<"$current_description"; then
        printf 'Release commit is already present: %s\n' "$current_commit"
    else
        if grep -Fqx "    $release_subject" <<<"$current_description"; then
            die "version $RELEASE_VERSION already points to a different source commit"
        fi
        current_commit=
    fi
else
    current_commit=
fi

if [[ -z "$current_commit" ]]; then
    release_timestamp="$(
        git show -s --format=%cI "$RELEASE_COMMIT" 2>/dev/null
    )" || die "unable to resolve release commit timestamp"

    flatpak build-export \
        --arch="$APP_ARCH" \
        --body="$release_body" \
        --gpg-homedir="$GNUPG_HOME" \
        --gpg-sign="$LINUX_REPOSITORY_SIGNING_FPR" \
        --subject="$release_subject" \
        --timestamp="$release_timestamp" \
        --update-appstream \
        "$repository_dir" \
        "$FLATPAK_BUILD_DIR" \
        "$APP_BRANCH"
    current_commit="$(ostree --repo="$repository_dir" rev-parse "$APP_REF")"
fi

gpg --batch --homedir "$GNUPG_HOME" --armor \
    --export "$LINUX_REPOSITORY_KEY_FPR" >"$public_key"
[[ -s "$public_key" ]] ||
    die "unable to export Flatpak repository public key"

flatpak build-update-repo \
    --comment="Official TauriTavern Flatpak repository" \
    --default-branch="$APP_BRANCH" \
    --description="Official stable Flatpak builds of TauriTavern" \
    --generate-static-deltas \
    --gpg-homedir="$GNUPG_HOME" \
    --gpg-import="$public_key" \
    --gpg-sign="$LINUX_REPOSITORY_SIGNING_FPR" \
    --homepage="https://github.com/Darkatse/TauriTavern" \
    --title="TauriTavern" \
    "$repository_dir"

[[ -s "$repository_dir/summary" && -s "$repository_dir/summary.sig" ]] ||
    die "signed Flatpak summary was not generated"

gpg_key="$(base64 --wrap=0 "$public_key")"
{
    printf '%s\n' \
        "[Flatpak Repo]" \
        "Title=TauriTavern" \
        "Comment=Official TauriTavern Flatpak repository" \
        "Description=Official stable Flatpak builds of TauriTavern" \
        "Url=${REPOSITORY_URL}/repo/" \
        "Homepage=https://github.com/Darkatse/TauriTavern" \
        "DefaultBranch=${APP_BRANCH}" \
        "GPGKey=${gpg_key}"
} >"$descriptor"

jq -n \
    --arg arch "$APP_ARCH" \
    --arg branch "$APP_BRANCH" \
    --arg commit "$RELEASE_COMMIT" \
    --arg flatpak_commit "$current_commit" \
    --arg ref "$APP_REF" \
    --arg signing_fingerprint "$LINUX_REPOSITORY_SIGNING_FPR" \
    --arg tag "$RELEASE_TAG" \
    --arg version "$RELEASE_VERSION" \
    --arg published_at "$(date -u +'%Y-%m-%dT%H:%M:%SZ')" \
    '{
        schema_version: 1,
        channel: "stable",
        version: $version,
        tag: $tag,
        commit: $commit,
        ref: $ref,
        arch: $arch,
        branch: $branch,
        flatpak_commit: $flatpak_commit,
        signing_fingerprint: $signing_fingerprint,
        published_at: $published_at
    }' >"$manifest"

verify_dir="$work_dir/verify"
export FLATPAK_USER_DIR="$verify_dir/user"
flatpak --user remote-add \
    --if-not-exists \
    --no-enumerate \
    --gpg-import="$public_key" \
    tauritavern-local \
    "file://$repository_dir"
verified_commit="$(
    flatpak --user remote-info \
        --arch="$APP_ARCH" \
        --show-commit \
        tauritavern-local \
        "$APP_ID"
)"
[[ "$verified_commit" == "$current_commit" ]] ||
    die "local Flatpak verification resolved $verified_commit instead of $current_commit"

# Publish content-addressed payloads before any metadata that can reference them.
aws_r2 sync \
    "$repository_dir" \
    "s3://$FLATPAK_R2_BUCKET/repo" \
    --exclude '*' \
    --include 'objects/*' \
    --include 'deltas/*' \
    --include 'summaries/*' \
    --cache-control 'public, max-age=31536000, immutable' \
    --only-show-errors

# These files may change in place, but they are not the final client entry points.
aws_r2 sync \
    "$repository_dir" \
    "s3://$FLATPAK_R2_BUCKET/repo" \
    --exclude 'objects/*' \
    --exclude 'deltas/*' \
    --exclude 'summaries/*' \
    --exclude '.lock' \
    --exclude 'summary' \
    --exclude 'summary.sig' \
    --exclude 'summary.idx' \
    --cache-control 'no-cache' \
    --only-show-errors

if [[ -f "$repository_dir/summary.idx" ]]; then
    aws_r2 cp \
        "$repository_dir/summary.idx" \
        "s3://$FLATPAK_R2_BUCKET/repo/summary.idx" \
        --cache-control 'no-cache' \
        --content-type 'application/octet-stream' \
        --only-show-errors
fi
aws_r2 cp \
    "$repository_dir/summary" \
    "s3://$FLATPAK_R2_BUCKET/repo/summary" \
    --cache-control 'no-cache' \
    --content-type 'application/octet-stream' \
    --only-show-errors
aws_r2 cp \
    "$repository_dir/summary.sig" \
    "s3://$FLATPAK_R2_BUCKET/repo/summary.sig" \
    --cache-control 'no-cache' \
    --content-type 'application/octet-stream' \
    --only-show-errors

aws_r2 cp \
    "$public_key" \
    "s3://$FLATPAK_R2_BUCKET/keys/tauritavern-repository.asc" \
    --cache-control 'no-cache' \
    --content-type 'application/pgp-keys' \
    --only-show-errors
aws_r2 cp \
    "$manifest" \
    "s3://$FLATPAK_R2_BUCKET/repository-manifest.json" \
    --cache-control 'no-cache' \
    --content-type 'application/json' \
    --only-show-errors
aws_r2 cp \
    "$descriptor" \
    "s3://$FLATPAK_R2_BUCKET/tauritavern.flatpakrepo" \
    --cache-control 'no-cache' \
    --content-type 'application/vnd.flatpak.repo' \
    --only-show-errors

printf 'Published %s at %s\n' "$APP_REF" "$current_commit"
