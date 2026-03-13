#!/usr/bin/env python3
"""Compare genre data between baked-in game_db and LaunchBox metadata.

For each ROM on disk (NFS), looks up:
  - What genre the baked-in game_db assigns (from generated game_db.rs)
  - What genre LaunchBox assigns (from Metadata.xml)
  - Whether they agree or disagree

Reports per-system stats and interesting disagreements.
"""

import os
import re
import sys
import json
import xml.etree.ElementTree as ET
from collections import defaultdict
from pathlib import Path

# --- Configuration ---

NFS_ROMS = Path("<NFS_MOUNT>/roms")
GAME_DB_RS = Path(
    "<WORKSPACE>/target/debug/build/"
    "replay-control-core-38b2403f41389830/out/game_db.rs"
)
LAUNCHBOX_XML = Path("/tmp/launchbox/Metadata.xml")

# Systems with baked-in game_db data
SYSTEMS = {
    "sega_smd": "SMD",
    "nintendo_snes": "SNES",
    "nintendo_nes": "NES",
    "nintendo_gba": "GBA",
    "nintendo_gb": "GB",
    "nintendo_gbc": "GBC",
    "nintendo_n64": "N64",
    "sega_sms": "SMS",
    "sega_gg": "GG",
}

# LaunchBox platform -> our system folder
LB_PLATFORM_MAP = {
    "Sega Genesis": "sega_smd",
    "Sega Mega Drive": "sega_smd",
    "Super Nintendo Entertainment System": "nintendo_snes",
    "Nintendo Entertainment System": "nintendo_nes",
    "Nintendo Game Boy Advance": "nintendo_gba",
    "Nintendo Game Boy": "nintendo_gb",
    "Nintendo Game Boy Color": "nintendo_gbc",
    "Nintendo 64": "nintendo_n64",
    "Sega Master System": "sega_sms",
    "Sega Game Gear": "sega_gg",
}

# Normalize genres to a common taxonomy for fair comparison
GENRE_NORMALIZE = {
    # game_db normalized genres
    "Action": "Action",
    "Adventure": "Adventure",
    "Beat'em Up": "Beat'em Up",
    "Board & Card": "Board & Card",
    "Driving": "Racing",
    "Educational": "Educational",
    "Fighting": "Fighting",
    "Music": "Music",
    "Pinball": "Pinball",
    "Platform": "Platform",
    "Puzzle": "Puzzle",
    "Quiz": "Quiz",
    "Role-Playing": "Role-Playing",
    "Shooter": "Shooter",
    "Simulation": "Simulation",
    "Sports": "Sports",
    "Strategy": "Strategy",
    "Maze": "Maze",
    "Other": "Other",
    # LaunchBox genre strings
    "Beat 'em Up": "Beat'em Up",
    "Board Game": "Board & Card",
    "Casino": "Board & Card",
    "Compilation": "Compilation",
    "Construction and Management Simulation": "Simulation",
    "Education": "Educational",
    "Flight Simulator": "Simulation",
    "Horror": "Action",
    "Life Simulation": "Simulation",
    "Party": "Action",
    "Racing": "Racing",
    "Sandbox": "Action",
    "Stealth": "Action",
    "Vehicle Simulation": "Simulation",
    "Visual Novel": "Adventure",
}


def norm_genre(g):
    """Normalize a genre string for comparison."""
    if not g or not g.strip():
        return ""
    g = g.strip()
    # LaunchBox uses semicolons for multiple genres - take the first
    if ";" in g:
        g = g.split(";")[0].strip()
    return GENRE_NORMALIZE.get(g, g)


def normalize_title(name):
    """Normalize a ROM filename for matching (mirrors the Rust normalize_title)."""
    # Strip extension
    stem = name
    if "." in stem:
        stem = stem[:stem.rfind(".")]

    # Strip parenthetical/bracket tags
    result = []
    depth = 0
    for ch in stem:
        if ch in "([":
            depth += 1
        elif ch in ")]":
            depth = max(0, depth - 1)
        elif depth == 0:
            result.append(ch)

    stripped = "".join(result).strip()

    # Handle "Title, The" -> "The Title"
    if ", " in stripped:
        idx = stripped.rfind(", ")
        before = stripped[:idx]
        after_comma = stripped[idx + 2:]
        # Extract first word
        match = re.match(r"([A-Za-z]+)(.*)", after_comma)
        if match:
            first_word = match.group(1)
            rest = match.group(2).lstrip(" -")
            if first_word.lower() in ("the", "a", "an"):
                if rest:
                    stripped = f"{first_word} {before} {rest}"
                else:
                    stripped = f"{first_word} {before}"

    # Keep only alphanumeric, lowercase
    return "".join(ch.lower() for ch in stripped if ch.isalnum())


