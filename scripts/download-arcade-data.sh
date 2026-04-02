#!/usr/bin/env bash
#
# Download arcade source data files used by replay-control-core's build.rs to generate
# the embedded arcade game database (PHF map).
#
# These files are NOT checked into git. Run this script after cloning or when
# you want to refresh the data.
#
# Usage: ./scripts/download-arcade-data.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
DATA_DIR="$PROJECT_ROOT/data"

mkdir -p "$DATA_DIR"

download() {
    local url="$1"
    local dest="$2"
    local desc="$3"

    echo "Downloading $desc..."
    echo "  URL:  $url"
    echo "  Dest: $dest"

    if curl -fSL --retry 3 --retry-delay 2 -o "$dest" "$url"; then
        local size
        size=$(stat --printf="%s" "$dest" 2>/dev/null || stat -f "%z" "$dest" 2>/dev/null)
        echo "  OK ($size bytes)"
    else
        echo "  FAILED to download $desc" >&2
        return 1
    fi
    echo
}

download \
    "https://raw.githubusercontent.com/libretro/FBNeo/master/dats/FinalBurn%20Neo%20(ClrMame%20Pro%20XML%2C%20Arcade%20only).dat" \
    "$DATA_DIR/fbneo-arcade.dat" \
    "FBNeo Arcade-only DAT"

download \
    "https://raw.githubusercontent.com/libretro/mame2003-plus-libretro/master/metadata/mame2003-plus.xml" \
    "$DATA_DIR/mame2003plus.xml" \
    "MAME 2003+ XML"

download \
    "https://raw.githubusercontent.com/libretro/mame2003-plus-libretro/master/metadata/catver.ini" \
    "$DATA_DIR/catver.ini" \
    "catver.ini (MAME 2003+ categories)"

# --- MAME current (0.285) ---
#
# The full MAME listxml is ~285 MB XML. We download the Progetto-SNAPS DAT pack
# (a 7z archive ~40 MB) which contains the full listxml, then preprocess it with
# a Python script to extract only arcade entries with the metadata fields we need.
# The result is a compact ~3.6 MB XML file.
#
# Requirements: 7z (p7zip) and python3.
#
MAME_VERSION="285"
MAME_7Z="$DATA_DIR/MAME_Dats_${MAME_VERSION}.7z"
MAME_OUTPUT="$DATA_DIR/mame0285-arcade.xml"

if [ -f "$MAME_OUTPUT" ]; then
    echo "MAME 0.${MAME_VERSION} arcade XML already exists at $MAME_OUTPUT, skipping."
    echo "  Delete it and re-run to refresh."
    echo
else
    # Check prerequisites
    if ! command -v 7z &>/dev/null; then
        echo "WARNING: 7z not found. Skipping MAME current download." >&2
        echo "  Install p7zip-full (apt) or p7zip (brew) to enable this." >&2
        echo
    elif ! command -v python3 &>/dev/null; then
        echo "WARNING: python3 not found. Skipping MAME current download." >&2
        echo
    else
        download \
            "https://www.progettosnaps.net/download/?tipo=dat_mame&file=/dats/MAME/packs/MAME_Dats_${MAME_VERSION}.7z" \
            "$MAME_7Z" \
            "MAME 0.${MAME_VERSION} DAT pack (7z archive)" || {
            echo "  WARNING: MAME DAT pack download failed." >&2
            echo "  Build will proceed without MAME current data." >&2
            echo
        }

        if [ -f "$MAME_7Z" ]; then
            echo "Extracting full MAME XML from archive..."
            TMPDIR_MAME=$(mktemp -d)
            trap "rm -rf '$TMPDIR_MAME'" EXIT

            7z e "$MAME_7Z" -o"$TMPDIR_MAME" "XML/mame_*_0.${MAME_VERSION}.xml" -y >/dev/null 2>&1

            MAME_FULL_XML=$(find "$TMPDIR_MAME" -name "*.xml" -type f | head -1)
            if [ -z "$MAME_FULL_XML" ]; then
                echo "  ERROR: Could not find XML file in archive" >&2
            else
                echo "  Preprocessing: extracting arcade metadata..."
                python3 "$SCRIPT_DIR/extract-mame-arcade.py" "$MAME_FULL_XML" "$MAME_OUTPUT"
                MAME_SIZE=$(stat --printf="%s" "$MAME_OUTPUT" 2>/dev/null || stat -f "%z" "$MAME_OUTPUT" 2>/dev/null)
                echo "  OK: $MAME_OUTPUT ($MAME_SIZE bytes)"
            fi

            # Clean up the large 7z archive and temp dir
            rm -f "$MAME_7Z"
            rm -rf "$TMPDIR_MAME"
            trap - EXIT
            echo
        fi
    fi
