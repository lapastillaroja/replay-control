#!/bin/bash
# Serve the docs site locally.
#
# Usage:
#   ./dev.sh              # LAN IP (accessible from other devices)
#   ./dev.sh --localhost   # localhost only (SRI/CORS safe)
#   ./dev.sh --port 8000  # custom port
set -euo pipefail

PORT="1313"
MODE="lan"

while [[ $# -gt 0 ]]; do
    case "$1" in
        --lan)       MODE="lan"; shift ;;
        --localhost) MODE="local"; shift ;;
        --port)      PORT="$2"; shift 2 ;;
        *)           PORT="$1"; shift ;;
    esac
done

if [[ "$MODE" == "lan" ]]; then
    IP=$(hostname -I | awk '{print $1}')
    BASE="http://${IP}:${PORT}/replay-control/"
    BIND="0.0.0.0"
else
    BASE="http://localhost:${PORT}/replay-control/"
    BIND="0.0.0.0"
fi

echo "Serving at: ${BASE}"
cd "$(dirname "$0")"
exec hugo server \
    --port "$PORT" \
    --bind "$BIND" \
    --baseURL "$BASE" \
    --appendPort=false \
    --disableFastRender \
    --noHTTPCache
