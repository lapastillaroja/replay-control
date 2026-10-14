#!/usr/bin/env bash
set -euo pipefail

arch=${1:?usage: tests/package/run-fedora.sh ARCH PACKAGE (x86_64)}
package=${2:?usage: tests/package/run-fedora.sh ARCH PACKAGE (x86_64)}
[[ "$arch" == x86_64 ]] || { echo "Unsupported Fedora architecture: $arch" >&2; exit 1; }
package=$(realpath "$package")
repo_root=$(cd "$(dirname "$0")/../.." && pwd)
cd "$repo_root"
exec python3 -m pytest tests/package/test_standalone.py -q -s \
    --package "$package" --format rpm --arch x86_64
