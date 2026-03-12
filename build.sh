#!/bin/bash
set -euo pipefail

CRATE="replay-control-app"
OUT_DIR="target/site"
PKG_DIR="$OUT_DIR/pkg"
TARGET=""

# Parse arguments
for arg in "$@"; do
    case "$arg" in
        --target)  shift_next=true ;;
        aarch64)   TARGET="aarch64-unknown-linux-gnu" ;;
        *)
            if [[ "${shift_next:-}" == "true" ]]; then
                TARGET="$arg"
                shift_next=false
            fi
            ;;
    esac
done

# Allow TARGET env var as well
TARGET="${TARGET:-${BUILD_TARGET:-}}"

echo "==> Building WASM (hydrate)..."
cargo build -p "$CRATE" --lib \
  --target wasm32-unknown-unknown \
  --profile wasm-release \
  --features hydrate \
  --no-default-features

echo "==> Running wasm-bindgen..."
mkdir -p "$PKG_DIR"
wasm-bindgen \
  "target/wasm32-unknown-unknown/wasm-release/${CRATE//-/_}.wasm" \
  --out-dir "$PKG_DIR" \
  --out-name "${CRATE//-/_}" \
  --target web \
  --no-typescript

# Optimize WASM with wasm-opt if available.
WASM_FILE="$PKG_DIR/${CRATE//-/_}_bg.wasm"
if command -v wasm-opt &>/dev/null; then
    echo "==> Running wasm-opt -Oz..."
    BEFORE=$(stat -c%s "$WASM_FILE" 2>/dev/null || echo 0)
    wasm-opt -Oz \
        --enable-bulk-memory \
        --enable-nontrapping-float-to-int \
        --enable-sign-ext \
        --enable-mutable-globals \
        "$WASM_FILE" -o "$WASM_FILE"
    AFTER=$(stat -c%s "$WASM_FILE" 2>/dev/null || echo 0)
    echo "    WASM: ${BEFORE} -> ${AFTER} bytes ($(( (BEFORE - AFTER) * 100 / BEFORE ))% reduction)"
else
    echo "    (wasm-opt not found, skipping)"
fi

# Pre-compress WASM for static serving.
echo "==> Pre-compressing WASM..."
gzip -9 -k -f "$WASM_FILE"
GZ_SIZE=$(stat -c%s "${WASM_FILE}.gz" 2>/dev/null || echo 0)
echo "    ${WASM_FILE}.gz: ${GZ_SIZE} bytes"

# Copy static assets
cat replay-control-app/style/_*.css > "$OUT_DIR/style.css"
cp -r "replay-control-app/static/icons" "$OUT_DIR/icons" 2>/dev/null || true

echo "==> Building server (ssr)..."
if [[ -n "$TARGET" ]]; then
    echo "    Target: $TARGET"

    # For aarch64 cross-compilation, ensure C headers are available.
    # The bundled SQLite in rusqlite needs libc headers for the target.
    if [[ "$TARGET" == "aarch64-unknown-linux-gnu" && -z "${CFLAGS_aarch64_unknown_linux_gnu:-}" ]]; then
        SYSROOT="/tmp/aarch64-sysroot"
        if [[ ! -f "$SYSROOT/usr/include/stdio.h" ]]; then
            echo "    Setting up aarch64 sysroot (downloading headers)..."
            mkdir -p /tmp/aarch64-rpms
            dnf download --forcearch=aarch64 --destdir=/tmp/aarch64-rpms glibc-devel kernel-headers 2>/dev/null
            mkdir -p "$SYSROOT"
            for rpm in /tmp/aarch64-rpms/*.rpm; do
                rpm2cpio "$rpm" | (cd "$SYSROOT" && cpio -idm 2>/dev/null)
            done
        fi
        if [[ -f "$SYSROOT/usr/include/stdio.h" ]]; then
            echo "    Using aarch64 sysroot at $SYSROOT"
            export CFLAGS_aarch64_unknown_linux_gnu="--sysroot=$SYSROOT/usr -I$SYSROOT/usr/include"
        else
            echo "    WARNING: Could not set up aarch64 sysroot. Build may fail."
            echo "    Install glibc-devel.aarch64 or set CFLAGS_aarch64_unknown_linux_gnu manually."
        fi
    fi

    cargo build -p "$CRATE" --bin "$CRATE" \
      --release \
      --target "$TARGET" \
      --features ssr \
      --no-default-features
    BIN_PATH="target/$TARGET/release/$CRATE"
else
    cargo build -p "$CRATE" --bin "$CRATE" \
      --release \
      --features ssr \
      --no-default-features
    BIN_PATH="target/release/$CRATE"
fi

echo ""
echo "Done!"
echo "  Binary: $BIN_PATH"
echo "  Site:   $OUT_DIR/"

if [[ -n "$TARGET" ]]; then
    echo ""
    echo "Deploy to Pi with:"
    echo "  bash install.sh --local --ip <pi-address>"
else
    echo ""
    echo "Run with:"
    echo "  ./$BIN_PATH --storage-path /path/to/replayos --site-root $OUT_DIR"
fi
