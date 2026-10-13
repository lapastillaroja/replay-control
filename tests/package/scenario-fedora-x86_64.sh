#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "$0")/../.." && pwd)
package=${1:?usage: scenario-fedora-x86_64.sh PACKAGE}
package=$(realpath "$package")
engine=${CONTAINER_ENGINE:-podman}
image=replay-control-package-test:fedora42
name="replay-control-fedora-$$"
trap '"$engine" rm -f "$name" >/dev/null 2>&1 || true' EXIT
network_args=()
if [[ ${RUN_BROWSER_E2E:-0} == 1 ]]; then
    if [[ "$engine" == podman ]]; then
        network_args=(--network podman)
    else
        network_args=(-p 127.0.0.1::8443)
    fi
fi

"$engine" build --platform linux/amd64 -t "$image" \
    -f "$repo_root/tests/package/Fedora.Dockerfile" "$repo_root/tests/package"
"$engine" run -d --name "$name" --platform linux/amd64 \
    "${network_args[@]}" "$image" >/dev/null
"$engine" cp "$repo_root/tests/package/systemctl-shim" "$name:/usr/local/bin/systemctl"
"$engine" cp "$repo_root/tests/package/systemctl-shim" "$name:/usr/bin/systemctl"
"$engine" cp "$repo_root/tests/package/systemd-run-shim" "$name:/usr/local/bin/systemd-run"
"$engine" cp "$repo_root/tests/package/systemd-run-shim" "$name:/usr/bin/systemd-run"
"$engine" cp "$repo_root/tests/package/test-update-now.sh" "$name:/root/test-update-now.sh"
"$engine" cp "$repo_root/tests/package/test-desktop-launcher.sh" "$name:/root/test-desktop-launcher.sh"
"$engine" cp "$repo_root/tests/package/extract-server-function-route.py" "$name:/root/extract-server-function-route.py"
"$engine" cp "$repo_root/packaging/replay-control-rpm.spec" "$name:/root/replay-control-rpm.spec"
"$engine" exec "$name" chmod +x /usr/local/bin/systemctl /usr/bin/systemctl
"$engine" exec "$name" chmod +x /usr/local/bin/systemd-run
"$engine" exec "$name" chmod +x /usr/bin/systemd-run
"$engine" cp "$package" "$name:/root/$(basename "$package")"
package_version=${package##*/replay-control-}
package_version=${package_version%-1.x86_64.rpm}
package_version=${package_version/-/\~}
"$engine" exec "$name" sh -c "printf '%s\\n' '$package_version' > /root/package-version"
"$engine" exec "$name" bash -lc '
    set -e
    mkdir -p /media/sd/roms
    dnf install -y /root/replay-control-*.rpm
    test -s /usr/share/icons/hicolor/192x192/apps/replay-control.png
    grep -Fxq "Exec=/usr/lib/replay-control/open-browser" /usr/share/applications/replay-control.desktop
    rpm -qf /usr/share/applications/replay-control.desktop \
        /usr/share/icons/hicolor/192x192/apps/replay-control.png \
        /usr/lib/replay-control/package-update-rpm \
        /usr/lib/replay-control/open-browser | grep -q replay-control
    bash /root/test-desktop-launcher.sh
    for i in {1..30}; do
        if curl -kfsS https://127.0.0.1:8443/storage-setup >/dev/null; then break; fi
        sleep 2
    done
    curl -kfsS https://127.0.0.1:8443/storage-setup | grep -q "Choose your storage location"
    curl -kfsS https://127.0.0.1:8443/storage-setup | grep -q "STEP 1 OF 2"
    curl -kfsS https://127.0.0.1:8443/storage-setup/app.js | grep -Fq "/?setup"
    test "$(curl -ksS -o /dev/null -w "%{http_code}" -X POST \
        -H "content-type: application/json" -d "{\"path\":\"/media/sd\"}" \
        https://127.0.0.1:8443/storage-setup/select)" = 202
    for i in {1..30}; do
        if curl -kfsS https://127.0.0.1:8443/api/version && \
            grep -qx ok /var/lib/replay-control/storage-switch-status 2>/dev/null; then
            rpm -q replay-control
            test -s /usr/share/replay-control/catalog.sqlite
            test -d /usr/share/replay-control/site/pkg
            break
        fi
        sleep 2
    done
    curl -kfsS https://127.0.0.1:8443/api/version >/dev/null
    curl -kfsS "https://127.0.0.1:8443/?setup" | grep -q "Choose your game library folder"
    for i in {1..30}; do
        if grep -q "Downloading LaunchBox metadata from" /var/log/replay-control-test.log; then break; fi
        sleep 2
    done
    grep -q "Downloading LaunchBox metadata from" /var/log/replay-control-test.log
    test -d /etc/replay-control
    test -d /var/lib/replay-control/storages
    printf "REPLAY_STORAGE_PATH=/media/sd\nREPLAY_CONTROL_SKIP_FIRST_RUN_SEED=1\nREPLAY_GITHUB_API_URL=http://127.0.0.1:9000\n" > /etc/default/replay-control
    wasm_hash=$(sed -n "s/^wasm: //p" /usr/share/replay-control/site/hash.txt)
    favorite_route=$(python3 /root/extract-server-function-route.py \
        "/usr/share/replay-control/site/pkg/replay_control_app.$wasm_hash.wasm" \
        /sfn/get_system_favorites)
    status=$(curl -ksS -o /tmp/favorite-route-response -w "%{http_code}" -X POST \
        -H "content-type: application/x-www-form-urlencoded" --data "system=nintendo_nes" \
        "https://127.0.0.1:8443$favorite_route")
    if [[ "$status" != 200 ]]; then
        echo "Favorite route returned $status:" >&2
        cat /tmp/favorite-route-response >&2
        exit 1
    fi
'
if [[ ${RUN_BROWSER_E2E:-0} == 1 ]]; then
    bash "$repo_root/tests/package/run-browser-e2e.sh" "$name" initial
fi
"$engine" exec "$name" bash -lc '
    set -e
    printf "REPLAY_HTTPS_PORT=9443\n" >> /etc/default/replay-control
    systemctl restart replay-control
    for i in {1..30}; do
        if curl -kfsS https://127.0.0.1:9443/api/version; then exit 0; fi
        sleep 2
    done
    tail -n 30 /var/log/replay-control-test.log
    exit 1
'
"$engine" exec "$name" bash -lc '
    set -e
    mkdir -p /root/old-payload
    (cd /root/old-payload && rpm2cpio /root/replay-control-*-1.x86_64.rpm | cpio -idm --quiet)
    rpmbuild -bb /root/replay-control-rpm.spec \
        --define "_topdir /root/rpmbuild" \
        --define "payload /root/old-payload" \
        --define "version 0.0.1" \
        --define "_build_id_links none" >/dev/null
    dnf downgrade -y /root/rpmbuild/RPMS/x86_64/replay-control-0.0.1-1.*.x86_64.rpm >/dev/null
    test "$(rpm -q --qf "%{VERSION}" replay-control)" = 0.0.1
'
"$engine" exec "$name" bash /root/test-update-now.sh rpm
echo 'Fedora 42 x86_64 DNF installation and Update Now passed.'
