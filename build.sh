#!/bin/bash
set -euo pipefail

CRATE="replay-control-app"
CRATE_SNAKE="${CRATE//-/_}"
TARGET_DIR="${CARGO_TARGET_DIR:-target}"
OUT_DIR="$TARGET_DIR/site"
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

# ── Download data files if missing ─────────────────────────────────
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

if [[ "${SKIP_DATA:-}" == "1" ]]; then
    echo "==> SKIP_DATA=1: skipping data download + catalog rebuild."
else
    # Probe every downloaded input the catalog's preflight requires (mirror of
    # build-catalog REQUIRED_SOURCES). Downloaded inputs live under
    # data/upstream; data/wikidata is committed. Checking only a subset lets a
    # partial cache (e.g. fbneo present but mame/no-intro/catver missing after a
    # failed step) report "present" and then fail preflight, so list them all.
    # TGDB lookups are handled separately below (they need an API key).
    data_missing=false
    for req in \
        upstream/fbneo-arcade.dat \
        upstream/mame2003plus.xml \
        upstream/mame0285-arcade.xml \
        upstream/no-intro \
        upstream/thegamesdb-latest.json \
        upstream/catver.ini \
        upstream/catver-mame-current.ini \
        upstream/nplayers.ini \
        upstream/libretro-meta/maxusers \
        upstream/libretro-meta/genre \
        upstream/amiga/whdload_db.xml \
        upstream/mister-manuals \
        upstream/retrokit-manuals \
        wikidata/series.json; do
        [[ ! -e "$SCRIPT_DIR/data/$req" ]] && data_missing=true
    done

    if [[ "$data_missing" == "true" ]]; then
        echo "==> Downloading data files..."
        bash "$SCRIPT_DIR/scripts/download-arcade-data.sh"
        bash "$SCRIPT_DIR/scripts/download-metadata.sh"
        mkdir -p "$SCRIPT_DIR/data/wikidata"
        if [[ ! -s "$SCRIPT_DIR/data/wikidata/series.json" ]]; then
            if [[ "${CI:-}" == "true" ]]; then
                echo "ERROR: data/wikidata/series.json is missing or empty." >&2
                echo "Release builds use the committed Wikidata snapshot and must not query live SPARQL." >&2
                exit 1
            fi
            python3 "$SCRIPT_DIR/scripts/wikidata-series-extract.py" > "$SCRIPT_DIR/data/wikidata/series.json"
        fi
        echo "    Data files ready."
    else
        echo "==> Data files present, skipping download."
    fi

    # TGDB ID→name lookups (developer/publisher/genre). These need a TheGamesDB
    # API key; download-tgdb-lookups.sh sources scripts/.env. When no key is
    # configured the lookups are skipped — build-catalog then fails its strict
    # preflight (a partial catalog with empty developers/publishers is a
    # shipping defect). Pass --allow-partial deliberately for a keyless build.
    if [[ ! -s "$SCRIPT_DIR/data/upstream/tgdb-developers.json" ]]; then
        echo "==> Fetching TGDB lookup tables..."
        if bash "$SCRIPT_DIR/scripts/download-tgdb-lookups.sh"; then
            echo "    TGDB lookups ready."
        else
            echo "    WARNING: no TGDB API key — developer/publisher data will be missing." >&2
            echo "    Set TGDB_API_KEY (scripts/.env) or pass BUILD_CATALOG_ALLOW_PARTIAL=1." >&2
        fi
    fi

    echo "==> Building game catalog..."
    catalog_args=(--output catalog.sqlite)
    [[ "${BUILD_CATALOG_ALLOW_PARTIAL:-}" == "1" ]] && catalog_args+=(--allow-partial)
    if ! cargo run --release -p build-catalog -- "${catalog_args[@]}"; then
        echo "ERROR: catalog build failed" >&2
        exit 1
    fi
fi

echo "==> Building WASM (hydrate)..."
cargo build -p "$CRATE" --lib \
  --target wasm32-unknown-unknown \
  --profile wasm-release \
  --features hydrate \
  --no-default-features

echo "==> Running wasm-bindgen..."
mkdir -p "$PKG_DIR"
wasm-bindgen \
  "$TARGET_DIR/wasm32-unknown-unknown/wasm-release/${CRATE_SNAKE}.wasm" \
  --out-dir "$PKG_DIR" \
  --out-name "${CRATE_SNAKE}" \
  --target web \
  --no-typescript

