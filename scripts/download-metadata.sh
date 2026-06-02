#!/usr/bin/env bash
#
# Download metadata source files for the embedded game metadata database.
#
# This downloads:
#   1. No-Intro DAT files from libretro-database (for ROM identification)
#   2. libretro-database metadata files (maxusers, genre) for supplementary data
#   3. TheGamesDB JSON dump for rich metadata (year, genre, developer, players)
#   4. URL indexes for MiSTer and Retrokit manuals (PDFs are not downloaded)
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
# Downloaded third-party inputs live under data/upstream (regenerable,
# gitignored) — kept separate from committed curated data so build caches that
# restore this dir can never clobber source-of-truth files.
DATA_DIR="$PROJECT_ROOT/data/upstream"

mkdir -p "$DATA_DIR/no-intro"
mkdir -p "$DATA_DIR/libretro-meta/maxusers"
mkdir -p "$DATA_DIR/libretro-meta/genre"
mkdir -p "$DATA_DIR/mister-manuals"
mkdir -p "$DATA_DIR/retrokit-manuals"

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
    ["sega_sg"]="Sega - SG-1000.dat"
    ["sega_32x"]="Sega - 32X.dat"
    ["microsoft_msx"]="Microsoft - MSX.dat|Microsoft - MSX2.dat"
)

for system in "${!NOINTRO_DATS[@]}"; do
    IFS='|' read -r -a dat_names <<< "${NOINTRO_DATS[$system]}"
    for dat_name in "${dat_names[@]}"; do
        # URL-encode the filename (spaces -> %20)
        encoded_name="${dat_name// /%20}"
        dest="$DATA_DIR/no-intro/$dat_name"
        download \
            "$LIBRETRO_DB_RAW/metadat/no-intro/$encoded_name" \
            "$dest" \
            "No-Intro DAT: $dat_name" || true
    done
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
    ["sega_sg"]="Sega - SG-1000.dat"
    ["sega_32x"]="Sega - 32X.dat"
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
    ["sega_sg"]="Sega - SG-1000.dat"
    ["sega_32x"]="Sega - 32X.dat"
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
echo "=== Manual URL indexes ==="
echo

MISTER_RAW_BASE="https://raw.githubusercontent.com/ajgowans"
MISTER_MANUAL_REPOS=(
    manualsdb-3do manualsdb-atari2600 manualsdb-atari5200 manualsdb-atari7800
    manualsdb-atarilynx manualsdb-cdi manualsdb-fds manualsdb-gameboy
    manualsdb-gamegear manualsdb-gba manualsdb-gbc manualsdb-jaguar
    manualsdb-jaguarcd manualsdb-megadrive manualsdb-n64 manualsdb-neogeoaes
    manualsdb-neogeocd manualsdb-nes manualsdb-ngp manualsdb-ngpc
    manualsdb-psx manualsdb-sega32x manualsdb-segasaturn manualsdb-segasg1000
    manualsdb-segacd manualsdb-sms manualsdb-snes manualsdb-turbografx16
    manualsdb-turbografxcd
)

for repo in "${MISTER_MANUAL_REPOS[@]}"; do
    download \
        "$MISTER_RAW_BASE/$repo/main/external_files.csv" \
        "$DATA_DIR/mister-manuals/$repo.csv" \
        "MiSTer manuals index: $repo" || true
    download \
        "$MISTER_RAW_BASE/$repo/main/LICENSE" \
        "$DATA_DIR/mister-manuals/$repo.LICENSE" \
        "MiSTer manuals license: $repo" || true
done

RETROKIT_FOLDERS=(
    3do amiga arcade atari2600 atari5200 atari7800 atarijaguar atarilynx
    c64 dreamcast gamegear gb gba gbc mastersystem megadrive n64 nds
    neogeo neogeocd nes ngp pc pcengine pce-cd psx saturn sega32x segacd
    sg-1000 snes
)

for folder in "${RETROKIT_FOLDERS[@]}"; do
    download \
        "https://archive.org/download/retrokit-manuals/$folder/$folder-sources.tsv" \
        "$DATA_DIR/retrokit-manuals/$folder-sources.tsv" \
        "Retrokit manuals index: $folder" || true
done

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
echo "Manual indexes:"
echo "  MiSTer:   $(find "$DATA_DIR/mister-manuals" -name '*.csv' 2>/dev/null | wc -l | tr -d ' ') CSV files"
echo "  Retrokit: $(find "$DATA_DIR/retrokit-manuals" -name '*-sources.tsv' 2>/dev/null | wc -l | tr -d ' ') TSV files"
echo
echo "Done. You can now build replay-control-core to generate the game metadata DB."
