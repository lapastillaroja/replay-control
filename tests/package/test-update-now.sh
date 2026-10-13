#!/usr/bin/env bash
set -euo pipefail

format=${1:?package format required}
case "$format" in
    arm64|amd64)
        package=$(find /root -maxdepth 1 -name "replay-control_*_${format}.deb" -print -quit)
        ;;
    rpm)
        package=$(find /root -maxdepth 1 -name 'replay-control-*-1.x86_64.rpm' -print -quit)
        ;;
    *) echo "Unsupported package format: $format" >&2; exit 1 ;;
esac
[[ -s "$package" ]] || { echo 'Package fixture is missing' >&2; exit 1; }
version=$(tr '~' '-' < /root/package-version)
port=$(sed -n 's/^REPLAY_HTTPS_PORT=//p' /etc/default/replay-control | tail -1)
case "$port" in
    \"*\") port=${port#\"}; port=${port%\"} ;;
    \'*\') port=${port#\'}; port=${port%\'} ;;
esac
[[ "$port" =~ ^[0-9]+$ ]] || port=8443
if [[ "$format" != rpm ]]; then
    stage=$(mktemp -d)
    dpkg-deb --raw-extract "$package" "$stage"
    sed -i 's/^Version:.*/Version: 0.0.1/' "$stage/DEBIAN/control"
    dpkg-deb --build --root-owner-group "$stage" /root/replay-control-old.deb >/dev/null
    rm -rf "$stage"
    DEBIAN_FRONTEND=noninteractive apt-get install -y --allow-downgrades /root/replay-control-old.deb >/dev/null
    test "$(dpkg-query -W -f='${Version}' replay-control)" = 0.0.1
fi
old_pid=$(cat /run/replay-control-test.pid)
mkdir -p /root/mock/repos/lapastillaroja/replay-control/releases/tags
name=$(basename "$package")
ln "$package" "/root/mock/$name"
(cd /root/mock && sha256sum "$name" > checksums.sha256)
python3 - "$name" <<'PY'
import json
import sys
from pathlib import Path

package = sys.argv[1]
version = Path("/root/package-version").read_text().strip().replace("~", "-")
base = "http://127.0.0.1:9000/"
names = [package, "checksums.sha256", "replay-control-app-aarch64-linux.tar.gz", "replay-site.tar.gz"]
release = {"assets": [{"name": name, "browser_download_url": base + name} for name in names]}
Path("/root/mock/repos/lapastillaroja/replay-control/releases/tags/v" + version).write_text(json.dumps(release))
PY
python3 -m http.server 9000 --bind 127.0.0.1 -d /root/mock >/var/log/replay-control-mock-github.log 2>&1 &
for _ in {1..30}; do
    if curl -fsS http://127.0.0.1:9000/checksums.sha256 >/dev/null; then break; fi
    sleep 1
done
curl -fsS http://127.0.0.1:9000/checksums.sha256 >/dev/null || {
    cat /var/log/replay-control-mock-github.log >&2
    exit 1
}
route=$(grep -aoE '/sfn/start_update[0-9]+' /usr/bin/replay-control-app | sed -n '1p')
[[ -n "$route" ]] || { echo 'StartUpdate server function route missing from binary' >&2; exit 1; }
started=false
for _ in {1..90}; do
    : > /tmp/replay-update-response
    status=$(curl -ksS --max-time 300 -o /tmp/replay-update-response -w '%{http_code}' -X POST \
        -H 'content-type: application/x-www-form-urlencoded' \
        -H 'accept: application/x-www-form-urlencoded' \
        --data "tag=v$version" "https://127.0.0.1:$port$route" 2>/dev/null || true)
    if [[ "$status" != 200 && ! -s /tmp/replay-update-response ]]; then
        sleep 2
        continue
    fi
    if [[ "$status" == 200 ]] && ! grep -q 'ServerError' /tmp/replay-update-response; then
        started=true
        break
    fi
    if grep -q 'Another operation is already running' /tmp/replay-update-response; then
        sleep 3
        continue
    fi
    cat /tmp/replay-update-response >&2
    tail -n 50 /var/log/replay-control-test.log >&2
    exit 1
done
if [[ "$started" != true ]]; then
    echo 'Update Now stayed busy after the startup scan' >&2
    exit 1
fi
for _ in {1..60}; do
    result=$(curl -kfsS "https://127.0.0.1:$port/api/update-result" 2>/dev/null || true)
    if [[ "$result" == "ok:$version" ]]; then
        if [[ "$format" == rpm ]]; then
            test "$(rpm -q --qf '%{VERSION}' replay-control)" = "$(cat /root/package-version)"
            test "$(cat /run/replay-control-test.pid)" != "$old_pid"
        else
            test "$(dpkg-query -W -f='${Version}' replay-control)" = "$(cat /root/package-version)"
            test "$(cat /run/replay-control-test.pid)" != "$old_pid"
        fi
        exit 0
    fi
    if [[ "$result" == failed:* ]]; then
        echo "$result" >&2
        cat /var/log/replay-control-package-update.log >&2
        exit 1
    fi
    sleep 2
done
cat /var/log/replay-control-package-update.log >&2
exit 1
