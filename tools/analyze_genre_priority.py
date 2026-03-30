#!/usr/bin/env python3
"""
Analyze genre assignment priority in the game_db build-time logic.

Quantifies how many canonical games have their genre set from a beta/prototype
ROM's CRC rather than a primary ROM's CRC, and compares libretro vs TGDB genres.

Usage:
    python3 tools/analyze_genre_priority.py [--nfs-path /path/to/replayos-nfs/roms]
"""

import json
import os
import re
import sys
from collections import defaultdict
from pathlib import Path

# --------------------------------------------------------------------------- #
# Config
# --------------------------------------------------------------------------- #

DATA_DIR = Path(__file__).resolve().parent.parent / "data"
NOINTRO_DIR = DATA_DIR / "no-intro"
GENRE_DIR = DATA_DIR / "libretro-meta" / "genre"
TGDB_PATH = DATA_DIR / "thegamesdb-latest.json"

# Matches build.rs GAME_DB_SYSTEMS
SYSTEMS = [
    {"folder": "nintendo_nes", "dat": "Nintendo - Nintendo Entertainment System.dat", "tgdb_ids": [7]},
    {"folder": "nintendo_snes", "dat": "Nintendo - Super Nintendo Entertainment System.dat", "tgdb_ids": [6]},
    {"folder": "nintendo_gb", "dat": "Nintendo - Game Boy.dat", "tgdb_ids": [4]},
    {"folder": "nintendo_gbc", "dat": "Nintendo - Game Boy Color.dat", "tgdb_ids": [41]},
    {"folder": "nintendo_gba", "dat": "Nintendo - Game Boy Advance.dat", "tgdb_ids": [5]},
    {"folder": "nintendo_n64", "dat": "Nintendo - Nintendo 64.dat", "tgdb_ids": [3]},
    {"folder": "sega_sms", "dat": "Sega - Master System - Mark III.dat", "tgdb_ids": [35]},
    {"folder": "sega_smd", "dat": "Sega - Mega Drive - Genesis.dat", "tgdb_ids": [18, 36]},
    {"folder": "sega_gg", "dat": "Sega - Game Gear.dat", "tgdb_ids": [20]},
]

# Tags that indicate non-primary ROMs
BETA_TAGS = re.compile(r'\((Beta|Proto|Sample|Demo)\b', re.IGNORECASE)

# TGDB genre ID -> name (mirrors build.rs tgdb_genre_name)
TGDB_GENRE_MAP = {
    1: "Action", 2: "Adventure", 3: "Board", 4: "Card", 5: "Casino",
    6: "Educational", 7: "Family", 8: "Shooter", 9: "Fighting",
    10: "Horror", 11: "MMO", 12: "Music", 13: "Pinball", 14: "Platform",
    15: "Puzzle", 16: "Racing", 17: "Role-Playing", 18: "Sandbox",
    19: "Simulation", 20: "Sports", 21: "Stealth", 22: "Strategy",
    23: "Trivia", 24: "Comedy", 25: "Fitness", 26: "Flight Simulator",
    27: "Virtual Life", 28: "Compilation", 29: "Party", 30: "Rhythm",
}

# Mirrors build.rs normalize_console_genre
def normalize_genre(genre: str) -> str:
    mapping = {
        "Action": "Action",
        "Adventure": "Adventure",
        "Beat'em Up": "Beat'em Up", "Beat-'Em-Up": "Beat'em Up", "Beat 'Em Up": "Beat'em Up",
        "Board": "Board & Card", "Card": "Board & Card", "Board Game": "Board & Card",
        "Casino": "Board & Card", "Gambling": "Board & Card",
        "Racing": "Driving", "Driving": "Driving",
        "Educational": "Educational",
        "Fighting": "Fighting",
        "Music": "Music", "Rhythm": "Music", "Music and Dance": "Music",
        "Pinball": "Pinball",
        "Platform": "Platform",
        "Puzzle": "Puzzle",
        "Quiz": "Quiz", "Trivia": "Quiz",
        "Role-Playing": "Role-Playing", "Role-playing (RPG)": "Role-Playing",
        "RPG": "Role-Playing", "Role-Playing (RPG)": "Role-Playing",
        "Shooter": "Shooter", "Shoot-'Em-Up": "Shooter", "Shoot'em Up": "Shooter",
        "Lightgun Shooter": "Shooter", "Run & Gun": "Shooter", "Shoot 'Em Up": "Shooter",
        "Simulation": "Simulation", "Flight Simulator": "Simulation", "Virtual Life": "Simulation",
        "Sports": "Sports", "Fitness": "Sports",
        "Strategy": "Strategy",
        "Maze": "Maze",
        "Compilation": "Action", "Party": "Action",
        "Sandbox": "Action", "Stealth": "Action", "Horror": "Action",
        "MMO": "Action", "Family": "Action", "Comedy": "Action",
    }
    if not genre:
        return ""
    return mapping.get(genre, "Other")

