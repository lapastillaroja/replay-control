#!/usr/bin/env bash
set -euo pipefail

if [[ $# -gt 1 ]]; then
    echo 'usage: package-flatpak.sh [VERSION]' >&2
    exit 1
fi
version=${1:-$(bash "$(dirname "$0")/app-version.sh")}
version=${version#v}
target_root=${CARGO_TARGET_DIR:-target}
binary="$target_root/release/replay-control-app"
site="$target_root/site"
[[ -s "$binary" && -s catalog.sqlite && -d "$site/pkg" ]] || {
    echo 'Build the amd64 server, site, and catalog before packaging.' >&2
    exit 1
}
command -v flatpak-builder >/dev/null || {
    echo 'flatpak-builder is required.' >&2
    exit 1
}
command -v flatpak >/dev/null || {
    echo 'flatpak is required.' >&2
    exit 1
}
"$(dirname "$0")/verify-package-server-routes.sh" "$binary" "$site"
python3 - <<'PY'
import sqlite3

with sqlite3.connect("file:catalog.sqlite?mode=ro", uri=True) as catalog:
    if catalog.execute("PRAGMA journal_mode").fetchone()[0] != "delete":
        raise SystemExit("The catalog must use DELETE journal mode for Flatpak's read-only /app filesystem. Rebuild it with build.sh.")
PY

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
payload="$work/payload"
mkdir -p "$payload"
install -m755 "$binary" "$payload/replay-control-app"
install -m755 packaging/flatpak/replay-control "$payload/replay-control"
install -m644 catalog.sqlite "$payload/catalog.sqlite"
cp -a "$site" "$payload/site"
install -m644 packaging/flatpak/io.github.lapastillaroja.ReplayControl.desktop "$payload/"
install -m644 packaging/flatpak/io.github.lapastillaroja.ReplayControl.metainfo.xml "$payload/"
install -m644 packaging/flatpak/icon-128.png "$payload/icon-128.png"
cp packaging/flatpak/io.github.lapastillaroja.ReplayControl.json "$work/manifest.json"

flatpak-builder --force-clean --user --install-deps-from=flathub \
    --repo="$work/repo" "$work/build" "$work/manifest.json"
output="replay-control-${version}-x86_64.flatpak"
flatpak build-bundle "$work/repo" "$output" \
    io.github.lapastillaroja.ReplayControl \
    --runtime-repo=https://dl.flathub.org/repo/flathub.flatpakrepo
echo "Built $output"
