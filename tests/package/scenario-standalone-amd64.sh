#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "$0")/../.." && pwd)
package=$(realpath "${1:?usage: scenario-standalone-amd64.sh PACKAGE VERSION}")
expected=${2:?expected application version required}
expected_app=${expected/\~/-}
engine=${CONTAINER_ENGINE:-podman}
base_image=${PACKAGE_TEST_BASE:-debian:trixie}
image=replay-control-package-test:$(tr ':/' '--' <<< "$base_image")-amd64
container="replay-control-package-amd64-$$"
cleanup() {
    local status=$?
    if [[ ${KEEP_FAILED_PACKAGE_CONTAINER:-0} == 1 && $status -ne 0 ]]; then
        echo "Keeping failed package container $container" >&2
        return
    fi
    "$engine" rm -f "$container" >/dev/null 2>&1 || true
}
trap cleanup EXIT

"$engine" build --platform linux/amd64 -t "$image" \
    --build-arg "BASE=$base_image" \
    -f "$repo_root/tests/package/Dockerfile" "$repo_root/tests/package"
network_args=()
if [[ ${RUN_BROWSER_E2E:-0} == 1 ]]; then
    if [[ "$engine" == podman ]]; then
        network_args=(--network podman)
    else
        network_args=(-p 127.0.0.1::8080 -p 127.0.0.1::8443)
    fi
fi
"$engine" run -d --name "$container" --platform linux/amd64 \
    "${network_args[@]}" "$image" sleep infinity >/dev/null
"$engine" cp "$repo_root/tests/package/systemctl-shim" "$container:/usr/bin/systemctl"
"$engine" cp "$repo_root/tests/package/systemd-run-shim" "$container:/usr/local/bin/systemd-run"
"$engine" cp "$repo_root/tests/package/systemd-run-shim" "$container:/usr/bin/systemd-run"
"$engine" cp "$repo_root/tests/package/test-update-now.sh" "$container:/root/test-update-now.sh"
"$engine" cp "$repo_root/tests/package/test-desktop-launcher.sh" "$container:/root/test-desktop-launcher.sh"
"$engine" exec "$container" chmod +x /usr/bin/systemctl
"$engine" exec "$container" chmod +x /usr/local/bin/systemd-run
"$engine" exec "$container" chmod +x /usr/bin/systemd-run
"$engine" cp "$package" "$container:/root/$(basename "$package")"
"$engine" exec "$container" sh -c "printf '%s\\n' '$(dpkg-deb --field "$package" Version)' > /root/package-version"
"$engine" exec "$container" bash -lc '
    set -e
    mkdir -p /srv/roms /srv/other/roms /srv/other/bios /srv/fail/roms
    mkdir -p /var/lib/replay-control/cache
    cat > /var/lib/replay-control/cache/launchbox-metadata.xml <<EOF
