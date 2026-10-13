#!/usr/bin/env bash
set -euo pipefail

arch=${1:?usage: tests/package/run-debian.sh ARCH PACKAGE (ARCH: arm64 or amd64)}
package=${2:?usage: tests/package/run-debian.sh ARCH PACKAGE (ARCH: arm64 or amd64)}
package=$(realpath "$package")
[[ -s "$package" ]] || { echo "Package not found: $package" >&2; exit 1; }
actual_arch=$(dpkg-deb --field "$package" Architecture)
[[ "$actual_arch" == "$arch" ]] || {
    echo "Package architecture is $actual_arch, expected $arch" >&2
    exit 1
}
script_dir=$(cd "$(dirname "$0")" && pwd)
case "$arch" in
    arm64) exec "$script_dir/scenario-replayos-arm64.sh" "$package" ;;
    amd64)
        version=$(dpkg-deb --field "$package" Version)
        exec "$script_dir/scenario-standalone-amd64.sh" "$package" "$version"
        ;;
    *) echo "Unsupported Debian architecture: $arch" >&2; exit 1 ;;
esac
