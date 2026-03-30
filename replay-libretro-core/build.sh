#!/bin/bash
set -euo pipefail

# Build the replay-libretro-core for local testing or Pi deployment.
#
# Usage:
#   ./build.sh              # Build for local x86_64
#   ./build.sh aarch64      # Cross-compile for Pi (aarch64)
#
# The output .so file is renamed to follow the libretro naming convention:
#   replay_hello_world_libretro.so
#
# ─── Deploying to Pi as a replacement core ──────────────────────────────────
#
# To test the full pipeline (Replay Control app → RetroArch → our core),
# you can replace an existing core's .so with ours. For example, to
# replace the NES core (fceumm):
#
#   1. Build for aarch64:
#        ./build.sh aarch64
#
#   2. SSH into the Pi and back up the original core:
#        ssh root@replay.local 'cp /opt/replay/cores/fceumm_libretro.so /opt/replay/cores/fceumm_libretro.so.bak'
#
#   3. Deploy our core under the target core's name:
#        scp target/aarch64-unknown-linux-gnu/release/replay_hello_world_libretro.so \
#            root@replay.local:/opt/replay/cores/fceumm_libretro.so
#
#   4. Also copy the original .so alongside (for easy restore):
#        scp target/aarch64-unknown-linux-gnu/release/replay_hello_world_libretro.so \
#            root@replay.local:/opt/replay/cores/
#
#   5. Launch any NES game from the Replay Control app. RetroArch will
#      load our core instead of fceumm. You'll see the ROM info screen
#      with the file's name, size, CRC32, and byte histogram.
#
#   6. To restore the original core:
#        ssh root@replay.local 'cp /opt/replay/cores/fceumm_libretro.so.bak /opt/replay/cores/fceumm_libretro.so'
#
# Other cores you could replace for testing:
#   - fceumm_libretro.so      (NES — most games are small, fast to load)
#   - gambatte_libretro.so     (Game Boy)
#   - mgba_libretro.so         (GBA)
#   - genesis_plus_gx_libretro.so (Mega Drive/Genesis)
#   - snes9x_libretro.so       (SNES)
#

CORE_NAME="replay_hello_world"
TARGET=""

for arg in "$@"; do
    case "$arg" in
        aarch64) TARGET="aarch64-unknown-linux-gnu" ;;
    esac
done

echo "==> Building ${CORE_NAME}_libretro.so..."

if [[ -n "$TARGET" ]]; then
    echo "    Target: $TARGET"
    cargo build --release --target "$TARGET"
    SRC="target/$TARGET/release/libreplay_libretro_core.so"
    OUT_DIR="target/$TARGET/release"
else
    cargo build --release
    SRC="target/release/libreplay_libretro_core.so"
    OUT_DIR="target/release"
fi

# Rename to libretro convention
DEST="${OUT_DIR}/${CORE_NAME}_libretro.so"
cp "$SRC" "$DEST"

# Strip symbols for smaller binary
if [[ -n "$TARGET" ]]; then
    aarch64-linux-gnu-strip "$DEST" 2>/dev/null || echo "    (strip not available for target)"
else
    strip "$DEST" 2>/dev/null || echo "    (strip not available)"
fi

SIZE=$(stat -c%s "$DEST" 2>/dev/null || echo "?")
echo ""
echo "Done!"
echo "  Output: $DEST"
echo "  Size:   $SIZE bytes"

if [[ -n "$TARGET" ]]; then
    echo ""
    echo "Deploy to Pi (as itself):"
    echo "  scp $DEST root@replay.local:/opt/replay/cores/"
    echo ""
    echo "Deploy as NES core replacement (for testing):"
    echo "  ssh root@replay.local 'cp /opt/replay/cores/fceumm_libretro.so /opt/replay/cores/fceumm_libretro.so.bak'"
    echo "  scp $DEST root@replay.local:/opt/replay/cores/fceumm_libretro.so"
    echo ""
    echo "Restore original NES core:"
    echo "  ssh root@replay.local 'cp /opt/replay/cores/fceumm_libretro.so.bak /opt/replay/cores/fceumm_libretro.so'"
fi