def parse_game_db_rs(path):
    """Parse the generated game_db.rs to extract genre and players per system.

    Returns: {system_folder: {normalized_title: {"genre": str, "players": int, "display_name": str}}}
    """
    data = {}
    current_system = None
    prefix_to_system = {v: k for k, v in SYSTEMS.items()}

    with open(path, "r") as f:
        content = f.read()

    # Parse canonical game arrays: static PREFIX_GAMES: &[CanonicalGame] = &[...];
    for prefix, system in prefix_to_system.items():
        pattern = rf'static {prefix}_GAMES: &\[CanonicalGame\] = &\[(.*?)\];'
        match = re.search(pattern, content, re.DOTALL)
        if not match:
            continue

        games = {}
        game_list = []
        block = match.group(1)

        # Parse each CanonicalGame entry
        for game_match in re.finditer(
            r'CanonicalGame \{[^}]*display_name: "([^"]*)".*?genre: "([^"]*)".*?players: (\d+).*?normalized_genre: "([^"]*)"',
            block,
        ):
            display_name = game_match.group(1).replace('\\"', '"').replace("\\\\", "\\")
            genre = game_match.group(2)
            players = int(game_match.group(3))
            norm_g = game_match.group(4)
            game_list.append({
                "display_name": display_name,
                "genre": genre,
                "players": players,
                "normalized_genre": norm_g,
            })

        # Now parse ROM DB (PHF format) to get filename_stem -> game_id mapping
        # PHF entries format: ("filename_stem", GameEntry { ... game: &PREFIX_GAMES[N] })
        rom_pattern = rf'\("([^"]*(?:\\.[^"]*)*)", GameEntry \{{[^}}]*game: &{prefix}_GAMES\[(\d+)\]\s*\}}\)'
        for rom_match in re.finditer(rom_pattern, content):
            filename_stem = rom_match.group(1).replace('\\"', '"').replace("\\\\", "\\")
            game_id = int(rom_match.group(2))
            if game_id < len(game_list):
                game = game_list[game_id]
                norm_title = normalize_title(filename_stem)
                if norm_title not in games:
                    games[norm_title] = {
                        "genre": game["normalized_genre"],
                        "players": game["players"],
                        "display_name": game["display_name"],
                        "raw_genre": game["genre"],
                        "filename_stem": filename_stem,
                    }

        data[system] = games

    return data


def parse_launchbox_xml(path, platforms):
    """Stream-parse LaunchBox XML and extract genre/players per system.

    Returns: {system_folder: {normalized_title: {"genre": str, "players": int, "name": str}}}
    """
    data = defaultdict(dict)

    for event, elem in ET.iterparse(str(path), events=("end",)):
        if elem.tag != "Game":
            continue

        platform = elem.findtext("Platform", "")
        system = platforms.get(platform)
        if not system:
            elem.clear()
            continue

        name = elem.findtext("Name", "").strip()
        if not name:
            elem.clear()
            continue

        genre = elem.findtext("Genres", "").strip()
        maxp_str = elem.findtext("MaxPlayers", "").strip()
        maxp = 0
        if maxp_str:
            try:
                maxp = int(maxp_str)
            except ValueError:
                pass

        norm = normalize_title(name)
        if norm and norm not in data[system]:
            data[system][norm] = {
                "genre": genre,
                "players": maxp,
                "name": name,
            }

        elem.clear()

    return dict(data)


def scan_roms(roms_dir, systems):
    """Scan ROM directories to get list of actual ROMs on disk.

    Returns: {system_folder: [normalized_title, ...]}
    """
    result = {}
    for system in systems:
        sys_dir = roms_dir / system
        if not sys_dir.exists():
            result[system] = []
            continue

        titles = set()
        for root, dirs, files in os.walk(sys_dir):
            # Skip hidden/special directories
            dirs[:] = [d for d in dirs if not d.startswith("_")]
            for f in files:
                norm = normalize_title(f)
                if norm:
                    titles.add(norm)

        result[system] = sorted(titles)

    return result