fi

# Download supplemental catver.ini for current MAME (covers games not in MAME 2003+).
#
# We merge two sources from AntoPISA's MAME_SupportFiles repo:
#   1. catver.ini — romname=category format, but often lags behind MAME releases
#   2. category.ini — category-as-section format, updated more frequently (tracks latest MAME)
#
# The merge script converts category.ini entries to catver format and adds any
# entries not already present in catver.ini, producing a single merged file.
CATVER_MAME_URL="https://raw.githubusercontent.com/AntoPISA/MAME_SupportFiles/refs/heads/main/catver.ini/catver.ini"
CATEGORY_INI_URL="https://raw.githubusercontent.com/AntoPISA/MAME_SupportFiles/refs/heads/main/category.ini/category.ini"

CATVER_MAME_OUTPUT="$DATA_DIR/catver-mame-current.ini"
CATVER_TMP="$DATA_DIR/.catver-raw.ini"
CATEGORY_TMP="$DATA_DIR/.category-raw.ini"

download \
    "$CATVER_MAME_URL" \
    "$CATVER_TMP" \
    "catver.ini (MAME categories from MAME_SupportFiles)" || true

download \
    "$CATEGORY_INI_URL" \
    "$CATEGORY_TMP" \
    "category.ini (MAME categories, updated more frequently)" || true

if [ -f "$CATVER_TMP" ] && [ -f "$CATEGORY_TMP" ]; then
    echo "Merging catver.ini + category.ini into catver-mame-current.ini..."
    python3 "$SCRIPT_DIR/merge-catver.py" "$CATVER_TMP" "$CATEGORY_TMP" "$CATVER_MAME_OUTPUT"
    rm -f "$CATVER_TMP" "$CATEGORY_TMP"
    echo
elif [ -f "$CATVER_TMP" ]; then
    # Fallback: use catver.ini alone if category.ini failed
    mv "$CATVER_TMP" "$CATVER_MAME_OUTPUT"
    echo "  (category.ini unavailable, using catver.ini only)"
    echo
fi

# Download nplayers.ini — player count data for arcade games.
# This supplements MAME/FBNeo metadata with player counts for entries that lack them.
# Source: http://nplayers.arcadebelgium.be (CC BY-SA 3.0)
NPLAYERS_VERSION="0278"
NPLAYERS_ZIP="$DATA_DIR/nplayers${NPLAYERS_VERSION}.zip"
NPLAYERS_OUTPUT="$DATA_DIR/nplayers.ini"

if [ -f "$NPLAYERS_OUTPUT" ]; then
    echo "nplayers.ini already exists at $NPLAYERS_OUTPUT, skipping."
    echo "  Delete it and re-run to refresh."
    echo
else
    download \
        "http://nplayers.arcadebelgium.be/files/nplayers${NPLAYERS_VERSION}.zip" \
        "$NPLAYERS_ZIP" \
        "nplayers.ini (player count data, v${NPLAYERS_VERSION})" || {
        echo "  WARNING: nplayers.ini download failed (host may be down)." >&2
        echo "  Build will proceed without supplementary player count data." >&2
        echo
    }

    if [ -f "$NPLAYERS_ZIP" ]; then
        echo "Extracting nplayers.ini from archive..."
        unzip -o -j "$NPLAYERS_ZIP" nplayers.ini -d "$DATA_DIR" >/dev/null 2>&1
        if [ -f "$NPLAYERS_OUTPUT" ]; then
            NPLAYERS_SIZE=$(stat --printf="%s" "$NPLAYERS_OUTPUT" 2>/dev/null || stat -f "%z" "$NPLAYERS_OUTPUT" 2>/dev/null)
            echo "  OK: $NPLAYERS_OUTPUT ($NPLAYERS_SIZE bytes)"
        else
            echo "  ERROR: nplayers.ini not found in archive" >&2
        fi
        rm -f "$NPLAYERS_ZIP"
        echo
    fi
fi

echo "All source data downloaded to $DATA_DIR"
