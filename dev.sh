#!/bin/bash
set -euo pipefail

# Dev server with auto-reload on file changes.
# Builds WASM (debug) + SSR (debug) and restarts the server.
#
# Usage:
#   ./dev.sh [--storage-path /path/to/roms] [--port 8091]
#
# Defaults:
#   port: 8091
#   storage-path: auto-detect

CRATE="replay-control-app"
OUT_DIR="target/site"
PKG_DIR="$OUT_DIR/pkg"
PORT="${PORT:-8091}"
STORAGE_ARGS=""

# Parse arguments to pass through to the server.
SERVER_ARGS=""
while [[ $# -gt 0 ]]; do
    case "$1" in
        --port) PORT="$2"; shift 2 ;;
        *) SERVER_ARGS="$SERVER_ARGS $1"; shift ;;
    esac
done

build_wasm() {
    echo "==> Building WASM (debug)..."
    cargo build -p "$CRATE" --lib \
        --target wasm32-unknown-unknown \
        --features hydrate \
        --no-default-features

    echo "==> Running wasm-bindgen..."
    mkdir -p "$PKG_DIR"
    wasm-bindgen \
        "target/wasm32-unknown-unknown/debug/${CRATE//-/_}.wasm" \
        --out-dir "$PKG_DIR" \
        --out-name "${CRATE//-/_}" \
        --target web \
        --no-typescript
}

build_ssr() {
    echo "==> Building server (debug)..."
    cargo build -p "$CRATE" --bin "$CRATE" \
        --features ssr \
        --no-default-features
}

# Copy CSS.
copy_assets() {
    cp "replay-control-app/style/style.css" "$OUT_DIR/style.css"
}

echo "==> Initial build..."
build_wasm
build_ssr
copy_assets

echo ""
echo "==> Starting cargo-watch on port $PORT"
echo "    Watching: replay-control-app/src, replay-control-core/src, replay-control-app/style"
echo "    Press Ctrl+C to stop."
echo ""

exec cargo watch \
    -w replay-control-app/src \
    -w replay-control-core/src \
    -w replay-control-app/style \
    -s "$(cat <<INNER
set -e
cargo build -p $CRATE --lib --target wasm32-unknown-unknown --features hydrate --no-default-features
wasm-bindgen target/wasm32-unknown-unknown/debug/${CRATE//-/_}.wasm --out-dir $PKG_DIR --out-name ${CRATE//-/_} --target web --no-typescript
cp replay-control-app/style/style.css $OUT_DIR/style.css
cargo run -p $CRATE --features ssr -- --port $PORT $SERVER_ARGS
INNER
)"