<?xml version="1.0"?>
<LaunchBox><Game><Name>Package Test Game</Name><DatabaseID>424242</DatabaseID><Platform>Nintendo Entertainment System</Platform><Overview>Package integration fixture.</Overview></Game></LaunchBox>
EOF
    printf "REPLAY_PORT=8080\nREPLAY_HTTPS_PORT=8443\nREPLAY_CONTROL_SKIP_FIRST_RUN_SEED=1\n" > /etc/default/replay-control
    apt-get update -qq
    DEBIAN_FRONTEND=noninteractive apt-get install -y /root/replay-control_*_amd64.deb
    test -s /usr/share/icons/hicolor/192x192/apps/replay-control.png
    grep -Fxq "Exec=/usr/lib/replay-control/open-browser" /usr/share/applications/replay-control.desktop
    dpkg-query -S /usr/share/applications/replay-control.desktop \
        /usr/share/icons/hicolor/192x192/apps/replay-control.png \
        /usr/lib/replay-control/package-update-deb \
        /usr/lib/replay-control/open-browser | grep -q replay-control:
    bash /root/test-desktop-launcher.sh
    for i in {1..30}; do
        if curl -kfsS https://127.0.0.1:8443/storage-setup >/dev/null; then break; fi
        sleep 2
    done
    curl -kfsS https://127.0.0.1:8443/storage-setup | grep -q "Choose your storage location"
    curl -kfsS https://127.0.0.1:8443/storage-setup | grep -q "STEP 1 OF 2"
    curl -kfsS https://127.0.0.1:8443/storage-setup/app.js | grep -Fq "/?setup"
    curl -kfsS https://127.0.0.1:8443/storage-setup/app.js | grep -Fq "/storage-setup/messages"
    curl -kfsS -H "Accept-Language: es-ES,es;q=0.9" \
        https://127.0.0.1:8443/storage-setup/messages |
        grep -Fq "\"title\":\"Elige la ubicación de almacenamiento\""
    mkdir -p /etc/replay-control
    if test -e /etc/replay-control/settings.cfg; then
        cp /etc/replay-control/settings.cfg /tmp/locale-settings-before
    fi
    printf "locale = \"ja\"\n" > /etc/replay-control/settings.cfg
    curl -kfsS -H "Accept-Language: es-ES" \
        https://127.0.0.1:8443/storage-setup/messages |
        grep -Fq "\"locale\":\"ja\""
    if test -e /tmp/locale-settings-before; then
        mv /tmp/locale-settings-before /etc/replay-control/settings.cfg
    else
        rm /etc/replay-control/settings.cfg
    fi
    curl -kfsS "https://127.0.0.1:8443/storage-setup/list?path=/srv" | grep -q "\"valid\":true"
    test "$(curl -ksS -o /dev/null -w "%{http_code}" -X POST \
        -H "content-type: application/json" \
        -d "{\"path\":\"/srv\"}" \
        https://127.0.0.1:8443/storage-setup/select)" = 202
    for i in {1..30}; do
        if curl -kfsS https://127.0.0.1:8443/api/version >/dev/null 2>&1 && \
            grep -qx /srv /var/lib/replay-control/standalone-storage-path && \
            grep -qx ok /var/lib/replay-control/storage-switch-status 2>/dev/null; then break; fi
        sleep 2
    done
    grep -qx /srv /var/lib/replay-control/standalone-storage-path
    curl -kfsS https://127.0.0.1:8443/api/version >/dev/null
    curl -kfsS "https://127.0.0.1:8443/?setup" | grep -q "Choose your game library folder"
    for i in {1..30}; do
        if python3 -c "import sqlite3; db=sqlite3.connect(\"/var/lib/replay-control/external_metadata.db\"); assert db.execute(\"SELECT COUNT(*) FROM provider_game WHERE provider = ?\", (\"launchbox\",)).fetchone()[0] == 1" 2>/dev/null; then break; fi
        sleep 2
    done
    python3 -c "import sqlite3; db=sqlite3.connect(\"/var/lib/replay-control/external_metadata.db\"); assert db.execute(\"SELECT COUNT(*) FROM provider_game WHERE provider = ?\", (\"launchbox\",)).fetchone()[0] == 1"
    test ! -e /srv/.replay-control-data/external_metadata.db
    test -d /etc/replay-control
    test -d /var/lib/replay-control/storages
    curl -kfsS https://127.0.0.1:8443/settings | grep -q "Storage location"
    test "$(curl -ksS -o /dev/null -w "%{http_code}" -X POST \
        -H "content-type: application/json" \
        -d "{\"path\":\"/srv/missing\"}" \
        https://127.0.0.1:8443/storage-setup/select)" = 400
    grep -qx /srv /var/lib/replay-control/standalone-storage-path
    test "$(curl -ksS -o /dev/null -w "%{http_code}" -X POST \
        -H "content-type: application/json" \
        -d "{\"path\":\"/srv/other\"}" \
        https://127.0.0.1:8443/storage-setup/select)" = 202
    for i in {1..30}; do
        if grep -qx /srv/other /var/lib/replay-control/standalone-storage-path && \
            grep -qx ok /var/lib/replay-control/storage-switch-status; then break; fi
        sleep 2
    done
    grep -qx /srv/other /var/lib/replay-control/standalone-storage-path
    curl -kfsS https://127.0.0.1:8443/api/version >/dev/null
    systemctl stop replay-control
    mv /srv/other/roms /srv/other/roms.offline
    systemctl start replay-control
    for i in {1..30}; do
        if curl -kfsS https://127.0.0.1:8443/storage-setup/status | grep -q "storage_error"; then break; fi
        sleep 2
    done
    curl -kfsS https://127.0.0.1:8443/storage-setup/status | grep -q "This folder must contain a readable roms/ directory"
    grep -qx /srv/other /var/lib/replay-control/standalone-storage-path
    mv /srv/other/roms.offline /srv/other/roms
    systemctl restart replay-control
    for i in {1..30}; do
        if curl -kfsS https://127.0.0.1:8443/api/version >/dev/null 2>&1; then break; fi
        sleep 2
    done
    curl -kfsS https://127.0.0.1:8443/api/version >/dev/null
    test -d /srv/roms && test -d /srv/other/bios
    touch /run/replay-control-fail-start-once
    test "$(curl -ksS -o /dev/null -w "%{http_code}" -X POST \
        -H "content-type: application/json" \
        -d "{\"path\":\"/srv/fail\"}" \
        https://127.0.0.1:8443/storage-setup/select)" = 202
    for i in {1..30}; do
        if grep -qx error /var/lib/replay-control/storage-switch-status 2>/dev/null; then break; fi
        sleep 2
    done
    grep -qx error /var/lib/replay-control/storage-switch-status
    grep -qx /srv/other /var/lib/replay-control/standalone-storage-path
    for i in {1..30}; do
        if curl -kfsS https://127.0.0.1:8443/api/version >/dev/null 2>&1; then break; fi
        sleep 2
    done
    curl -kfsS https://127.0.0.1:8443/api/version >/dev/null
    cp /var/lib/replay-control/standalone-storage-path \
        /var/lib/replay-control/storage-switch-previous
    printf "/srv/fail\n" > /var/lib/replay-control/standalone-storage-path
    printf "/srv/fail\n" > /var/lib/replay-control/storage-switch-pending
    systemctl restart replay-control
    for i in {1..30}; do
        if grep -qx /srv/other /var/lib/replay-control/standalone-storage-path && \
            test ! -e /var/lib/replay-control/storage-switch-pending; then break; fi
        sleep 2
    done
    grep -qx /srv/other /var/lib/replay-control/standalone-storage-path
    test ! -e /var/lib/replay-control/storage-switch-pending
    cat > /etc/default/replay-control <<EOF
REPLAY_PORT=8080
REPLAY_HTTPS_PORT=8443
REPLAY_STORAGE_PATH=/srv
REPLAY_CONTROL_SKIP_FIRST_RUN_SEED=1
REPLAY_GITHUB_API_URL=http://127.0.0.1:9000
EOF
    systemctl restart replay-control
    for i in {1..30}; do
        if [[ $(curl -ksS https://127.0.0.1:8443/storage-setup/active-root) == /srv/other ]]; then break; fi
        sleep 2
    done
    test "$(curl -kfsS https://127.0.0.1:8443/storage-setup/active-root)" = /srv/other
    rm -f /var/lib/replay-control/standalone-storage-path
    systemctl restart replay-control
    for i in {1..30}; do
        if [[ $(curl -ksS https://127.0.0.1:8443/storage-setup/active-root) == /srv ]]; then break; fi
        sleep 2
    done
    test "$(curl -kfsS https://127.0.0.1:8443/storage-setup/active-root)" = /srv
    for i in {1..30}; do
        if curl -kfsS https://127.0.0.1:8443/api/version; then break; fi
        sleep 2
    done
    curl -kfsS https://127.0.0.1:8443/api/version >/dev/null
'
if [[ ${RUN_BROWSER_E2E:-0} == 1 ]]; then
    bash "$repo_root/tests/package/run-browser-e2e.sh" "$container" initial
fi
"$engine" exec "$container" sh -c \
    "curl -kfsS https://127.0.0.1:8443/api/version | grep -q '\"version\":\"$expected_app\"'"
"$engine" exec "$container" bash /root/test-update-now.sh amd64
if [[ ${RUN_BROWSER_E2E:-0} == 1 ]]; then
    bash "$repo_root/tests/package/run-browser-e2e.sh" "$container" update "$expected_app"
fi
"$engine" exec "$container" bash -lc '
    set -e
    printf "/srv/other\n" > /var/lib/replay-control/standalone-storage-path
    mv /srv/other/roms /srv/other/roms-offline
    printf "failed:Package installed but storage is unavailable.\n" \
        > /var/lib/replay-control/update-result
    systemctl restart replay-control
    for i in {1..30}; do
        if curl -kfsS https://127.0.0.1:8443/storage-setup >/dev/null 2>&1; then break; fi
        sleep 2
    done
    curl -kfsS https://127.0.0.1:8443/api/update-result |
        grep -Fxq "failed:Package installed but storage is unavailable."
    expected_app=$(tr "~" "-" < /root/package-version)
    curl -kfsS https://127.0.0.1:8443/api/version |
        grep -Fq "\"version\":\"$expected_app\""
    test "$(curl -ksS -o /dev/null -w "%{redirect_url}" \
        https://127.0.0.1:8443/settings)" = \
        "https://127.0.0.1:8443/storage-setup"
    mv /srv/other/roms-offline /srv/other/roms
    systemctl restart replay-control
'
echo "Standalone $base_image amd64 install passed for $expected."
