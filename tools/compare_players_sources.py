#!/usr/bin/env python3
"""Compare player count data between baked-in game_db and LaunchBox metadata.

For each ROM on disk (NFS), looks up:
  - What player count the baked-in game_db assigns (from generated game_db.rs)
  - What player count LaunchBox assigns (from Metadata.xml)
  - Whether they agree or disagree

Reports per-system stats and interesting disagreements.
"""

import os
import re
import sys
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


def normalize_title(name):
    """Normalize a ROM filename for matching."""
    stem = name
    if "." in stem:
        stem = stem[:stem.rfind(".")]

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

    if ", " in stripped:
        idx = stripped.rfind(", ")
        before = stripped[:idx]
        after_comma = stripped[idx + 2:]
        match = re.match(r"([A-Za-z]+)(.*)", after_comma)
        if match:
            first_word = match.group(1)
            rest = match.group(2).lstrip(" -")
            if first_word.lower() in ("the", "a", "an"):
                if rest:
                    stripped = f"{first_word} {before} {rest}"
                else:
                    stripped = f"{first_word} {before}"

    return "".join(ch.lower() for ch in stripped if ch.isalnum())


def parse_game_db_rs(path):
    """Parse game_db.rs for player count data."""
    data = {}
    prefix_to_system = {v: k for k, v in SYSTEMS.items()}

    with open(path, "r") as f:
        content = f.read()

    for prefix, system in prefix_to_system.items():
        pattern = rf'static {prefix}_GAMES: &\[CanonicalGame\] = &\[(.*?)\];'
        match = re.search(pattern, content, re.DOTALL)
        if not match:
            continue

        games = {}
        game_list = []
        block = match.group(1)

        for game_match in re.finditer(
            r'CanonicalGame \{[^}]*display_name: "([^"]*)".*?genre: "([^"]*)".*?players: (\d+).*?normalized_genre: "([^"]*)"',
            block,
        ):
            display_name = game_match.group(1).replace('\\"', '"').replace("\\\\", "\\")
            players = int(game_match.group(3))
            game_list.append({
                "display_name": display_name,
                "players": players,
            })

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
                        "players": game["players"],
                        "display_name": game["display_name"],
                        "filename_stem": filename_stem,
                    }

        data[system] = games

    return data


def parse_launchbox_xml(path, platforms):
    """Stream-parse LaunchBox XML for player count data."""
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
                "players": maxp,
                "name": name,
            }

        elem.clear()

    return dict(data)


def scan_roms(roms_dir, systems):
    """Scan ROM directories."""
    result = {}
    for system in systems:
        sys_dir = roms_dir / system
        if not sys_dir.exists():
            result[system] = []
            continue

        titles = set()
        for root, dirs, files in os.walk(sys_dir):
            dirs[:] = [d for d in dirs if not d.startswith("_")]
            for f in files:
                norm = normalize_title(f)
                if norm:
                    titles.add(norm)

        result[system] = sorted(titles)

    return result


