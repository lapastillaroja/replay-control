#!/bin/bash
# Serve the docs site locally.
#
# Usage:
#   ./dev.sh              # localhost (SRI/CORS safe)
#   ./dev.sh --lan        # LAN IP (accessible from other devices, SRI may break)
#   ./dev.sh --port 8000  # custom port
set -euo pipefail

PORT="1313"
MODE="local"

while [[ $# -gt 0 ]]; do
    case "$1" in
        --lan)  MODE="lan"; shift ;;
        --port) PORT="$2"; shift 2 ;;
        *)      PORT="$1"; shift ;;
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
