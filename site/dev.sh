#!/bin/bash
# Serve the docs site locally with correct baseURL for LAN access.
set -euo pipefail

PORT="${1:-1313}"
IP=$(hostname -I | awk '{print $1}')
BASE="http://${IP}:${PORT}/replay-control/"

echo "Serving at: ${BASE}"
cd "$(dirname "$0")"
exec hugo server \
    --port "$PORT" \
    --bind 0.0.0.0 \
    --baseURL "$BASE" \
    --appendPort=false \
    --disableFastRender \
    --noHTTPCache
