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
  --release \
  --features hydrate \
  --no-default-features

echo "==> Running wasm-bindgen..."
mkdir -p "$PKG_DIR"
wasm-bindgen \
  "target/wasm32-unknown-unknown/release/${CRATE//-/_}.wasm" \
  --out-dir "$PKG_DIR" \
  --out-name "${CRATE//-/_}" \
  --target web \
  --no-typescript

# Copy static assets
cp "replay-control-app/style/style.css" "$OUT_DIR/style.css"
cp -r "replay-control-app/static/icons" "$OUT_DIR/icons" 2>/dev/null || true

echo "==> Building server (ssr)..."
if [[ -n "$TARGET" ]]; then
    echo "    Target: $TARGET"
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
