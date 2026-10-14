#!/usr/bin/env python3
"""Resize and center system controller icons from source artwork.

Source: KyleBing/retro-game-console-icons (GPLv3)
        series_trimui/300w@1x/ directory

Usage:
    python3 tools/resize-system-icons.py [source_dir] [--only system_folder]

    source_dir: directory containing the 300x300 PNGs (default: /tmp/arcade-icons/png-options)
    Generated PS2/GameCube/Wii/Triforce sources: private repo design/assets/system-icons/

Output: replay-control-app/static/icons/systems/ (112x112 PNGs, trimmed + centered)

Requires: ImageMagick 7 (`magick` command)
"""

import subprocess
import os
import sys
import argparse

# Source → destination filename mapping (source artwork name → app folder_name)
MAPPING = {
    "ARCADE_FBNEO": "arcade_fbneo",  # FBNeo-branded cabinet (was the generic ARCADE)
    "STV": "arcade_stv",  # generated Sega Titan Video mark
    "MAME": "arcade_mame",
    "MAME2003PLUS": "arcade_mame_2k3p",
    "NAOMI": "arcade_dc",
    "ATARI2600": "atari_2600",
    "ATARI5200": "atari_5200",
    "ATARI7800": "atari_7800",
    "LYNX": "atari_lynx",
    "CPC": "amstrad_cpc",
    "AMIGA": "commodore_ami",
    "AMIGACD": "commodore_amicd",
    "C64": "commodore_c64",
    "DOS": "ibm_pc",
    "MSX": "microsoft_msx",
    "PCE": "nec_pce",
    "PCECD": "nec_pcecd",
    "NDS": "nintendo_ds",
    "GAMECUBE": "nintendo_gc",  # generated controller; KyleBing NGC is Neo Geo Color
    "WII": "nintendo_wii",  # generated Wii Remote and Nunchuk
    "VB": "nintendo_vb",
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
    "PS2": "sony_ps2",  # approved generated DualShock 2, archived in private repo
    "TRIFORCE": "arcade_triforce",  # generated hardware enclosure; no KyleBing source
    "PSP": "sony_psp",
    "VIDEOS": "alpha_player",
}

# Atari Jaguar, Atari Jaguar CD, and Philips CD-i use project-local generated
# icons because the KyleBing source collection has no artwork for those systems.

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
    ], check=True, capture_output=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source_dir", nargs="?", default="/tmp/arcade-icons/png-options")
    parser.add_argument("--only", choices=sorted(MAPPING.values()), help="Regenerate only this system")
    args = parser.parse_args()
    src_dir = args.source_dir
    dest = os.path.join(os.path.dirname(__file__), "..",
                        "replay-control-app", "static", "icons", "systems")
    dest = os.path.normpath(dest)
    os.makedirs(dest, exist_ok=True)

    ok = 0
    missing = []

    # Main mapping
    for src_name, dst_name in MAPPING.items():
        if args.only and dst_name != args.only:
            continue
        src_path = os.path.join(src_dir, f"{src_name}.png")
        dst_path = os.path.join(dest, f"{dst_name}.png")
        if os.path.exists(src_path):
            process_icon(src_path, dst_path)
            ok += 1
        else:
            missing.append(src_name)

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
        if args.only:
            sys.exit(1)
    if errors:
        print(f"Errors: {errors}")
        sys.exit(1)


if __name__ == "__main__":
    main()
