#!/usr/bin/env bash
set -euo pipefail

container=${1:?usage: run-browser-e2e.sh CONTAINER initial|update [VERSION]}
phase=${2:?browser test phase required}
engine=${CONTAINER_ENGINE:-podman}
repo_root=$(cd "$(dirname "$0")/../.." && pwd)

if [[ "$engine" == podman ]]; then
    container_ip=$("$engine" inspect "$container" \
        --format '{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}')
    browser_url="https://$container_ip:8443"
else
    host_port=$("$engine" port "$container" 8443/tcp | tail -1)
    browser_url="https://127.0.0.1:${host_port##*:}"
fi

cd "$repo_root"
case "$phase" in
    initial)
        APP_URL="$browser_url" REPLAY_PACKAGE_BROWSER=1 \
            python3 -m pytest tests/e2e/test_package_setup.py -q
        ;;
    update)
        version=${3:?update phase requires the application version}
        APP_URL="$browser_url" REPLAY_PACKAGE_VERSION="$version" \
            python3 -m pytest tests/e2e/test_package_update_result.py -q
        ;;
    *) echo "Unknown browser test phase: $phase" >&2; exit 1 ;;
esac
