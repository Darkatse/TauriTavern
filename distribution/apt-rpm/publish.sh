#!/usr/bin/env bash

set -euo pipefail

export AWS_REQUEST_CHECKSUM_CALCULATION="${AWS_REQUEST_CHECKSUM_CALCULATION:-when_required}"
export AWS_RESPONSE_CHECKSUM_VALIDATION="${AWS_RESPONSE_CHECKSUM_VALIDATION:-when_required}"

: "${RELEASE_ASSETS_DIR:?Missing RELEASE_ASSETS_DIR}"
: "${RELEASE_VERSION:?Missing RELEASE_VERSION}"
: "${RELEASE_TAG:?Missing RELEASE_TAG}"
: "${RELEASE_COMMIT:?Missing RELEASE_COMMIT}"
: "${R2_ENDPOINT:?Missing R2_ENDPOINT}"
: "${PACKAGES_R2_BUCKET:?Missing PACKAGES_R2_BUCKET}"
: "${LINUX_REPOSITORY_SIGNING_FPR:?Missing LINUX_REPOSITORY_SIGNING_FPR}"
: "${LINUX_REPOSITORY_GPG_PASSPHRASE_FILE:?Missing LINUX_REPOSITORY_GPG_PASSPHRASE_FILE}"

REPOSITORY_CHANNEL=${REPOSITORY_CHANNEL:-stable}
EXPECTED_DEB_VERSION=${EXPECTED_DEB_VERSION:-$RELEASE_VERSION}
EXPECTED_RPM_RELEASE=${EXPECTED_RPM_RELEASE:-}

case "$REPOSITORY_CHANNEL" in
    stable)
        apt_pool_path=pool/main/t/tauri-tavern
        manifest_name=repository-manifest.json
        rpm_repository_paths=(
            rpm/fedora/stable/x86_64
            rpm/fedora/stable/aarch64
            rpm/opensuse/16.0/x86_64
            rpm/opensuse/16.0/aarch64
        )
        ;;
    canary)
        apt_pool_path=pool/canary/main/t/tauri-tavern
        manifest_name=repository-manifest-canary.json
        rpm_repository_paths=(
            rpm/fedora/canary/x86_64
            rpm/fedora/canary/aarch64
            rpm/opensuse/16.0/canary/x86_64
            rpm/opensuse/16.0/canary/aarch64
        )
        ;;
    *)
        echo "Unsupported repository channel: $REPOSITORY_CHANNEL" >&2
        exit 1
        ;;
esac

for required_command in \
    apt-ftparchive aws createrepo_c dpkg-deb gpg gzip jq rpm rpmsign; do
    command -v "$required_command" >/dev/null 2>&1 || {
        echo "Missing required command: $required_command" >&2
        exit 1
    }
done

repository_dir=$(mktemp -d "${RUNNER_TEMP:-/tmp}/tauritavern-repository.XXXXXX")
trap 'rm -rf "$repository_dir"' EXIT

aws_r2() {
    aws --endpoint-url "$R2_ENDPOINT" s3 "$@"
}

sync_existing_packages() {
    mkdir -p "$repository_dir/apt/$apt_pool_path"
    aws_r2 sync \
        "s3://$PACKAGES_R2_BUCKET/apt/$apt_pool_path" \
        "$repository_dir/apt/$apt_pool_path"

    for repository_path in "${rpm_repository_paths[@]}"; do
        mkdir -p "$repository_dir/$repository_path/packages"
        aws_r2 sync \
            "s3://$PACKAGES_R2_BUCKET/$repository_path/packages" \
            "$repository_dir/$repository_path/packages"
    done
}

stage_deb_packages() {
    mapfile -d '' deb_assets < <(find "$RELEASE_ASSETS_DIR" -type f -name '*.deb' -print0)
    test "${#deb_assets[@]}" -eq 2 || {
        echo "Expected two DEB assets, found ${#deb_assets[@]}" >&2
        exit 1
    }

    for asset in "${deb_assets[@]}"; do
        package=$(dpkg-deb -f "$asset" Package)
        version=$(dpkg-deb -f "$asset" Version)
        architecture=$(dpkg-deb -f "$asset" Architecture)
        test "$package" = "tauri-tavern" || {
            echo "Unexpected DEB package name in $asset: $package" >&2
            exit 1
        }
        test "$version" = "$EXPECTED_DEB_VERSION" || {
            echo "Unexpected DEB version in $asset: $version" >&2
            exit 1
        }
        case "$architecture" in
            amd64 | arm64) ;;
            *)
                echo "Unexpected DEB architecture in $asset: $architecture" >&2
                exit 1
                ;;
        esac
        install -m 0644 "$asset" \
            "$repository_dir/apt/$apt_pool_path/tauri-tavern_${version}_${architecture}.deb"
    done
}