def main():
    print("=" * 80)
    print("GENRE COMPARISON: Baked-in game_db vs LaunchBox")
    print("=" * 80)
    print()

    # 1. Parse the baked-in game_db.rs
    print("Parsing game_db.rs...")
    game_db = parse_game_db_rs(GAME_DB_RS)
    for sys, games in sorted(game_db.items()):
        has_genre = sum(1 for g in games.values() if g["genre"])
        print(f"  {sys}: {len(games)} entries, {has_genre} with genre")
    print()

    # 2. Parse LaunchBox XML
    print("Parsing LaunchBox XML (this may take a minute)...")
    lb_data = parse_launchbox_xml(LAUNCHBOX_XML, LB_PLATFORM_MAP)
    for sys, games in sorted(lb_data.items()):
        has_genre = sum(1 for g in games.values() if g["genre"])
        print(f"  {sys}: {len(games)} entries, {has_genre} with genre")
    print()

    # 3. Scan ROMs on disk
    print("Scanning ROMs on disk...")
    rom_titles = scan_roms(NFS_ROMS, SYSTEMS.keys())
    for sys, titles in sorted(rom_titles.items()):
        print(f"  {sys}: {len(titles)} ROMs")
    print()

    # 4. Compare per system
    print("=" * 80)
    print("PER-SYSTEM GENRE COMPARISON")
    print("=" * 80)
    print()

    overall = {
        "total_roms": 0,
        "gamedb_covered": 0,
        "lb_covered": 0,
        "both_covered": 0,
        "agree": 0,
        "disagree": 0,
    }
    all_disagreements = []

    for system in sorted(SYSTEMS.keys()):
        titles = rom_titles.get(system, [])
        if not titles:
            print(f"--- {system}: NO ROMs on disk, skipping ---")
            print()
            continue

        gdb = game_db.get(system, {})
        lb = lb_data.get(system, {})

        gdb_covered = 0
        lb_covered = 0
        both_covered = 0
        agree = 0
        disagree = 0
        disagreements = []

        for title in titles:
            gdb_entry = gdb.get(title)
            lb_entry = lb.get(title)

            gdb_genre = norm_genre(gdb_entry["genre"]) if gdb_entry and gdb_entry["genre"] else ""
            lb_genre = norm_genre(lb_entry["genre"]) if lb_entry and lb_entry["genre"] else ""

            if gdb_genre:
                gdb_covered += 1
            if lb_genre:
                lb_covered += 1
            if gdb_genre and lb_genre:
                both_covered += 1
                if gdb_genre == lb_genre:
                    agree += 1
                else:
                    disagree += 1
                    display = gdb_entry["display_name"] if gdb_entry else title
                    disagreements.append({
                        "title": display,
                        "norm": title,
                        "gamedb": gdb_entry["genre"] if gdb_entry else "",
                        "gamedb_norm": gdb_genre,
                        "lb": lb_entry["genre"] if lb_entry else "",
                        "lb_norm": lb_genre,
                    })

        total = len(titles)
        print(f"--- {system} ({total} ROMs) ---")
        print(f"  game_db coverage: {gdb_covered}/{total} ({100*gdb_covered//max(1,total)}%)")
        print(f"  LaunchBox coverage: {lb_covered}/{total} ({100*lb_covered//max(1,total)}%)")
        print(f"  Both have genre: {both_covered}")
        if both_covered > 0:
            print(f"  Agree: {agree} ({100*agree//max(1,both_covered)}%)")
            print(f"  Disagree: {disagree} ({100*disagree//max(1,both_covered)}%)")

        if disagreements:
            print(f"  Sample disagreements (first 10):")
            for d in disagreements[:10]:
                print(f"    {d['title']}: gamedb={d['gamedb']} -> {d['gamedb_norm']}, LB={d['lb']} -> {d['lb_norm']}")

        print()

        overall["total_roms"] += total
        overall["gamedb_covered"] += gdb_covered
        overall["lb_covered"] += lb_covered
        overall["both_covered"] += both_covered
        overall["agree"] += agree
        overall["disagree"] += disagree
        all_disagreements.extend([(system, d) for d in disagreements])

    # 5. Overall summary
    print("=" * 80)
    print("OVERALL SUMMARY")
    print("=" * 80)
    t = overall["total_roms"]
    print(f"  Total ROMs on disk: {t}")
    print(f"  game_db genre coverage: {overall['gamedb_covered']}/{t} ({100*overall['gamedb_covered']//max(1,t)}%)")
    print(f"  LaunchBox genre coverage: {overall['lb_covered']}/{t} ({100*overall['lb_covered']//max(1,t)}%)")
    print(f"  Both have genre: {overall['both_covered']}")
    bc = overall["both_covered"]
    if bc > 0:
        print(f"  Agreement rate: {overall['agree']}/{bc} ({100*overall['agree']//max(1,bc)}%)")
        print(f"  Disagreement rate: {overall['disagree']}/{bc} ({100*overall['disagree']//max(1,bc)}%)")
    print()

    # 6. LaunchBox-only coverage (ROMs that game_db misses but LB has)
    print("=" * 80)
    print("LaunchBox FILLS GAPS (has genre where game_db doesn't)")
    print("=" * 80)
    for system in sorted(SYSTEMS.keys()):
        titles = rom_titles.get(system, [])
        if not titles:
            continue
        gdb = game_db.get(system, {})
        lb = lb_data.get(system, {})
        fills = 0
        for title in titles:
            gdb_entry = gdb.get(title)
            lb_entry = lb.get(title)
            gdb_genre = norm_genre(gdb_entry["genre"]) if gdb_entry and gdb_entry["genre"] else ""
            lb_genre = norm_genre(lb_entry["genre"]) if lb_entry and lb_entry["genre"] else ""
            if not gdb_genre and lb_genre:
                fills += 1
        total = len(titles)
        gdb_covered = sum(1 for t in titles if (gdb.get(t) and norm_genre(gdb.get(t, {}).get("genre", ""))))
        gap = total - gdb_covered
        print(f"  {system}: {fills}/{gap} gaps filled by LB ({100*fills//max(1,gap)}%)")
    print()

    # 7. Full disagreements list
    print("=" * 80)
    print(f"ALL DISAGREEMENTS ({len(all_disagreements)} total)")
    print("=" * 80)
    for system, d in all_disagreements:
        print(f"  [{system}] {d['title']}: gamedb={d['gamedb']}({d['gamedb_norm']}) vs LB={d['lb']}({d['lb_norm']})")

    return overall, all_disagreements


if __name__ == "__main__":
    overall, disagreements = main()
