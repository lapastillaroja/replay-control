#!/usr/bin/env python3
"""Sync the bundled arcade_dc catalog with Flycast's Naomi game table.

Usage: python3 scripts/sync_flycast_games.py [--source URL-or-path] [--check]

Flycast's Game::bios field distinguishes Naomi, Naomi 2, Atomiswave, and
System SP. MAME's sourcefile alone cannot distinguish the Naomi boards.
Existing CSV rows retain supplemental year, manufacturer, status, and category
data; newly discovered rows use the bundled MAME extract where available.
"""

import argparse
import ast
import csv
import io
import re
import urllib.request
import xml.etree.ElementTree as ET
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
OUTPUT = ROOT / "data/arcade/flycast_games.csv"
MAME = ROOT / "data/upstream/mame0289-arcade.xml"
SOURCE = "https://raw.githubusercontent.com/flyinghead/flycast/master/core/hw/naomi/naomi_roms.cpp"
FIELDS = (
    "rom_name", "display_name", "year", "manufacturer", "players", "rotation",
    "status", "is_clone", "parent", "category", "board",
)
BIOS_BOARDS = {
    "naomi": "sega_naomi",
    "naomi-ko-h": "sega_naomi",
    "naomi-jp-d": "sega_naomi",
    "airlbios": "sega_naomi",
    "f355dlx": "sega_naomi",
    "f355bios": "sega_naomi",
    "hod2bios": "sega_naomi",
    "naomi2": "sega_naomi_2",
    "awbios": "sammy_atomiswave",
    "segasp": "sega_system_sp",
}
COMMENT_OR_STRING = re.compile(r'"(?:\\.|[^"\\])*"|//[^\n]*|/\*.*?\*/', re.DOTALL)


def strip_comments(source):
    return COMMENT_OR_STRING.sub(
        lambda match: match.group() if match.group().startswith('"') else " ", source
    )


def brace_blocks(source):
    """Yield direct child initializers of `const Game Games[]`."""
    match = re.search(r"\bconst\s+Game\s+Games\s*\[\s*\]\s*=\s*\{", source)
    if not match:
        raise ValueError("Flycast Game table not found")
    depth = 1
    start = None
    index = match.end()
    while index < len(source) and depth:
        if source[index] == '"':
            index += 1
            while index < len(source):
                if source[index] == "\\":
                    index += 2
                elif source[index] == '"':
                    break
                else:
                    index += 1
        elif source[index] == "{":
            depth += 1
            if depth == 2:
                start = index + 1
        elif source[index] == "}":
            depth -= 1
            if depth == 1 and start is not None:
                yield source[start:index]
                start = None
        index += 1
    if depth:
        raise ValueError("Flycast Game table has unbalanced braces")


def leading_fields(block):
    """Read scalar fields up to the nested ROM-blob initializer."""
    fields = []
    start = 0
    index = 0
    while index < len(block):
        char = block[index]
        if char == '"':
            index += 1
            while index < len(block):
                if block[index] == "\\":
                    index += 2
                elif block[index] == '"':
                    break
                else:
                    index += 1
        elif char == ",":
            fields.append(block[start:index].strip())
            start = index + 1
        elif char == "{":
            break
        index += 1
    if len(fields) != 8:
        raise ValueError(f"expected 8 leading Game fields, got {len(fields)}: {fields!r}")
    return fields


def quoted(value):
    if not value.startswith('"') or not value.endswith('"'):
        raise ValueError(f"expected C++ string literal, got {value!r}")
    return ast.literal_eval(value)


def parse_flycast(source, minimum_games=250):
    games = {}
    for block in brace_blocks(strip_comments(source)):
        if block.strip() == "nullptr":
            continue
        name, parent, description, _, _, bios, _, rotation = leading_fields(block)
        name = quoted(name)
        bios = quoted(bios)
        if bios not in BIOS_BOARDS:
            raise ValueError(f"unknown Flycast BIOS {bios!r} for {name}")
        if parent != "nullptr":
            parent = quoted(parent)
        else:
            parent = ""
        if rotation not in {"ROT0", "ROT270"}:
            raise ValueError(f"unknown Flycast rotation {rotation!r} for {name}")
        game = {
            "display_name": quoted(description),
            "parent": parent,
            "rotation": rotation.removeprefix("ROT"),
            "board": BIOS_BOARDS[bios],
        }
        if name in games and games[name] != game:
            raise ValueError(f"conflicting Flycast definitions for {name}")
        games[name] = game
    if len(games) < minimum_games:
        raise ValueError(f"Flycast table unexpectedly small: {len(games)} games")
    return games


def read_source(location):
    if location.startswith(("https://", "http://")):
        request = urllib.request.Request(location, headers={"User-Agent": "ReplayControl-Flycast-Sync/1.0"})
        with urllib.request.urlopen(request, timeout=30) as response:
            return response.read().decode("utf-8")
    return Path(location).read_text(encoding="utf-8")


def read_existing(path):
    with path.open(newline="", encoding="utf-8") as file:
        return {row["rom_name"]: row for row in csv.DictReader(file)}


def read_mame(path):
    result = {}
    for _, element in ET.iterparse(path, events=("end",)):
        if element.tag != "m":
            continue
        result[element.attrib["name"]] = {
            "display_name": element.findtext("d", ""),
            "year": element.findtext("y", ""),
            "manufacturer": element.findtext("f", ""),
            "players": element.attrib.get("players", "0"),
            "status": element.attrib.get("status", "unknown"),
        }
        element.clear()
    return result


def merge(games, existing, mame):
    rows = []
    for name in sorted(games):
        game = games[name]
        previous = existing.get(name, {})
        reference = mame.get(name, {})
        row = {
            "rom_name": name,
            "display_name": previous.get("display_name") or reference.get("display_name") or game["display_name"],
            "year": previous.get("year") or reference.get("year", ""),
            "manufacturer": previous.get("manufacturer") or reference.get("manufacturer", ""),
            "players": previous.get("players") or reference.get("players", "0"),
            "rotation": game["rotation"],
            "status": previous.get("status") or reference.get("status", "unknown"),
            "is_clone": str(bool(game["parent"])).lower(),
            "parent": game["parent"],
            "category": previous.get("category", ""),
            "board": game["board"],
        }
        rows.append(row)
    return rows


def render(rows):
    buffer = io.StringIO(newline="")
    writer = csv.DictWriter(buffer, fieldnames=FIELDS, lineterminator="\n")
    writer.writeheader()
    writer.writerows(rows)
    return buffer.getvalue()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", default=SOURCE, help="Flycast naomi_roms.cpp URL or local path")
    parser.add_argument("--check", action="store_true", help="report drift without updating the CSV")
    args = parser.parse_args()

    games = parse_flycast(read_source(args.source))
    existing = read_existing(OUTPUT)
    rows = merge(games, existing, read_mame(MAME))
    updated = render(rows)
    added = sorted(games.keys() - existing.keys())
    removed = sorted(existing.keys() - games.keys())
    changed = sum(existing.get(row["rom_name"]) != row for row in rows if row["rom_name"] in existing)
    print(f"Flycast: {len(games)} games; add {len(added)}, remove {len(removed)}, update {changed}")
    if added:
        print("Added:", ", ".join(added))
    if removed:
        print("Removed:", ", ".join(removed))
    if args.check:
        return 1 if OUTPUT.read_text(encoding="utf-8") != updated else 0
    OUTPUT.write_text(updated, encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
