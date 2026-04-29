#!/bin/bash
# Serve the docs site locally.
#
# Usage:
#   ./dev.sh                          # default: baseURL uses host LAN IP (192.168.10.4)
#   ./dev.sh --host 192.168.1.50      # override the host/IP in baseURL
#   ./dev.sh --localhost              # localhost only (SRI/CORS safe)
#   ./dev.sh --lan                    # auto-detect via `hostname -I` (broken inside podman)
#   ./dev.sh --port 8000              # custom port
set -euo pipefail

PORT="1313"
MODE="host"
HOST="192.168.10.4"

while [[ $# -gt 0 ]]; do
    case "$1" in
        --lan)       MODE="lan"; shift ;;
        --localhost) MODE="local"; shift ;;
        --host)      HOST="$2"; MODE="host"; shift 2 ;;
        --port)      PORT="$2"; shift 2 ;;
        *)           PORT="$1"; shift ;;
    esac
done

if [[ "$MODE" == "host" ]]; then
    BASE="http://${HOST}:${PORT}/replay-control/"
    BIND="0.0.0.0"
elif [[ "$MODE" == "lan" ]]; then
    IP=$(hostname -I | awk '{print $1}')
    BASE="http://${IP}:${PORT}/replay-control/"
    BIND="0.0.0.0"
else
    BASE="http://localhost:${PORT}/replay-control/"
    BIND="0.0.0.0"
fi

echo "Serving at: ${BASE}"
cd "$(dirname "$0")"

# Build the Pagefind search index so /pagefind/* assets are served in dev.
# Hugo's `hugo server` doesn't run pagefind; without this, search 404s.
echo "Building Pagefind index..."
hugo --quiet --gc --destination public-pagefind
npx -y pagefind --site public-pagefind --output-path static/pagefind --quiet
rm -rf public-pagefind

exec hugo server \
    --port "$PORT" \
    --bind "$BIND" \
    --baseURL "$BASE" \
    --appendPort=false \
    --disableFastRender \
    --disableLiveReload \
    --noHTTPCache
