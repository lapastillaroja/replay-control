#!/usr/bin/env bash
set -euo pipefail

# End-to-end arm64 package install and v1.0 archive migration on Debian 13
# with the official RePlayOS frontend package. The Pi test covers real systemd.
repo_root=$(cd "$(dirname "$0")/../.." && pwd)
package=${1:?usage: scenario-replayos-arm64.sh PACKAGE}
package=$(realpath "$package")
package_version=$(dpkg-deb --field "$package" Version)
dpkg --compare-versions '1.2.9' lt '1.3.0~beta'
dpkg --compare-versions '1.3.0~beta-1' ge '1.3.0~beta'
dpkg --compare-versions '1.3.0~beta.1' ge '1.3.0~beta'
engine=${CONTAINER_ENGINE:-podman}
image=replay-control-package-test:trixie-arm64
replayos_image=replay-control-package-test:replayos-arm64
prefix="replay-control-package-$$"
temp=$(mktemp -d)
cleanup() {
    local status=$?
    if [[ ${KEEP_FAILED_PACKAGE_CONTAINER:-0} == 1 && $status -ne 0 ]]; then
        echo "Keeping failed package containers with prefix $prefix" >&2
        return
    fi
    "$engine" rm -f "$prefix-clean" "$prefix-replayos-clean" \
        "$prefix-migrate" "$prefix-upgrade" \
        "$prefix-replayos-base" >/dev/null 2>&1 || true
    rm -rf "$temp"
}
trap cleanup EXIT

"$engine" build --platform linux/arm64 -t "$image" \
    -f "$repo_root/tests/package/Dockerfile" "$repo_root/tests/package"

base=https://github.com/lapastillaroja/replay-control/releases/download/v1.0.0
mkdir -p "$temp/v1.0.0"
for asset in checksums.sha256 install.sh replay-control-app-aarch64-linux.tar.gz \
    replay-catalog.tar.gz replay-site.tar.gz; do
    curl -fsSL --retry 3 -o "$temp/v1.0.0/$asset" "$base/$asset"
done
(cd "$temp/v1.0.0" && sha256sum -c checksums.sha256)

