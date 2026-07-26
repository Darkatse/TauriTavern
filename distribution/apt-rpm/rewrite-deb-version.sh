#!/usr/bin/env bash

set -euo pipefail

if [ "$#" -ne 2 ]; then
    echo "Usage: $0 <package.deb> <version>" >&2
    exit 2
fi

package_path=$1
target_version=$2

command -v dpkg-deb >/dev/null 2>&1 || {
    echo "Missing required command: dpkg-deb" >&2
    exit 1
}
test -f "$package_path" || {
    echo "DEB package not found: $package_path" >&2
    exit 1
}
dpkg --validate-version "$target_version"

work_dir=$(mktemp -d "${RUNNER_TEMP:-/tmp}/tauritavern-deb.XXXXXX")
trap 'rm -rf "$work_dir"' EXIT

package_root="$work_dir/package"
rebuilt_package="$work_dir/rebuilt.deb"
dpkg-deb --raw-extract "$package_path" "$package_root"

control_file="$package_root/DEBIAN/control"
test -f "$control_file" || {
    echo "Missing DEBIAN/control in $package_path" >&2
    exit 1
}

awk -v version="$target_version" '
    BEGIN { replaced = 0 }
    /^Version: / {
        print "Version: " version
        replaced = 1
        next
    }
    { print }
    END {
        if (!replaced) {
            exit 1
        }
    }
' "$control_file" >"$control_file.new"
mv "$control_file.new" "$control_file"

dpkg-deb --build --root-owner-group "$package_root" "$rebuilt_package"
test "$(dpkg-deb -f "$rebuilt_package" Version)" = "$target_version"
install -m 0644 "$rebuilt_package" "$package_path"
