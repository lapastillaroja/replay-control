#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "$0")/.." && pwd)
cargo metadata --no-deps --format-version 1 --manifest-path "$repo_root/Cargo.toml" |
    python3 -c 'import json, sys; print(next(package["version"] for package in json.load(sys.stdin)["packages"] if package["name"] == "replay-control-app"))'