# RePlayOS publishes its frontend as a .deb in its signed OTA release. Install
# that package and its private library dependency before either migration test.
replayos_base=https://github.com/rtomasa/replayos-ota-upd/releases/download/ota
mkdir -p "$temp/replayos"
curl -fsSL --retry 3 -o "$temp/replayos/Packages" "$replayos_base/Packages"
for name in replay-libswitchres replay-frontend; do
    read -r filename checksum < <(awk -v wanted="$name" '
        $1 == "Package:" { selected = ($2 == wanted); filename = ""; checksum = "" }
        selected && $1 == "Filename:" { filename = $2 }
        selected && $1 == "SHA256:" { checksum = $2 }
        selected && /^$/ { print filename, checksum; exit }
    ' "$temp/replayos/Packages")
    filename=${filename#./}
    [[ "$filename" =~ ^${name}_[0-9A-Za-z.+~-]+_arm64\.deb$ &&
       "$checksum" =~ ^[0-9a-fA-F]{64}$ ]] || {
        echo "Invalid RePlayOS package index entry for $name" >&2
        exit 1
    }
    curl -fsSL --retry 3 -o "$temp/replayos/$filename" "$replayos_base/$filename"
    (cd "$temp/replayos" && printf '%s  %s\n' "$checksum" "$filename" | sha256sum -c -)
done
"$engine" run -d --name "$prefix-replayos-base" --platform linux/arm64 "$image" sleep infinity >/dev/null
for deb in "$temp/replayos/"*.deb; do
    "$engine" cp "$deb" "$prefix-replayos-base:/root/$(basename "$deb")"
done
"$engine" exec "$prefix-replayos-base" bash -lc \
    'apt-get update -qq && DEBIAN_FRONTEND=noninteractive apt-get install -y -qq /root/replay-*.deb' \
    > "$temp/replayos-install.log" 2>&1 || {
        tail -n 50 "$temp/replayos-install.log" >&2
        exit 1
    }
"$engine" exec "$prefix-replayos-base" dpkg-query -W replay-frontend replay-libswitchres
"$engine" commit "$prefix-replayos-base" "$replayos_image" >/dev/null
"$engine" rm -f "$prefix-replayos-base" >/dev/null

start_container() {
    local name=$1
    local base_image=${2:-$image}
    local mounts=()
    if [[ "$base_image" == "$replayos_image" ]]; then
        mounts=(--tmpfs /media/sd)
    fi
    "$engine" run -d --name "$name" --platform linux/arm64 "${mounts[@]}" "$base_image" sleep infinity >/dev/null
    "$engine" cp "$repo_root/tests/package/systemctl-shim" "$name:/usr/local/bin/systemctl"
    "$engine" cp "$repo_root/tests/package/systemctl-shim" "$name:/usr/bin/systemctl"
    "$engine" cp "$repo_root/tests/package/readlink-shim" "$name:/usr/local/bin/readlink"
    "$engine" cp "$repo_root/tests/package/systemd-run-shim" "$name:/usr/local/bin/systemd-run"
    "$engine" cp "$repo_root/tests/package/systemd-run-shim" "$name:/usr/bin/systemd-run"
    "$engine" cp "$repo_root/tests/package/test-update-now.sh" "$name:/root/test-update-now.sh"
    "$engine" cp "$repo_root/tests/package/test-desktop-launcher.sh" "$name:/root/test-desktop-launcher.sh"
    "$engine" exec "$name" chmod +x /usr/local/bin/systemctl
    "$engine" exec "$name" chmod +x /usr/bin/systemctl
    "$engine" exec "$name" chmod +x /usr/local/bin/readlink
    "$engine" exec "$name" chmod +x /usr/local/bin/systemd-run
    "$engine" exec "$name" chmod +x /usr/bin/systemd-run
    "$engine" cp "$package" "$name:/root/$(basename "$package")"
    "$engine" exec "$name" sh -c "printf '%s\\n' '$package_version' > /root/package-version"
}

start_container "$prefix-clean"
"$engine" cp "$repo_root/install.sh" "$prefix-clean:/root/install-new.sh"
"$engine" exec "$prefix-clean" bash -lc '
    set -e
    mkdir -p /media/sd/roms
    apt-get update -qq
    bash /root/install-new.sh --package /root/replay-control_*_arm64.deb
    for i in {1..30}; do
        if curl -kfsS https://127.0.0.1:8443/storage-setup >/dev/null; then break; fi
        sleep 2
    done
    curl -kfsS https://127.0.0.1:8443/storage-setup | grep -q "Choose your storage location"
    curl -kfsS -H "Accept-Language: es" https://127.0.0.1:8443/storage-setup/messages |
        grep -Fq "Elige la ubicación de almacenamiento"
    curl -kfsS -H "Accept-Language: ja" https://127.0.0.1:8443/storage-setup/messages |
        grep -Fq "ストレージの場所を選択"
    cat > /etc/default/replay-control <<EOF
REPLAY_PORT=8080
REPLAY_HTTPS_PORT=8443
REPLAY_STORAGE_PATH=/media/sd
REPLAY_CONTROL_SKIP_FIRST_RUN_SEED=1
REPLAY_GITHUB_API_URL=http://127.0.0.1:9000
EOF
    systemctl start replay-control
    for i in {1..30}; do
        if curl -kfsS https://127.0.0.1:8443/api/version; then break; fi
        sleep 2
    done
    curl -kfsS https://127.0.0.1:8443/api/version >/dev/null
    sed -i "s/^REPLAY_HTTPS_PORT=.*/REPLAY_HTTPS_PORT=9443/" /etc/default/replay-control
    systemctl restart replay-control
    for i in {1..30}; do
        if curl -kfsS https://127.0.0.1:9443/api/version; then exit 0; fi
        sleep 2
    done
    exit 1
'
"$engine" exec "$prefix-clean" bash -lc '
    test -s /usr/share/replay-control/catalog.sqlite
    test -d /usr/share/replay-control/site/pkg
    dpkg-query -S /usr/bin/replay-control-app \
        /usr/share/replay-control/catalog.sqlite \
        /usr/lib/replay-control/package-update-deb \
        /lib/systemd/system/replay-control.service >/dev/null
    dpkg -V replay-control
    dpkg-query -W -f="\${Status}" replay-control | grep -q "install ok installed"
    ! dpkg-query -W -f="\${Depends}" replay-control | grep -qi replay-
'
"$engine" exec "$prefix-clean" bash /root/test-update-now.sh arm64
start_container "$prefix-replayos-clean" "$replayos_image"
"$engine" cp "$repo_root/install.sh" "$prefix-replayos-clean:/root/install-new.sh"
"$engine" exec "$prefix-replayos-clean" bash -lc '
    set -e
    mkdir -p /media/sd/config /media/sd/roms
    printf "system_storage = \"sd\"\n" > /media/sd/config/replay.cfg
    cat > /etc/default/replay-control <<EOF
REPLAY_PORT=8080
REPLAY_HTTPS_PORT=8443
REPLAY_STORAGE_PATH=/media/sd
REPLAY_CONTROL_SKIP_FIRST_RUN_SEED=1
EOF
    apt-get update -qq
    DEBIAN_FRONTEND=noninteractive apt-get install -y /root/replay-control_*_arm64.deb
    test -s /usr/share/icons/hicolor/192x192/apps/replay-control.png
    grep -Fxq "Exec=/usr/lib/replay-control/open-browser" /usr/share/applications/replay-control.desktop
    dpkg-query -S /usr/share/applications/replay-control.desktop \
        /usr/lib/replay-control/open-browser | grep -q replay-control:
    bash /root/test-desktop-launcher.sh
    dpkg-query -W replay-frontend replay-control
    for i in {1..30}; do
        if curl -kfsS https://127.0.0.1:8443/api/version; then break; fi
        sleep 2
    done
    curl -kfsS https://127.0.0.1:8443/api/version >/dev/null
    printf "user data survives upgrades\n" > /etc/replay-control/package-test-marker
    original=$(dpkg-deb --field /root/replay-control_*_arm64.deb Version)
    replay_version=$(dpkg-query -W -f="\${Version}" replay-frontend)
    for revision in 1 2; do
        stage=$(mktemp -d)
        dpkg-deb --raw-extract /root/replay-control_*_arm64.deb "$stage"
        sed -i "s/^Version:.*/Version: ${original}+test${revision}/" "$stage/DEBIAN/control"
        dpkg-deb --build --root-owner-group "$stage" "/root/replay-control-upgrade${revision}.deb"
        rm -rf "$stage"
        DEBIAN_FRONTEND=noninteractive apt-get install -y "/root/replay-control-upgrade${revision}.deb"
        test "$(dpkg-query -W -f="\${Version}" replay-control)" = "${original}+test${revision}"
        test "$(dpkg-query -W -f="\${Version}" replay-frontend)" = "$replay_version"
        test "$(cat /etc/replay-control/package-test-marker)" = "user data survives upgrades"
        for i in {1..30}; do
            if curl -kfsS https://127.0.0.1:8443/api/version >/dev/null; then break; fi
            sleep 2
        done
        curl -kfsS https://127.0.0.1:8443/api/version >/dev/null
    done
    mkdir -p /etc/systemd/system/replay-control.service.d
    : > /etc/systemd/system/replay-control.service.d/90-replay-control-dev.conf
    bash /root/install-new.sh --uninstall
    ! dpkg-query -W -f="\${Status}" replay-control 2>/dev/null |
        grep -q "install ok installed"
    test -f /etc/default/replay-control
    test -f /etc/replay-control/package-test-marker
    test ! -e /etc/systemd/system/replay-control.service.d/90-replay-control-dev.conf
    bash /root/install-new.sh --purge --yes
    test ! -e /etc/default/replay-control
    test ! -e /etc/replay-control
    test ! -e /var/lib/replay-control
'

# Exercise a real application-version transition through the same APT helper
# that Update Now launches. Use the published v1.0 binary in a package fixture
# and preserve settings across the 1.0 -> current upgrade.
start_container "$prefix-upgrade" "$replayos_image"
"$engine" cp "$temp/v1.0.0/replay-control-app-aarch64-linux.tar.gz" \
    "$prefix-upgrade:/root/v1-binary.tar.gz"
"$engine" cp "$temp/v1.0.0/replay-site.tar.gz" "$prefix-upgrade:/root/v1-site.tar.gz"
"$engine" cp "$temp/v1.0.0/replay-catalog.tar.gz" "$prefix-upgrade:/root/v1-catalog.tar.gz"
"$engine" exec "$prefix-upgrade" bash -lc '
    set -e
    mkdir -p /media/sd/config /media/sd/roms
    printf "system_storage = \"sd\"\n" > /media/sd/config/replay.cfg
    cat > /etc/default/replay-control <<EOF
REPLAY_PORT=8080
REPLAY_HTTPS_PORT=8443
REPLAY_STORAGE_PATH=/media/sd
REPLAY_CONTROL_SKIP_FIRST_RUN_SEED=1
EOF
    stage=$(mktemp -d)
    dpkg-deb --raw-extract /root/replay-control_*_arm64.deb "$stage"
    tar -xzf /root/v1-binary.tar.gz -C "$stage/usr/bin"
    rm -rf "$stage/usr/share/replay-control/site"
    tar -xzf /root/v1-site.tar.gz -C "$stage/usr/share/replay-control"
    tar -xzf /root/v1-catalog.tar.gz -C "$stage/usr/share/replay-control"
    sed -i "s/^Version:.*/Version: 1.0.0/" "$stage/DEBIAN/control"
    dpkg-deb --build --root-owner-group "$stage" /root/replay-control-v1.deb
    rm -rf "$stage"
    DEBIAN_FRONTEND=noninteractive apt-get install -y /root/replay-control-v1.deb
    for i in {1..30}; do
        if curl -kfsS https://127.0.0.1:8443/api/version | grep -q "\"version\":\"1.0.0\""; then break; fi
        sleep 2
    done
    curl -kfsS https://127.0.0.1:8443/api/version | grep -q "\"version\":\"1.0.0\""
    printf "settings survive real upgrade\n" > /etc/replay-control/package-test-marker
    cp /root/replay-control_*_arm64.deb /tmp/current.deb
    expected_app=$(tr "~" "-" < /root/package-version)
    /usr/lib/replay-control/package-update-deb /tmp/current.deb "$expected_app"
    grep -q "^ok:$expected_app$" /var/lib/replay-control/update-result
    test "$(cat /etc/replay-control/package-test-marker)" = "settings survive real upgrade"
    curl -kfsS https://127.0.0.1:8443/api/version | grep -q "\"version\":\"$expected_app\""
'

start_container "$prefix-migrate" "$replayos_image"
"$engine" cp "$temp/v1.0.0" "$prefix-migrate:/root/v1.0.0"
"$engine" cp "$repo_root/tests/package/prepare-v1.sh" "$prefix-migrate:/root/prepare-v1.sh"
"$engine" cp "$repo_root/install.sh" "$prefix-migrate:/root/install-new.sh"
"$engine" exec "$prefix-migrate" bash /root/prepare-v1.sh
"$engine" exec "$prefix-migrate" bash -lc '
    set -e
    bash /root/install-new.sh --local /root/v1.0.0 --dry-run |
        grep -Fq "Would extract binary to /usr/local/bin/replay-control-app"
    bash /root/install-new.sh --package /root/replay-control_*_arm64.deb --dry-run |
        grep -Fq "Would install Debian package"
    if DEBIAN_FRONTEND=noninteractive apt-get install -y /root/replay-control_*_arm64.deb; then
        echo "Direct APT install bypassed archive migration" >&2
        exit 1
    fi
    test -e /etc/systemd/system/replay-control.service
    touch /run/replay-control-fail-enable-once
    if bash /root/install-new.sh --package /root/replay-control_*_arm64.deb; then
        echo "Migration unexpectedly succeeded after service activation failed" >&2
        exit 1
    fi
    test -e /etc/systemd/system/replay-control.service
    test -e /usr/local/bin/replay-control-app
    systemctl is-enabled replay-control.service
    ! dpkg-query -W -f="\${Status}" replay-control 2>/dev/null | grep -q "install ok installed"
    restored=false
    for _ in {1..30}; do
        if curl -fsS http://127.0.0.1:8080/api/version 2>/dev/null |
           grep -q "\"version\":\"1.0.0\""; then
            restored=true
            break
        fi
        sleep 2
    done
    test "$restored" = true
    printf "copied on retry\n" > /media/sd/.replay-control-data/migration-retry-marker
    sed -i "s/^font_size = \"large\"$/font_size = \"huge\"/" /media/sd/.replay-control/settings.cfg
    printf "%s\n" "$$" > /run/replay-control-package-migration
    DEBIAN_FRONTEND=noninteractive apt-get install -y /root/replay-control_*_arm64.deb
    rm -f /run/replay-control-package-migration
    test -e /etc/systemd/system/replay-control.service
    dpkg-query -W -f="\${Status}" replay-control | grep -q "install ok installed"
'
"$engine" exec "$prefix-migrate" bash /root/install-new.sh --package "/root/$(basename "$package")"
"$engine" exec "$prefix-migrate" bash -lc '
    set -e
    dpkg-query -W -f="\${Status}" replay-control | grep -q "install ok installed"
    test "$(cat /etc/replay-control/package-test-marker)" = "settings preserved"
    test "$(cat /var/lib/replay-control/package-test-marker)" = "library preserved"
    grep -qx "font_size = \"huge\"" /media/sd/.replay-control/settings.cfg
    grep -qx "font_size = \"huge\"" /etc/replay-control/settings.cfg
    grep -qx "copied on retry" /var/lib/replay-control/migration-retry-marker
    test -s /media/sd/.replay-control/user_data.db
    find /var/lib/replay-control/storages -name library.db -size +0c | grep -q .
    for _ in {1..30}; do
        service_pid=$(cat /run/replay-control-test.pid)
        if ls -l "/proc/$service_pid/fd" | grep -q "/var/lib/replay-control/storages/.*/library.db"; then
            break
        fi
        sleep 1
    done
    ls -l "/proc/$service_pid/fd" | grep -q "/var/lib/replay-control/storages/.*/library.db"
    test ! -e /usr/local/bin/replay-control-app
    test ! -e /etc/systemd/system/replay-control.service
    curl -kfsS https://127.0.0.1:8443/api/version
    if bash /root/install-new.sh --local /root/v1.0.0; then
        echo "Archive installer overwrote a package installation" >&2
        exit 1
    fi
'
"$engine" exec "$prefix-migrate" bash -lc '
    set -e
    cat > /usr/local/bin/apt-get <<EOF
#!/bin/sh
echo "Could not get lock /var/lib/dpkg/lock-frontend" >&2
exit 100
EOF
    chmod +x /usr/local/bin/apt-get
    cp /root/replay-control_*_arm64.deb /tmp/retry.deb
    if /usr/lib/replay-control/package-update-deb /tmp/retry.deb "$(cat /root/package-version)"; then exit 1; fi
    grep -q "failed:APT is busy" /var/lib/replay-control/update-result
    cat > /usr/local/bin/apt-get <<EOF
#!/bin/sh
echo "No space left on device" >&2
exit 100
EOF
    cp /root/replay-control_*_arm64.deb /tmp/no-space.deb
    if /usr/lib/replay-control/package-update-deb /tmp/no-space.deb "$(cat /root/package-version)"; then exit 1; fi
    grep -q "failed:Insufficient space" /var/lib/replay-control/update-result
    cat > /usr/local/bin/apt-get <<EOF
#!/bin/sh
sleep 3
EOF
    cp /root/replay-control_*_arm64.deb /tmp/interrupted.deb
    timeout -s TERM 1 /usr/lib/replay-control/package-update-deb /tmp/interrupted.deb "$(cat /root/package-version)" || true
    grep -q "failed:Package update was interrupted" /var/lib/replay-control/update-result
'
echo 'Standalone and RePlayOS clean installs, real upgrade, and v1.0 migration passed on Debian 13 arm64.'
