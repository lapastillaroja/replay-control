#!/usr/bin/env bash
#
# Download metadata source files for the embedded game metadata database.
#
# This downloads:
#   1. No-Intro DAT files from libretro-database (for ROM identification)
#   2. libretro-database metadata files (maxusers, genre) for supplementary data
#   3. TheGamesDB JSON dump for rich metadata (year, genre, developer, players)
#
# These files are NOT checked into git. Run this script after cloning or when
# you want to refresh the data.
#
# Usage: ./scripts/download-metadata.sh [--force]
#
# Options:
#   --force   Re-download all files even if they already exist

set -euo pipefail

FORCE=false
if [[ "${1:-}" == "--force" ]]; then
    FORCE=true
    echo "Force mode: re-downloading all files"
    echo
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
DATA_DIR="$PROJECT_ROOT/data"

mkdir -p "$DATA_DIR/no-intro"
mkdir -p "$DATA_DIR/libretro-meta/maxusers"
mkdir -p "$DATA_DIR/libretro-meta/genre"

LIBRETRO_DB_RAW="https://raw.githubusercontent.com/libretro/libretro-database/master"

download() {
    local url="$1"
    local dest="$2"
    local desc="$3"

    if [ "$FORCE" = false ] && [ -f "$dest" ]; then
        echo "  Already exists: $dest (skipping, use --force to re-download)"
        return 0
    fi

    echo "  Downloading $desc..."
    if curl -fSL --retry 3 --retry-delay 2 -o "$dest" "$url"; then
        local size
        size=$(stat --printf="%s" "$dest" 2>/dev/null || stat -f "%z" "$dest" 2>/dev/null)
        echo "    OK ($size bytes)"
    else
        echo "    FAILED to download $desc" >&2
        rm -f "$dest"
        return 1
    fi
}

# ---------------------------------------------------------------------------
# 1. No-Intro DAT files (ClrMamePro format) from libretro-database
#    These provide canonical ROM filenames, regions, and CRC32 hashes.
# ---------------------------------------------------------------------------
echo "=== No-Intro DAT files ==="
echo

# Map of RePlayOS folder names to No-Intro DAT filenames in libretro-database
declare -A NOINTRO_DATS=(
    ["nintendo_nes"]="Nintendo - Nintendo Entertainment System.dat"
    ["nintendo_snes"]="Nintendo - Super Nintendo Entertainment System.dat"
    ["nintendo_gb"]="Nintendo - Game Boy.dat"
    ["nintendo_gbc"]="Nintendo - Game Boy Color.dat"
    ["nintendo_gba"]="Nintendo - Game Boy Advance.dat"
    ["nintendo_n64"]="Nintendo - Nintendo 64.dat"
    ["sega_sms"]="Sega - Master System - Mark III.dat"
    ["sega_smd"]="Sega - Mega Drive - Genesis.dat"
    ["sega_gg"]="Sega - Game Gear.dat"
)

for system in "${!NOINTRO_DATS[@]}"; do
    dat_name="${NOINTRO_DATS[$system]}"
    # URL-encode the filename (spaces -> %20)
    encoded_name="${dat_name// /%20}"
    dest="$DATA_DIR/no-intro/$dat_name"
    download \
        "$LIBRETRO_DB_RAW/metadat/no-intro/$encoded_name" \
        "$dest" \
        "No-Intro DAT: $dat_name" || true
done

echo
echo "=== libretro metadata files ==="
echo

# ---------------------------------------------------------------------------
# 2. libretro-database metadata: maxusers (player counts)
#    These are simple DAT files mapping ROM filenames to max player counts.
# ---------------------------------------------------------------------------
declare -A MAXUSERS_DATS=(
    ["nintendo_nes"]="Nintendo - Nintendo Entertainment System.dat"
    ["nintendo_snes"]="Nintendo - Super Nintendo Entertainment System.dat"
    ["nintendo_gb"]="Nintendo - Game Boy.dat"
    ["nintendo_gbc"]="Nintendo - Game Boy Color.dat"
    ["nintendo_gba"]="Nintendo - Game Boy Advance.dat"
    ["nintendo_n64"]="Nintendo - Nintendo 64.dat"
    ["sega_sms"]="Sega - Master System - Mark III.dat"
    ["sega_smd"]="Sega - Mega Drive - Genesis.dat"
    ["sega_gg"]="Sega - Game Gear.dat"
)

for system in "${!MAXUSERS_DATS[@]}"; do
    dat_name="${MAXUSERS_DATS[$system]}"
    encoded_name="${dat_name// /%20}"
    dest="$DATA_DIR/libretro-meta/maxusers/$dat_name"
    download \
        "$LIBRETRO_DB_RAW/metadat/maxusers/$encoded_name" \
        "$dest" \
        "maxusers: $dat_name" || true
done

# ---------------------------------------------------------------------------
# 3. libretro-database metadata: genre
# ---------------------------------------------------------------------------
declare -A GENRE_DATS=(
    ["nintendo_nes"]="Nintendo - Nintendo Entertainment System.dat"
    ["nintendo_snes"]="Nintendo - Super Nintendo Entertainment System.dat"
    ["nintendo_gb"]="Nintendo - Game Boy.dat"
    ["nintendo_gbc"]="Nintendo - Game Boy Color.dat"
    ["nintendo_gba"]="Nintendo - Game Boy Advance.dat"
    ["nintendo_n64"]="Nintendo - Nintendo 64.dat"
    ["sega_sms"]="Sega - Master System - Mark III.dat"
    ["sega_smd"]="Sega - Mega Drive - Genesis.dat"
    ["sega_gg"]="Sega - Game Gear.dat"
)

for system in "${!GENRE_DATS[@]}"; do
    dat_name="${GENRE_DATS[$system]}"
    encoded_name="${dat_name// /%20}"
    dest="$DATA_DIR/libretro-meta/genre/$dat_name"
    download \
        "$LIBRETRO_DB_RAW/metadat/genre/$encoded_name" \
        "$dest" \
        "genre: $dat_name" || true
done

echo
echo "=== TheGamesDB JSON dump ==="
echo

# ---------------------------------------------------------------------------
# 4. TheGamesDB JSON dump — rich metadata (year, genre, developer, players)
# ---------------------------------------------------------------------------
TGDB_DEST="$DATA_DIR/thegamesdb-latest.json"
download \
    "https://cdn.thegamesdb.net/json/database-latest.json" \
    "$TGDB_DEST" \
    "TheGamesDB JSON dump" || {
    echo "WARNING: TheGamesDB download failed. Build will proceed with" >&2
    echo "  No-Intro + libretro metadata only (no year/developer data)." >&2
}

echo
echo "=== Summary ==="
echo "Downloaded metadata to: $DATA_DIR"
echo
echo "No-Intro DATs:"
ls -lh "$DATA_DIR/no-intro/"*.dat 2>/dev/null | while read -r line; do echo "  $line"; done || echo "  (none)"
echo
echo "Libretro maxusers:"
ls -lh "$DATA_DIR/libretro-meta/maxusers/"*.dat 2>/dev/null | while read -r line; do echo "  $line"; done || echo "  (none)"
echo
echo "Libretro genre:"
ls -lh "$DATA_DIR/libretro-meta/genre/"*.dat 2>/dev/null | while read -r line; do echo "  $line"; done || echo "  (none)"
echo
if [ -f "$TGDB_DEST" ]; then
    echo "TheGamesDB: $(ls -lh "$TGDB_DEST" | awk '{print $5}')"
else
    echo "TheGamesDB: NOT DOWNLOADED"
fi
echo
echo "Done. You can now build replay-control-core to generate the game metadata DB."