stage_rpm_packages() {
    mapfile -d '' rpm_assets < <(find "$RELEASE_ASSETS_DIR" -type f -name '*.rpm' -print0)
    test "${#rpm_assets[@]}" -eq 2 || {
        echo "Expected two RPM assets, found ${#rpm_assets[@]}" >&2
        exit 1
    }

    rpm_signature_db="$repository_dir/rpm-signature-db"
    signing_public_key="$repository_dir/repository-signing-public-key.asc"
    mkdir -p "$rpm_signature_db"
    gpg --batch --armor --export "$LINUX_REPOSITORY_SIGNING_FPR" >"$signing_public_key"
    rpm --dbpath "$rpm_signature_db" --initdb
    rpm --dbpath "$rpm_signature_db" --import "$signing_public_key"

    for asset in "${rpm_assets[@]}"; do
        package=$(rpm --dbpath "$rpm_signature_db" -qp --queryformat '%{NAME}' "$asset")
        version=$(rpm --dbpath "$rpm_signature_db" -qp --queryformat '%{VERSION}' "$asset")
        release=$(rpm --dbpath "$rpm_signature_db" -qp --queryformat '%{RELEASE}' "$asset")
        architecture=$(rpm --dbpath "$rpm_signature_db" -qp --queryformat '%{ARCH}' "$asset")
        test "$package" = "tauri-tavern" || {
            echo "Unexpected RPM package name in $asset: $package" >&2
            exit 1
        }
        test "$version" = "$RELEASE_VERSION" || {
            echo "Unexpected RPM version in $asset: $version" >&2
            exit 1
        }
        if [ -n "$EXPECTED_RPM_RELEASE" ] && [ "$release" != "$EXPECTED_RPM_RELEASE" ]; then
            echo "Unexpected RPM release in $asset: $release" >&2
            exit 1
        fi
        case "$architecture" in
            x86_64 | aarch64) ;;
            *)
                echo "Unexpected RPM architecture in $asset: $architecture" >&2
                exit 1
                ;;
        esac

        signed_rpm="$repository_dir/tauri-tavern-${version}-${release}.${architecture}.rpm"
        install -m 0644 "$asset" "$signed_rpm"
        rpmsign \
            --dbpath "$rpm_signature_db" \
            --define "_gpg_name ${LINUX_REPOSITORY_SIGNING_FPR}!" \
            --define "_gpg_sign_cmd_extra_args --batch --pinentry-mode loopback --passphrase-file $LINUX_REPOSITORY_GPG_PASSPHRASE_FILE" \
            --addsign \
            "$signed_rpm"
        signature_check=$(rpm --dbpath "$rpm_signature_db" --checksig "$signed_rpm")
        echo "$signature_check"
        grep -q 'digests signatures OK$' <<<"$signature_check"

        for distribution_path in "${rpm_repository_paths[@]}"; do
            [[ "$distribution_path" == */"$architecture" ]] || continue
            install -m 0644 "$signed_rpm" \
                "$repository_dir/$distribution_path/packages/$(basename "$signed_rpm")"
        done
    done
}

generate_apt_metadata() {
    apt_root="$repository_dir/apt"
    for architecture in amd64 arm64; do
        binary_dir="$apt_root/dists/$REPOSITORY_CHANNEL/main/binary-$architecture"
        mkdir -p "$binary_dir"
        (
            cd "$apt_root"
            apt-ftparchive -a "$architecture" packages "$apt_pool_path" \
                >"dists/$REPOSITORY_CHANNEL/main/binary-$architecture/Packages"
        )
        gzip -n -9 -c "$binary_dir/Packages" >"$binary_dir/Packages.gz"
    done

    (
        cd "$apt_root"
        apt-ftparchive \
            -o APT::FTPArchive::Release::Origin=TauriTavern \
            -o APT::FTPArchive::Release::Label=TauriTavern \
            -o APT::FTPArchive::Release::Suite="$REPOSITORY_CHANNEL" \
            -o APT::FTPArchive::Release::Codename="$REPOSITORY_CHANNEL" \
            -o APT::FTPArchive::Release::Architectures='amd64 arm64' \
            -o APT::FTPArchive::Release::Components=main \
            -o APT::FTPArchive::Release::Description="TauriTavern $REPOSITORY_CHANNEL repository" \
            release "dists/$REPOSITORY_CHANNEL" >"dists/$REPOSITORY_CHANNEL/Release"
    )

    gpg \
        --batch \
        --yes \
        --pinentry-mode loopback \
        --passphrase-file "$LINUX_REPOSITORY_GPG_PASSPHRASE_FILE" \
        --local-user "${LINUX_REPOSITORY_SIGNING_FPR}!" \
        --armor \
        --detach-sign \
        --output "$apt_root/dists/$REPOSITORY_CHANNEL/Release.gpg" \
        "$apt_root/dists/$REPOSITORY_CHANNEL/Release"
    gpg \
        --batch \
        --yes \
        --pinentry-mode loopback \
        --passphrase-file "$LINUX_REPOSITORY_GPG_PASSPHRASE_FILE" \
        --local-user "${LINUX_REPOSITORY_SIGNING_FPR}!" \
        --clearsign \
        --output "$apt_root/dists/$REPOSITORY_CHANNEL/InRelease" \
        "$apt_root/dists/$REPOSITORY_CHANNEL/Release"
}

