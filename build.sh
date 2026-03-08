#!/bin/bash
set -euo pipefail

CRATE="replay-app"
OUT_DIR="target/site"
PKG_DIR="$OUT_DIR/pkg"

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

# Copy CSS
cp "$CRATE/style/style.css" "$OUT_DIR/style.css"

echo "==> Building server (ssr)..."
cargo build -p "$CRATE" --bin "$CRATE" \
  --release \
  --features ssr \
  --no-default-features

echo ""
echo "Done!"
echo "  Binary: target/release/$CRATE"
echo "  Site:   $OUT_DIR/"
echo ""
echo "Run with:"
echo "  ./target/release/$CRATE --storage-path /path/to/replayos --site-root $OUT_DIR"
