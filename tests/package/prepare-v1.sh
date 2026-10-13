#!/usr/bin/env bash
set -euo pipefail

# Run inside the Debian 13 arm64 systemd test image after copying the verified
# v1.0.0 release assets to /root/v1.0.0.
fixture=/root/v1.0.0
mkdir -p "$fixture/target/aarch64-unknown-linux-gnu/release" "$fixture/target/site" \
    /media/sd/config /media/sd/roms /etc/replay-control /var/lib/replay-control
sh /usr/local/bin/init-replay-control-test-settings /media/sd/.replay-control
tar -xzf "$fixture/replay-control-app-aarch64-linux.tar.gz" \
    -C "$fixture/target/aarch64-unknown-linux-gnu/release"
tar -xzf "$fixture/replay-site.tar.gz" -C "$fixture/target"
tar -xzf "$fixture/replay-catalog.tar.gz" -C "$fixture"
printf 'system_storage = "sd"\n' > /media/sd/config/replay.cfg
cat > /etc/default/replay-control <<'EOF'
REPLAY_PORT=8080
REPLAY_HTTPS_PORT=8443
REPLAY_SITE_ROOT=/usr/local/share/replay/site
REPLAY_STORAGE_PATH=/media/sd
REPLAY_EXTRA_ARGS="--storage-path /media/sd --dangerous-disable-https"
REPLAY_CONTROL_SKIP_FIRST_RUN_SEED=1
EOF
printf 'settings preserved\n' > /etc/replay-control/package-test-marker
printf 'library preserved\n' > /var/lib/replay-control/package-test-marker
bash "$fixture/install.sh" --local "$fixture"
for _ in {1..30}; do
    if curl -fsS http://127.0.0.1:8080/api/version | grep -q '"version":"1.0.0"'; then
        # An explicit storage path in the archive service moves both host
        # settings and the storage-keyed library DB under that root.
        printf 'font_size = "large"\n' >> /media/sd/.replay-control/settings.cfg
        find /media/sd/.replay-control-data/storages -name library.db -size +0c \
            | grep -q .
        exit 0
    fi
    sleep 2
done
echo 'Version 1.0.0 service did not become healthy' >&2
systemctl status replay-control.service --no-pager || true
exit 1
