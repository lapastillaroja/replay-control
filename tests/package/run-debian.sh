#!/usr/bin/env bash
set -euo pipefail

arch=${1:?usage: tests/package/run-debian.sh ARCH PACKAGE (arm64 or amd64)}
package=${2:?usage: tests/package/run-debian.sh ARCH PACKAGE (arm64 or amd64)}
package=$(realpath "$package")
repo_root=$(cd "$(dirname "$0")/../.." && pwd)
case "$arch" in
    arm64) suite=test_replayos.py ;;
    amd64) suite=test_standalone.py ;;
    *) echo "Unsupported Debian architecture: $arch" >&2; exit 1 ;;
esac
cd "$repo_root"
exec python3 -m pytest "tests/package/$suite" -q -s \
    --package "$package" --format deb --arch "$arch"
