#!/usr/bin/env bash
set -euo pipefail
if [[ $# -gt 1 ]]; then
    echo 'usage: package-rpm.sh [VERSION]' >&2
    exit 1
fi
version=${1:-$(bash "$(dirname "$0")/app-version.sh")}
version=${version#v}
rpm_version=${version/-/\~}
target_root=${CARGO_TARGET_DIR:-target}
binary="$target_root/release/replay-control-app"
[[ -s "$binary" && -s catalog.sqlite && -d "$target_root/site/pkg" ]] || {
    echo 'Release binary, catalog, or site is missing' >&2
    exit 1
}
"$(dirname "$0")/verify-package-server-routes.sh" "$binary" "$target_root/site"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
payload="$work/payload"
mkdir -p "$payload/usr/bin" "$payload/usr/lib/replay-control" \
    "$payload/usr/share/replay-control" "$payload/usr/share/licenses/replay-control" \
    "$payload/usr/share/applications" "$payload/usr/share/icons/hicolor/192x192/apps" \
    "$payload/usr/lib/systemd/system" \
    "$work/BUILD" "$work/RPMS" "$work/SOURCES" "$work/SPECS" "$work/SRPMS"
install -m755 "$binary" "$payload/usr/bin/replay-control-app"
install -m644 catalog.sqlite "$payload/usr/share/replay-control/catalog.sqlite"
install -m644 LICENSE "$payload/usr/share/licenses/replay-control/LICENSE"
cp -a "$target_root/site" "$payload/usr/share/replay-control/site"
install -m755 packaging/start-service "$payload/usr/lib/replay-control/start-service"
install -m755 packaging/open-browser "$payload/usr/lib/replay-control/open-browser"
install -m755 packaging/switch-storage "$payload/usr/lib/replay-control/switch-storage"
install -m755 packaging/package-update-rpm "$payload/usr/lib/replay-control/package-update-rpm"
install -m644 packaging/replay-control.service "$payload/usr/lib/systemd/system/replay-control.service"
install -m644 packaging/replay-control.desktop "$payload/usr/share/applications/replay-control.desktop"
install -m644 replay-control-app/static/icons/icon-192.png \
    "$payload/usr/share/icons/hicolor/192x192/apps/replay-control.png"
rpmbuild -bb packaging/replay-control-rpm.spec \
    --define "_topdir $work" --define "payload $payload" \
    --define "version $rpm_version" --define '_build_id_links none'
cp "$work/RPMS/x86_64/"*.rpm "replay-control-${version}-1.x86_64.rpm"