# --------------------------------------------------------------------------- #
# Parsers (mirror build.rs logic)
# --------------------------------------------------------------------------- #

def parse_nointro_dat(path: Path) -> list[dict]:
    """Parse a No-Intro DAT file. Returns list of {name, rom_filename, region, crc32}."""
    entries = []
    if not path.exists():
        return entries

    in_game = False
    in_rom = False
    current = {}

    for line in path.read_text(errors="replace").splitlines():
        trimmed = line.strip()

        if trimmed == ")" and in_rom:
            in_rom = False
            continue

        if trimmed.startswith("game (") or trimmed == "game (":
            in_game = True
            current = {"name": "", "rom_filename": "", "region": "", "crc32": 0}
            continue

        if trimmed == ")" and in_game:
            in_game = False
            if current["name"] and current["rom_filename"]:
                if not current["region"]:
                    current["region"] = extract_region(current["name"])
                entries.append(current)
            continue

        if not in_game:
            continue

        # name field (not inside rom block)
        if trimmed.startswith("name ") and not in_rom:
            m = re.match(r'name\s+"([^"]*)"', trimmed)
            if m:
                current["name"] = m.group(1)

        if trimmed.startswith("region "):
            m = re.match(r'region\s+"([^"]*)"', trimmed)
            if m:
                current["region"] = m.group(1)

        if trimmed.startswith("rom (") or trimmed.startswith("rom("):
            in_rom = True
            m = re.search(r'name\s+"([^"]*)"', trimmed)
            if m:
                current["rom_filename"] = m.group(1)
            m = re.search(r'crc\s+([0-9A-Fa-f]+)', trimmed)
            if m:
                current["crc32"] = int(m.group(1), 16)
            if trimmed.endswith(")"):
                in_rom = False

    return entries


def extract_region(name: str) -> str:
    """Extract region from No-Intro name tags."""
    m = re.search(r'\((USA|Europe|Japan|World|Brazil|Korea|Australia|France|Germany|Italy|Spain|China|Taiwan)', name)
    return m.group(1) if m else ""


def parse_libretro_genre_dat(path: Path) -> dict[int, str]:
    """Parse a libretro genre DAT. Returns {crc32: genre_string}."""
    result = {}
    if not path.exists():
        return result

    in_game = False
    current_genre = ""
    current_crc = 0

    for line in path.read_text(errors="replace").splitlines():
        trimmed = line.strip()

        if trimmed.startswith("game (") or trimmed == "game (":
            in_game = True
            current_genre = ""
            current_crc = 0
            continue

        if trimmed == ")" and in_game:
            in_game = False
            if current_crc and current_genre:
                result[current_crc] = current_genre
            continue

        if not in_game:
            continue

        if trimmed.startswith("genre "):
            rest = trimmed[6:].strip().strip('"')
            current_genre = rest

        if trimmed.startswith("rom (") or trimmed.startswith("rom("):
            m = re.search(r'crc\s+([0-9A-Fa-f]+)', trimmed)
            if m:
                current_crc = int(m.group(1), 16)

    return result


def normalize_title(name: str) -> str:
    """Mirror build.rs normalize_title for grouping."""
    base = name.split("(")[0].strip()
    result = ""
    for ch in base:
        if ch.isalnum() or ch == " ":
            result += ch.lower()
    return " ".join(result.split())