generate_rpm_metadata() {
    for repository_path in "${rpm_repository_paths[@]}"; do
        repository_root="$repository_dir/$repository_path"
        createrepo_c --update "$repository_root"
        gpg \
            --batch \
            --yes \
            --pinentry-mode loopback \
            --passphrase-file "$LINUX_REPOSITORY_GPG_PASSPHRASE_FILE" \
            --local-user "${LINUX_REPOSITORY_SIGNING_FPR}!" \
            --armor \
            --detach-sign \
            --output "$repository_root/repodata/repomd.xml.asc" \
            "$repository_root/repodata/repomd.xml"
    done
}

write_manifest() {
    jq -n \
        --arg version "$RELEASE_VERSION" \
        --arg tag "$RELEASE_TAG" \
        --arg commit "$RELEASE_COMMIT" \
        --arg channel "$REPOSITORY_CHANNEL" \
        --arg deb_version "$EXPECTED_DEB_VERSION" \
        --arg rpm_release "$EXPECTED_RPM_RELEASE" \
        --arg published_at "$(date -u +'%Y-%m-%dT%H:%M:%SZ')" \
        '{
            schema_version: 1,
            channel: $channel,
            version: $version,
            deb_version: $deb_version,
            rpm_release: (if $rpm_release == "" then null else $rpm_release end),
            tag: $tag,
            commit: $commit,
            published_at: $published_at
        }' >"$repository_dir/$manifest_name"
}

verify_repository_metadata() {
    gpg --batch --verify \
        "$repository_dir/apt/dists/$REPOSITORY_CHANNEL/Release.gpg" \
        "$repository_dir/apt/dists/$REPOSITORY_CHANNEL/Release"
    gpg --batch --verify "$repository_dir/apt/dists/$REPOSITORY_CHANNEL/InRelease"

    for repository_path in "${rpm_repository_paths[@]}"; do
        repository_root="$repository_dir/$repository_path"
        gpg --batch --verify \
            "$repository_root/repodata/repomd.xml.asc" \
            "$repository_root/repodata/repomd.xml"
    done
}

upload_apt_repository() {
    aws_r2 sync \
        "$repository_dir/apt/pool" \
        "s3://$PACKAGES_R2_BUCKET/apt/pool" \
        --cache-control 'public, max-age=31536000, immutable'
    aws_r2 sync \
        "$repository_dir/apt/dists" \
        "s3://$PACKAGES_R2_BUCKET/apt/dists" \
        --exclude '*/Release' \
        --exclude '*/Release.gpg' \
        --exclude '*/InRelease' \
        --cache-control 'no-cache'
    for release_file in Release Release.gpg InRelease; do
        aws_r2 cp \
            "$repository_dir/apt/dists/$REPOSITORY_CHANNEL/$release_file" \
            "s3://$PACKAGES_R2_BUCKET/apt/dists/$REPOSITORY_CHANNEL/$release_file" \
            --cache-control 'no-cache'
    done
}

upload_rpm_repository() {
    for repository_path in "${rpm_repository_paths[@]}"; do
        repository_root="$repository_dir/$repository_path"
        destination="s3://$PACKAGES_R2_BUCKET/$repository_path"
        aws_r2 sync \
            "$repository_root/packages" \
            "$destination/packages" \
            --cache-control 'public, max-age=31536000, immutable'
        aws_r2 sync \
            "$repository_root/repodata" \
            "$destination/repodata" \
            --exclude 'repomd.xml' \
            --exclude 'repomd.xml.asc' \
            --cache-control 'public, max-age=31536000, immutable'
        aws_r2 cp \
            "$repository_root/repodata/repomd.xml.asc" \
            "$destination/repodata/repomd.xml.asc" \
            --cache-control 'no-cache'
        aws_r2 cp \
            "$repository_root/repodata/repomd.xml" \
            "$destination/repodata/repomd.xml" \
            --cache-control 'no-cache'
    done
}

sync_existing_packages
stage_deb_packages
stage_rpm_packages
generate_apt_metadata
generate_rpm_metadata
write_manifest
verify_repository_metadata

if [ "${TAURITAVERN_REPOSITORY_PREPARE_ONLY:-0}" = "1" ]; then
    echo "Prepared and verified TauriTavern $RELEASE_VERSION repository metadata"
    exit 0
fi

# Immutable payloads and ordinary metadata are uploaded before the signed
# release pointers, so a failed upload leaves clients on the previous snapshot.
upload_apt_repository
upload_rpm_repository
aws_r2 cp \
    "$repository_dir/$manifest_name" \
    "s3://$PACKAGES_R2_BUCKET/$manifest_name" \
    --cache-control 'no-cache'

echo "Published TauriTavern $RELEASE_VERSION ($REPOSITORY_CHANNEL) to $PACKAGES_R2_BUCKET"