# Optimize WASM with wasm-opt if available.
WASM_FILE="$PKG_DIR/${CRATE_SNAKE}_bg.wasm"
JS_FILE="$PKG_DIR/${CRATE_SNAKE}.js"
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

# Content-hash assets for cache busting (Leptos hash_files convention).
echo "==> Hashing assets..."
WASM_HASH=$(sha256sum "$WASM_FILE" | cut -c1-16)
HASHED_WASM="$PKG_DIR/${CRATE_SNAKE}.${WASM_HASH}.wasm"
mv "$WASM_FILE" "$HASHED_WASM"
sed -i "s|${CRATE_SNAKE}_bg\.wasm|${CRATE_SNAKE}.${WASM_HASH}.wasm|g" "$JS_FILE"
JS_HASH=$(sha256sum "$JS_FILE" | cut -c1-16)
HASHED_JS="$PKG_DIR/${CRATE_SNAKE}.${JS_HASH}.js"
mv "$JS_FILE" "$HASHED_JS"
printf 'js: %s\nwasm: %s\n' "$JS_HASH" "$WASM_HASH" > "$OUT_DIR/hash.txt"
echo "    js:   ${JS_HASH}"
echo "    wasm: ${WASM_HASH}"

# Pre-compress WASM for static serving.
echo "==> Pre-compressing WASM..."
gzip -9 -k -f "$HASHED_WASM"
GZ_SIZE=$(stat -c%s "${HASHED_WASM}.gz" 2>/dev/null || echo 0)
echo "    ${HASHED_WASM}.gz: ${GZ_SIZE} bytes"

# Copy static assets
cat replay-control-app/style/_*.css > "$OUT_DIR/style.css"
rm -rf "$OUT_DIR/icons" "$OUT_DIR/branding"
cp -r "replay-control-app/static/icons" "$OUT_DIR/icons" 2>/dev/null || true
cp -r "replay-control-app/static/branding" "$OUT_DIR/branding" 2>/dev/null || true

echo "==> Building server (ssr)..."
if [[ -n "$TARGET" ]]; then
    echo "    Target: $TARGET"

    # For aarch64 cross-compilation, ensure C headers are available.
    # The bundled SQLite in rusqlite needs libc headers for the target.
    if [[ "$TARGET" == "aarch64-unknown-linux-gnu" ]]; then
        AARCH64_SYSROOT="${AARCH64_SYSROOT:-}"
        if [[ -z "$AARCH64_SYSROOT" ]]; then
            # Auto-detect sysroot location (varies by distro):
            #   Fedora: /usr/aarch64-linux-gnu/sys-root/usr/include/stdio.h
            #   Ubuntu: /usr/aarch64-linux-gnu/usr/include/stdio.h
            #           or /usr/aarch64-linux-gnu/include/stdio.h
            for _sysroot in "/usr/aarch64-linux-gnu/sys-root" "/usr/aarch64-linux-gnu"; do
                for _inc in "$_sysroot/usr/include" "$_sysroot/include"; do
                    if [[ -f "$_inc/stdio.h" ]]; then
                        AARCH64_SYSROOT="$_sysroot"
                        break 2
                    fi
                done
            done
        fi
        if [[ -z "$AARCH64_SYSROOT" ]]; then
            echo ""
            echo "    ERROR: aarch64 sysroot not found."
            echo "    Searched: /usr/aarch64-linux-gnu/sys-root (Fedora)"
            echo "             /usr/aarch64-linux-gnu (Ubuntu/Debian)"
            echo "    Set AARCH64_SYSROOT to override. See CONTRIBUTING.md for cross-compilation setup."
            echo ""
            exit 1
        fi
        echo "    Sysroot: $AARCH64_SYSROOT"
    fi

    cargo build -p "$CRATE" --bin "$CRATE" \
      --release \
      --target "$TARGET" \
      --features ssr \
      --no-default-features
    BIN_PATH="$TARGET_DIR/$TARGET/release/$CRATE"
else
    cargo build -p "$CRATE" --bin "$CRATE" \
      --release \
      --features ssr \
      --no-default-features
    BIN_PATH="$TARGET_DIR/release/$CRATE"
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