def clean_display_name(name: str) -> str:
    """Mirror build.rs clean_display_name."""
    base = name.split("(")[0].strip()
    for article in [", The", ", An", ", A"]:
        idx = base.find(article)
        if idx >= 0:
            after = base[idx + len(article):]
            if not after or after.startswith(" - ") or after.startswith(" ~ "):
                art = article[2:]  # strip ", "
                if not after:
                    return f"{art} {base[:idx]}"
                else:
                    return f"{art} {base[:idx]}{after}"
    return base


def normalize_title_for_tgdb(title: str) -> str:
    """Simplified TGDB title normalization."""
    # Remove articles at start
    for prefix in ["The ", "A ", "An "]:
        if title.startswith(prefix):
            title = title[len(prefix):]
            break
    # Lowercase, strip punctuation, collapse spaces
    result = ""
    for ch in title:
        if ch.isalnum() or ch == " ":
            result += ch.lower()
    return " ".join(result.split())


def is_beta(name: str) -> bool:
    """Check if a ROM name indicates a beta/proto/sample/demo."""
    return bool(BETA_TAGS.search(name))


# --------------------------------------------------------------------------- #
# Main analysis
# --------------------------------------------------------------------------- #

def main():
    import argparse
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--nfs-path", type=Path, default=None,
                        help="Path to the ROM directory (e.g. /path/to/replayos-nfs/roms)")
    args = parser.parse_args()
    nfs_path = args.nfs_path

    # Load TGDB data
    print("Loading TGDB data...")
    tgdb = {}
    if TGDB_PATH.exists():
        with open(TGDB_PATH) as f:
            tgdb_json = json.load(f)
        for game in tgdb_json.get("data", {}).get("games", []):
            title = game.get("game_title", "")
            platform = game.get("platform", 0)
            genres = game.get("genres", []) or []
            players = game.get("players", 0) or 0
            year = 0
            rd = game.get("release_date", "")
            if rd and len(rd) >= 4:
                try:
                    year = int(rd[:4])
                except ValueError:
                    pass
            key = (normalize_title_for_tgdb(title), platform)
            tgdb[key] = {
                "title": title,
                "year": year,
                "players": players,
                "genre_ids": genres,
            }
    print(f"  TGDB: {len(tgdb)} entries loaded")

    # Aggregate stats across all systems
    total_groups = 0
    total_groups_with_betas = 0
    total_genre_from_beta = 0
    total_genre_from_primary = 0
    total_genre_mismatch_beta_vs_primary = 0
    total_libretro_tgdb_mismatch = 0
    total_user_affected = 0

    affected_games = []  # detailed list for output

    for sys_cfg in SYSTEMS:
        dat_name = sys_cfg["dat"]
        folder = sys_cfg["folder"]

        print(f"\n{'='*70}")
        print(f"System: {folder} ({dat_name})")
        print(f"{'='*70}")

        nointro_entries = parse_nointro_dat(NOINTRO_DIR / dat_name)
        genres = parse_libretro_genre_dat(GENRE_DIR / dat_name)
        print(f"  No-Intro entries: {len(nointro_entries)}")
        print(f"  Libretro genre entries: {len(genres)}")

        # Get user's ROM files if available (search recursively in subfolders)
        user_roms = set()
        if nfs_path:
            rom_dir = nfs_path / folder
            if rom_dir.exists():
                for f in rom_dir.rglob("*"):
                    if f.is_file():
                        user_roms.add(f.stem)
                        user_roms.add(f.name)

        # Group by normalized title (mirrors build.rs)
        game_groups = defaultdict(list)
        for idx, entry in enumerate(nointro_entries):
            key = normalize_title(entry["name"])
            game_groups[key].append(idx)

        sys_groups = 0
        sys_groups_with_betas = 0
        sys_genre_from_beta = 0
        sys_genre_mismatch = 0
        sys_libretro_tgdb_mismatch = 0
        sys_user_affected = 0

        for group_key in sorted(game_groups.keys()):
            indices = game_groups[group_key]
            sys_groups += 1

            # Classify entries as beta vs primary
            beta_indices = [i for i in indices if is_beta(nointro_entries[i]["name"])]
            primary_indices = [i for i in indices if not is_beta(nointro_entries[i]["name"])]
            has_betas = len(beta_indices) > 0
            if has_betas:
                sys_groups_with_betas += 1

            # Simulate build.rs genre assignment: iterate all indices, take first CRC match
            current_genre = ""
            genre_source_is_beta = False
            genre_source_entry = None
            for idx in indices:
                crc = nointro_entries[idx]["crc32"]
                if not current_genre and crc in genres:
                    current_genre = genres[crc]
                    genre_source_is_beta = is_beta(nointro_entries[idx]["name"])
                    genre_source_entry = nointro_entries[idx]["name"]
                    break

            # Also find what genre primary ROMs would give
            primary_genre = ""
            primary_genre_entry = None
            for idx in primary_indices:
                crc = nointro_entries[idx]["crc32"]
                if crc in genres:
                    primary_genre = genres[crc]
                    primary_genre_entry = nointro_entries[idx]["name"]
                    break

            # Also find what genre beta ROMs give
            beta_genre = ""
            beta_genre_entry = None
            for idx in beta_indices:
                crc = nointro_entries[idx]["crc32"]
                if crc in genres:
                    beta_genre = genres[crc]
                    beta_genre_entry = nointro_entries[idx]["name"]
                    break

            # Get TGDB genre for comparison
            best_idx = next(
                (i for i in indices if nointro_entries[i]["region"] in ("USA", "World")),
                indices[0]
            )
            display_name = clean_display_name(nointro_entries[best_idx]["name"])
            tgdb_genre_raw = ""
            tgdb_normalized = normalize_title_for_tgdb(display_name)
            for pid in sys_cfg["tgdb_ids"]:
                key = (tgdb_normalized, pid)
                if key in tgdb:
                    gids = tgdb[key]["genre_ids"]
                    if gids:
                        tgdb_genre_raw = TGDB_GENRE_MAP.get(gids[0], "Other")
                    break

            # Normalize for comparison
            norm_current = normalize_genre(current_genre)
            norm_primary = normalize_genre(primary_genre)
            norm_tgdb = normalize_genre(tgdb_genre_raw)

            # Check if genre was set from beta
            if genre_source_is_beta and current_genre:
                sys_genre_from_beta += 1

                # Check if primary ROMs give a different genre
                if primary_genre and norm_primary != normalize_genre(current_genre):
                    sys_genre_mismatch += 1

            # Check libretro vs TGDB mismatch (when genre came from beta)
            if genre_source_is_beta and norm_current and norm_tgdb and norm_current != norm_tgdb:
                sys_libretro_tgdb_mismatch += 1

            # Check if user is affected (has ROM files for this group)
            user_has_game = False
            if user_roms:
                for idx in indices:
                    entry = nointro_entries[idx]
                    stem = entry["rom_filename"].rsplit(".", 1)[0] if "." in entry["rom_filename"] else entry["rom_filename"]
                    if stem in user_roms or entry["rom_filename"] in user_roms:
                        user_has_game = True
                        break

            if genre_source_is_beta and current_genre:
                if user_has_game:
                    sys_user_affected += 1

                affected_games.append({
                    "system": folder,
                    "display_name": display_name,
                    "beta_genre": current_genre,
                    "beta_genre_norm": norm_current,
                    "beta_entry": genre_source_entry,
                    "primary_genre": primary_genre or "(none)",
                    "primary_genre_norm": norm_primary or "(none)",
                    "primary_entry": primary_genre_entry or "(none)",
                    "tgdb_genre": tgdb_genre_raw or "(none)",
                    "tgdb_genre_norm": norm_tgdb or "(none)",
                    "genres_conflict": (norm_primary and norm_primary != norm_current) or (norm_tgdb and norm_tgdb != norm_current),
                    "user_has_game": user_has_game,
                })

        print(f"  Total game groups: {sys_groups}")
        print(f"  Groups with beta/proto ROMs: {sys_groups_with_betas}")
        print(f"  Groups where genre came from beta CRC: {sys_genre_from_beta}")
        print(f"  Groups where beta genre != primary genre: {sys_genre_mismatch}")
        print(f"  Groups where beta-sourced genre != TGDB: {sys_libretro_tgdb_mismatch}")
        if nfs_path:
            print(f"  User ROMs on disk: {len(user_roms)}")
            print(f"  User-affected games (beta genre + on disk): {sys_user_affected}")

        total_groups += sys_groups
        total_groups_with_betas += sys_groups_with_betas
        total_genre_from_beta += sys_genre_from_beta
        total_genre_mismatch_beta_vs_primary += sys_genre_mismatch
        total_libretro_tgdb_mismatch += sys_libretro_tgdb_mismatch
        total_user_affected += sys_user_affected

    # Summary
    print(f"\n{'='*70}")
    print("OVERALL SUMMARY")
    print(f"{'='*70}")
    print(f"Total game groups (canonical games): {total_groups}")
    print(f"Groups containing beta/proto/sample/demo ROMs: {total_groups_with_betas}")
    print(f"Groups where genre was set from a beta ROM's CRC: {total_genre_from_beta}")
    print(f"  ...of which beta genre != primary ROM's genre: {total_genre_mismatch_beta_vs_primary}")
    print(f"  ...of which beta genre != TGDB genre: {total_libretro_tgdb_mismatch}")
    if nfs_path:
        print(f"User-affected games (on disk + beta genre): {total_user_affected}")

    # Detailed list of affected games
    if affected_games:
        print(f"\n{'='*70}")
        print("DETAILED: Games with genre set from beta CRC")
        print(f"{'='*70}")

        # First show only the conflicting ones
        conflicting = [g for g in affected_games if g["genres_conflict"]]
        if conflicting:
            print(f"\n--- Games where beta genre CONFLICTS with primary/TGDB ({len(conflicting)}) ---")
            for g in sorted(conflicting, key=lambda x: (x["system"], x["display_name"])):
                marker = " [ON DISK]" if g["user_has_game"] else ""
                print(f"  [{g['system']}] {g['display_name']}{marker}")
                print(f"    Beta genre:    {g['beta_genre']} -> {g['beta_genre_norm']}")
                print(f"      from: {g['beta_entry']}")
                print(f"    Primary genre: {g['primary_genre']} -> {g['primary_genre_norm']}")
                if g['primary_entry'] != "(none)":
                    print(f"      from: {g['primary_entry']}")
                print(f"    TGDB genre:    {g['tgdb_genre']} -> {g['tgdb_genre_norm']}")

        # Then the non-conflicting ones (beta genre same as would-be genre)
        non_conflicting = [g for g in affected_games if not g["genres_conflict"]]
        if non_conflicting:
            print(f"\n--- Games where beta genre matches primary/TGDB ({len(non_conflicting)}) ---")
            for g in sorted(non_conflicting, key=lambda x: (x["system"], x["display_name"])):
                marker = " [ON DISK]" if g["user_has_game"] else ""
                print(f"  [{g['system']}] {g['display_name']}{marker}")
                print(f"    Genre: {g['beta_genre']} -> {g['beta_genre_norm']} (from beta, same as primary)")

    # Output machine-readable summary for plan doc
    print(f"\n{'='*70}")
    print("MACHINE-READABLE SUMMARY")
    print(f"{'='*70}")
    print(f"TOTAL_GROUPS={total_groups}")
    print(f"GROUPS_WITH_BETAS={total_groups_with_betas}")
    print(f"GENRE_FROM_BETA={total_genre_from_beta}")
    print(f"GENRE_MISMATCH_BETA_VS_PRIMARY={total_genre_mismatch_beta_vs_primary}")
    print(f"LIBRETRO_TGDB_MISMATCH={total_libretro_tgdb_mismatch}")
    print(f"USER_AFFECTED={total_user_affected}")
    print(f"CONFLICTING_COUNT={len(conflicting) if affected_games else 0}")


if __name__ == "__main__":
    main()
