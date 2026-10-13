#!/usr/bin/env bash
set -euo pipefail

arch=${1:?usage: tests/package/run-fedora.sh ARCH PACKAGE (ARCH: x86_64)}
package=${2:?usage: tests/package/run-fedora.sh ARCH PACKAGE (ARCH: x86_64)}
[[ "$arch" == x86_64 ]] || { echo "Unsupported Fedora architecture: $arch" >&2; exit 1; }
package=$(realpath "$package")
[[ -s "$package" && "$package" == *.x86_64.rpm ]] || {
    echo "Expected an x86_64 RPM: $package" >&2
    exit 1
}
script_dir=$(cd "$(dirname "$0")" && pwd)
exec "$script_dir/scenario-fedora-x86_64.sh" "$package"
