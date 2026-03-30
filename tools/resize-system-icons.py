#!/usr/bin/env python3
"""Resize and center system controller icons from 300x300 originals.

Source: KyleBing/retro-game-console-icons (GPLv3)
        series_trimui/300w@1x/ directory

Usage:
    python3 tools/resize-system-icons.py [source_dir]

    source_dir: directory containing the 300x300 PNGs (default: /tmp/arcade-icons/png-options)

Output: replay-control-app/static/icons/systems/ (112x112 PNGs, trimmed + centered)

Requires: ImageMagick 7 (`magick` command)
"""

import subprocess
import os
import shutil
import sys

# Source → destination filename mapping (KyleBing name → app folder_name)
MAPPING = {
    "ARCADE": "arcade_fbneo",
    "MAME": "arcade_mame",
    "MAME2003PLUS": "arcade_mame_2k3p",
    "NAOMI": "arcade_dc",
    "ATARI2600": "atari_2600",
    "ATARI5200": "atari_5200",
    "ATARI7800": "atari_7800",
    "LYNX": "atari_lynx",
    "CPC": "amstrad_cpc",
    "AMIGA": "commodore_ami",
    "C64": "commodore_c64",
    "DOS": "ibm_pc",
    "MSX": "microsoft_msx",
    "PCE": "nec_pce",
    "PCECD": "nec_pcecd",
    "NDS": "nintendo_ds",
    "GB": "nintendo_gb",
    "GBA": "nintendo_gba",
    "GBC": "nintendo_gbc",
    "N64": "nintendo_n64",
    "FC": "nintendo_nes",
    "SFC": "nintendo_snes",
    "PANASONIC": "panasonic_3do",
    "SCUMMVM": "scummvm",
    "SEGA32X": "sega_32x",
    "SEGACD": "sega_cd",
    "DC": "sega_dc",
    "GG": "sega_gg",
    "SG1000": "sega_sg",
    "MD": "sega_smd",
    "MS": "sega_sms",
    "SATURN": "sega_st",
    "X68000": "sharp_x68k",
    "ZXS": "sinclair_zx",
    "NEOGEO": "snk_ng",
    "NEOCD": "snk_ngcd",
    "NGP": "snk_ngp",
    "PS": "sony_psx",
}

# Icons that use a different source (fallback or alias)
FALLBACKS = {
    "commodore_amicd": "commodore_ami",  # copy from Amiga
}

# Icons generated from non-KyleBing sources
EXTERNAL_FALLBACKS = {
    "atari_jaguar": "atari-jaguar-solid-56px.png",  # Controllercons
    "philips_cdi": "generic-gamepad-56px.png",       # Phosphor
    "alpha_player": "generic-gamepad-56px.png",      # Phosphor
}

SIZE = 112       # output canvas size (2x retina for ~56px display)
CONTENT = 100    # content area after trim (leaves padding for centering)


def process_icon(src_path, dst_path):
    """Remove drop shadow, trim, resize and center.

    The KyleBing icons have a drop shadow (semi-transparent pixels) below
    and to the right. A fuzz trim removes these before centering, so the
    controller visual itself is centered — not the controller+shadow.
    """
    subprocess.run([
        "magick", src_path,
        "-fuzz", "20%", "-trim", "+repage",
        "-resize", f"{CONTENT}x{CONTENT}",
        "-gravity", "center",
        "-background", "none",
        "-extent", f"{SIZE}x{SIZE}",
        dst_path,
    ], capture_output=True)


def main():
    src_dir = sys.argv[1] if len(sys.argv) > 1 else "/tmp/arcade-icons/png-options"
    fallback_dir = os.path.join(os.path.dirname(src_dir), "missing")
    dest = os.path.join(os.path.dirname(__file__), "..",
                        "replay-control-app", "static", "icons", "systems")
    dest = os.path.normpath(dest)
    os.makedirs(dest, exist_ok=True)

    ok = 0
    missing = []

    # Main mapping
    for src_name, dst_name in MAPPING.items():
        src_path = os.path.join(src_dir, f"{src_name}.png")
        dst_path = os.path.join(dest, f"{dst_name}.png")
        if os.path.exists(src_path):
            process_icon(src_path, dst_path)
            ok += 1
        else:
            missing.append(src_name)

    # External fallbacks
    for dst_name, fallback_file in EXTERNAL_FALLBACKS.items():
        src_path = os.path.join(fallback_dir, fallback_file)
        dst_path = os.path.join(dest, f"{dst_name}.png")
        if os.path.exists(src_path):
            process_icon(src_path, dst_path)
            ok += 1
        else:
            missing.append(dst_name)

    # Copy-based fallbacks (after main icons are generated)
    for dst_name, src_name in FALLBACKS.items():
        src_path = os.path.join(dest, f"{src_name}.png")
        dst_path = os.path.join(dest, f"{dst_name}.png")
        if os.path.exists(src_path):
            shutil.copy(src_path, dst_path)
            ok += 1

    # Verify
    errors = 0
    for f in sorted(os.listdir(dest)):
        if not f.endswith(".png"):
            continue
        r = subprocess.run(
            ["magick", "identify", os.path.join(dest, f)],
            capture_output=True, text=True,
        )
        if f"{SIZE}x{SIZE}" not in r.stdout:
            print(f"  ERROR: {f} — {r.stdout.strip()}")
            errors += 1

    print(f"Done: {ok} icons at {SIZE}x{SIZE}")
    if missing:
        print(f"Missing sources: {', '.join(missing)}")
    if errors:
        print(f"Errors: {errors}")
        sys.exit(1)


if __name__ == "__main__":
    main()