def main():
    print("=" * 80)
    print("PLAYER COUNT COMPARISON: Baked-in game_db vs LaunchBox")
    print("=" * 80)
    print()

    print("Parsing game_db.rs...")
    game_db = parse_game_db_rs(GAME_DB_RS)
    for sys, games in sorted(game_db.items()):
        has_players = sum(1 for g in games.values() if g["players"] > 0)
        print(f"  {sys}: {len(games)} entries, {has_players} with players")
    print()

    print("Parsing LaunchBox XML...")
    lb_data = parse_launchbox_xml(LAUNCHBOX_XML, LB_PLATFORM_MAP)
    for sys, games in sorted(lb_data.items()):
        has_players = sum(1 for g in games.values() if g["players"] > 0)
        print(f"  {sys}: {len(games)} entries, {has_players} with players")
    print()

    print("Scanning ROMs on disk...")
    rom_titles = scan_roms(NFS_ROMS, SYSTEMS.keys())
    for sys, titles in sorted(rom_titles.items()):
        print(f"  {sys}: {len(titles)} ROMs")
    print()

    print("=" * 80)
    print("PER-SYSTEM PLAYER COUNT COMPARISON")
    print("=" * 80)
    print()

    overall = {
        "total_roms": 0,
        "gamedb_covered": 0,
        "lb_covered": 0,
        "both_covered": 0,
        "exact_match": 0,
        "close_match": 0,  # differ by 1
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
        exact_match = 0
        close_match = 0
        disagree = 0
        disagreements = []

        for title in titles:
            gdb_entry = gdb.get(title)
            lb_entry = lb.get(title)

            gdb_players = gdb_entry["players"] if gdb_entry else 0
            lb_players = lb_entry["players"] if lb_entry else 0

            if gdb_players > 0:
                gdb_covered += 1
            if lb_players > 0:
                lb_covered += 1
            if gdb_players > 0 and lb_players > 0:
                both_covered += 1
                if gdb_players == lb_players:
                    exact_match += 1
                elif abs(gdb_players - lb_players) == 1:
                    close_match += 1
                    display = gdb_entry["display_name"] if gdb_entry else title
                    disagreements.append({
                        "title": display,
                        "gamedb": gdb_players,
                        "lb": lb_players,
                        "diff": abs(gdb_players - lb_players),
                    })
                else:
                    disagree += 1
                    display = gdb_entry["display_name"] if gdb_entry else title
                    disagreements.append({
                        "title": display,
                        "gamedb": gdb_players,
                        "lb": lb_players,
                        "diff": abs(gdb_players - lb_players),
                    })

        total = len(titles)
        print(f"--- {system} ({total} ROMs) ---")
        print(f"  game_db coverage: {gdb_covered}/{total} ({100*gdb_covered//max(1,total)}%)")
        print(f"  LaunchBox coverage: {lb_covered}/{total} ({100*lb_covered//max(1,total)}%)")
        print(f"  Both have players: {both_covered}")
        if both_covered > 0:
            print(f"  Exact match: {exact_match} ({100*exact_match//max(1,both_covered)}%)")
            print(f"  Close match (off by 1): {close_match} ({100*close_match//max(1,both_covered)}%)")
            print(f"  Disagree (off by 2+): {disagree} ({100*disagree//max(1,both_covered)}%)")

        # Show big disagreements
        big_disagree = [d for d in disagreements if d["diff"] >= 2]
        if big_disagree:
            print(f"  Notable disagreements (diff >= 2, first 10):")
            for d in sorted(big_disagree, key=lambda x: -x["diff"])[:10]:
                print(f"    {d['title']}: gamedb={d['gamedb']}, LB={d['lb']} (diff={d['diff']})")

        print()

        overall["total_roms"] += total
        overall["gamedb_covered"] += gdb_covered
        overall["lb_covered"] += lb_covered
        overall["both_covered"] += both_covered
        overall["exact_match"] += exact_match
        overall["close_match"] += close_match
        overall["disagree"] += disagree
        all_disagreements.extend([(system, d) for d in disagreements])

    # Overall summary
    print("=" * 80)
    print("OVERALL SUMMARY")
    print("=" * 80)
    t = overall["total_roms"]
    print(f"  Total ROMs on disk: {t}")
    print(f"  game_db players coverage: {overall['gamedb_covered']}/{t} ({100*overall['gamedb_covered']//max(1,t)}%)")
    print(f"  LaunchBox players coverage: {overall['lb_covered']}/{t} ({100*overall['lb_covered']//max(1,t)}%)")
    bc = overall["both_covered"]
    if bc > 0:
        print(f"  Both have players: {bc}")
        print(f"  Exact match: {overall['exact_match']}/{bc} ({100*overall['exact_match']//max(1,bc)}%)")
        print(f"  Close match (off by 1): {overall['close_match']}/{bc} ({100*overall['close_match']//max(1,bc)}%)")
        print(f"  Disagree (off by 2+): {overall['disagree']}/{bc} ({100*overall['disagree']//max(1,bc)}%)")
    print()

    # Gap-filling analysis
    print("=" * 80)
    print("LaunchBox FILLS GAPS (has players where game_db doesn't)")
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
            gdb_p = gdb_entry["players"] if gdb_entry else 0
            lb_p = lb_entry["players"] if lb_entry else 0
            if gdb_p == 0 and lb_p > 0:
                fills += 1
        gdb_covered = sum(1 for t in titles if (gdb.get(t) and gdb.get(t, {}).get("players", 0) > 0))
        gap = len(titles) - gdb_covered
        print(f"  {system}: {fills}/{gap} gaps filled by LB ({100*fills//max(1,gap)}%)")
    print()

    # All disagreements with diff >= 2
    big = [(s, d) for s, d in all_disagreements if d["diff"] >= 2]
    print("=" * 80)
    print(f"ALL NOTABLE DISAGREEMENTS (diff >= 2): {len(big)} total")
    print("=" * 80)
    for system, d in sorted(big, key=lambda x: -x[1]["diff"]):
        print(f"  [{system}] {d['title']}: gamedb={d['gamedb']}, LB={d['lb']} (diff={d['diff']})")

    return overall, all_disagreements


if __name__ == "__main__":
    main()
