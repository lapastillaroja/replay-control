#!/usr/bin/env python3
"""
Extract video game series data from Wikidata for RePlayOS supported platforms.

Uses a broad SPARQL query (all video games with P179) without platform filtering,
then post-filters to keep only:
  1. Games with a recognized platform QID (console games)
  2. Games whose title fuzzy-matches a known arcade game (from FBNeo/MAME DAT files)

Queries the Wikidata SPARQL endpoint for:
- P179 (part of the series) + P1545 (series ordinal)
- P155 (follows) / P156 (followed by) for sequel/prequel chains
- P400 (platform) — fetched but not used as a filter

Outputs JSON to stdout: array of objects with game_title, series_name,
series_order, system, follows, followed_by fields.

Usage:
    python3 scripts/wikidata-series-extract.py > data/wikidata/series.json
"""

import csv
import json
import os
import re
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
import xml.etree.ElementTree as ET

SPARQL_ENDPOINT = "https://query.wikidata.org/sparql"
USER_AGENT = "ReplayControl-SeriesExtract/1.0"

# Wikidata QID -> RePlayOS system folder name
PLATFORM_MAP = {
    # Nintendo
    "Q172742": "nintendo_nes",      # NES
    "Q491640": "nintendo_nes",      # Family Computer (Famicom)
    "Q135321": "nintendo_nes",      # Famicom Disk System
    "Q183259": "nintendo_snes",     # SNES / Super Famicom
    "Q186437": "nintendo_gb",       # Game Boy
    "Q203992": "nintendo_gbc",      # Game Boy Color
    "Q188642": "nintendo_gba",      # Game Boy Advance
    "Q184839": "nintendo_n64",      # Nintendo 64
    "Q170323": "nintendo_ds",       # Nintendo DS
    # Sega
    "Q10676": "sega_smd",           # Mega Drive / Genesis
    "Q209868": "sega_sms",          # Master System
    "Q751719": "sega_gg",           # Game Gear
    "Q200912": "sega_saturn",       # Saturn
    "Q184198": "sega_dc",           # Dreamcast
    "Q1047516": "sega_cd",          # Sega CD / Mega-CD
    "Q1063978": "sega_32x",         # Sega 32X
    # Sony
    "Q10677": "sony_psx",           # PlayStation
    # NEC
    "Q1057377": "nec_pce",          # TurboGrafx-16 / PC Engine
    "Q10854461": "nec_pcecd",       # TurboGrafx-CD / PC Engine CD
    # Atari
    "Q206261": "atari_2600",        # Atari 2600
    "Q743222": "atari_5200",        # Atari 5200
    "Q753600": "atari_7800",        # Atari 7800
    "Q650601": "atari_jaguar",      # Atari Jaguar
    "Q753657": "atari_lynx",        # Atari Lynx
    # Other consoles
    "Q229429": "panasonic_3do",     # 3DO Interactive Multiplayer
    "Q1023103": "philips_cdi",      # Philips CD-i
    "Q853547": "microsoft_msx",     # MSX
    "Q11232203": "microsoft_msx",   # MSX2
    # Arcade platforms — all map to arcade_fbneo (primary arcade emulator;
    # series data is shared across all arcade system variants)
    "Q631229": "arcade_fbneo",      # Arcade game (generic)
    "Q210167": "arcade_fbneo",      # Arcade video game
    "Q192851": "arcade_fbneo",      # Arcade video game machine (coin-operated)
    "Q1136498": "arcade_fbneo",     # Arcade system board (generic)
    "Q1034233": "arcade_fbneo",     # CP System (CPS-1)
    "Q2981666": "arcade_fbneo",     # CP System II (CPS-2)
    "Q2634041": "arcade_fbneo",     # CP System III (CPS-3)
    "Q76098": "arcade_fbneo",       # Neo Geo AES (home)
    "Q1054350": "arcade_fbneo",     # Neo Geo (general)
    "Q3338058": "arcade_fbneo",     # Neo Geo MVS (arcade)
    "Q64428080": "arcade_fbneo",    # Neo Geo AES (alternate QID)
    "Q2703883": "arcade_fbneo",     # Neo Geo CD
    "Q1369174": "arcade_fbneo",     # Sega NAOMI
    "Q843916": "arcade_fbneo",      # Sega NAOMI 2
    "Q4386178": "arcade_fbneo",     # Sega Model 2
    "Q3142301": "arcade_fbneo",     # Sega Model 3
    "Q1067380": "arcade_fbneo",     # Sega Titan Video (ST-V)
}


def qid_from_uri(uri):
    """Extract QID from a Wikidata entity URI."""
    if uri and "/" in uri:
        return uri.rsplit("/", 1)[-1]
    return uri


def sparql_query(query, retries=5, backoff=10):
    """Execute a SPARQL query against the Wikidata endpoint with retry logic."""
    params = urllib.parse.urlencode({
        "query": query,
        "format": "json",
    })
    url = f"{SPARQL_ENDPOINT}?{params}"
    headers = {
        "User-Agent": USER_AGENT,
        "Accept": "application/sparql-results+json",
    }
    req = urllib.request.Request(url, headers=headers)

    for attempt in range(retries):
        try:
            with urllib.request.urlopen(req, timeout=300) as resp:
                return json.loads(resp.read().decode("utf-8"))
        except urllib.error.HTTPError as e:
            if e.code == 429 or e.code >= 500:
                wait = backoff * (2 ** attempt)
                print(f"SPARQL error {e.code}, retrying in {wait}s (attempt {attempt + 1}/{retries})...", file=sys.stderr)
                time.sleep(wait)
            else:
                raise
        except urllib.error.URLError as e:
            wait = backoff * (2 ** attempt)
            print(f"Network error: {e}, retrying in {wait}s (attempt {attempt + 1}/{retries})...", file=sys.stderr)
            time.sleep(wait)

    raise RuntimeError(f"SPARQL query failed after {retries} retries")


# ---------------------------------------------------------------------------
# Arcade game name loading
# ---------------------------------------------------------------------------

def fuzzy_key(title):
    """Normalize a title for fuzzy matching: lowercase, strip parens/brackets,
    remove non-alphanumeric (except spaces), collapse whitespace."""
    # Strip parenthesized/bracketed content
    title = re.sub(r'\([^)]*\)', '', title)
    title = re.sub(r'\[[^\]]*\]', '', title)
    title = title.strip()
    # Keep only alphanumeric + spaces, lowercase
    result = []
    for ch in title:
        if ch.isalnum() or ch == ' ':
            result.append(ch.lower())
    return ' '.join(''.join(result).split())


def load_arcade_names(script_dir):
    """Load display names from all arcade data sources.
    Returns a set of fuzzy-normalized display names."""
    data_dir = os.path.join(script_dir, '..', 'data')
    core_data_dir = os.path.join(script_dir, '..', 'replay-control-core', 'data', 'arcade')
    names = set()

    # 1. FBNeo DAT (ClrMame Pro XML)
    fbneo_path = os.path.join(data_dir, 'fbneo-arcade.dat')
    if os.path.exists(fbneo_path):
        count = 0
        for event, elem in ET.iterparse(fbneo_path, events=['end']):
            if elem.tag == 'game':
                desc = elem.find('description')
                if desc is not None and desc.text:
                    names.add(fuzzy_key(desc.text))
                    count += 1
                elem.clear()
        print(f"  Arcade names: FBNeo DAT loaded {count} names", file=sys.stderr)

    # 2. MAME 2003+ XML
    mame2003_path = os.path.join(data_dir, 'mame2003plus.xml')
    if os.path.exists(mame2003_path):
        count = 0
        for event, elem in ET.iterparse(mame2003_path, events=['end']):
            if elem.tag == 'game':
                desc = elem.find('description')
                if desc is not None and desc.text:
                    names.add(fuzzy_key(desc.text))
                    count += 1
                elem.clear()
        print(f"  Arcade names: MAME 2003+ loaded {count} names", file=sys.stderr)

    # 3. MAME 0.285 compact XML (uses <m> with <d> child)
    mame285_path = os.path.join(data_dir, 'mame0285-arcade.xml')
    if os.path.exists(mame285_path):
        count = 0
        for event, elem in ET.iterparse(mame285_path, events=['end']):
            if elem.tag == 'm':
                desc = elem.find('d')
                if desc is not None and desc.text:
                    names.add(fuzzy_key(desc.text))
                    count += 1
                elem.clear()
        print(f"  Arcade names: MAME 0.285 loaded {count} names", file=sys.stderr)

    # 4. Flycast CSV (Naomi/Atomiswave)
    flycast_path = os.path.join(core_data_dir, 'flycast_games.csv')
    if os.path.exists(flycast_path):
        count = 0
        with open(flycast_path, 'r', encoding='utf-8') as f:
            reader = csv.DictReader(f)
            for row in reader:
                display_name = row.get('display_name', '')
                if display_name:
                    names.add(fuzzy_key(display_name))
                    count += 1
        print(f"  Arcade names: Flycast CSV loaded {count} names", file=sys.stderr)

    # Remove empty string if it snuck in
    names.discard('')

    print(f"  Arcade names: {len(names)} unique normalized names total", file=sys.stderr)
    return names


# ---------------------------------------------------------------------------
# SPARQL queries — broad (no platform filter)
# ---------------------------------------------------------------------------

def fetch_series_data():
    """Fetch P179 series data with P1545 ordinals for ALL video games.
    Platform is fetched optionally but not used as a filter."""

    query = """
SELECT DISTINCT
  ?game
  ?gameLabel
  ?platform
  ?series
  ?seriesLabel
  ?ordinal
WHERE {
  ?game wdt:P31 wd:Q7889 .
  ?game wdt:P179 ?series .

  OPTIONAL { ?game wdt:P400 ?platform . }

  OPTIONAL {
    ?game p:P179 ?seriesStmt .
    ?seriesStmt ps:P179 ?series .
    ?seriesStmt pq:P1545 ?ordinal .
  }

  SERVICE wikibase:label {
    bd:serviceParam wikibase:language "en,mul" .
  }
}
ORDER BY ?seriesLabel ?ordinal ?gameLabel
"""
    print("Fetching series data (P179 + P1545, all platforms)...", file=sys.stderr)
    result = sparql_query(query)
    return result["results"]["bindings"]


def fetch_sequel_data():
    """Fetch P155/P156 sequel/prequel chain data for ALL video games.
    Platform is fetched optionally but not used as a filter."""

    query = """
SELECT DISTINCT
  ?game
  ?gameLabel
  ?platform
  ?follows
  ?followsLabel
  ?followedBy
  ?followedByLabel
WHERE {
  ?game wdt:P31 wd:Q7889 .

  OPTIONAL { ?game wdt:P400 ?platform . }

  {
    ?game wdt:P155 ?follows .
    OPTIONAL { ?game wdt:P156 ?followedBy . }
  }
  UNION
  {
    ?game wdt:P156 ?followedBy .
    OPTIONAL { ?game wdt:P155 ?follows . }
  }

  SERVICE wikibase:label {
    bd:serviceParam wikibase:language "en,mul" .
  }
}
ORDER BY ?gameLabel
"""
    print("Fetching sequel/prequel data (P155/P156, all platforms)...", file=sys.stderr)
    result = sparql_query(query)
    return result["results"]["bindings"]


# ---------------------------------------------------------------------------
# Post-query filtering
# ---------------------------------------------------------------------------

def classify_row(row, arcade_names):
    """Determine the system for a SPARQL result row.

    Returns the system string (e.g. 'nintendo_nes', 'arcade_fbneo') or None if
    the game should be discarded.

    Logic:
      1. If there's a platform QID that maps to a known system -> use that system.
      2. If no recognized platform, check if the game title fuzzy-matches a known
         arcade display name -> 'arcade_fbneo'.
      3. Otherwise -> None (discard).
    """
    game_title = row.get("gameLabel", {}).get("value", "")

    # Check platform QID first
    if "platform" in row:
        platform_qid = qid_from_uri(row["platform"]["value"])
        system = PLATFORM_MAP.get(platform_qid)
        if system:
            return system

    # No recognized platform — try arcade title matching
    title_key = fuzzy_key(game_title)
    if title_key and title_key in arcade_names:
        return "arcade_fbneo"

    return None


def main():
    script_dir = os.path.dirname(os.path.abspath(__file__))

    # Load arcade display names for fuzzy matching
    print("Loading arcade game names...", file=sys.stderr)
    arcade_names = load_arcade_names(script_dir)

    # Fetch series data
    series_rows = fetch_series_data()
    print(f"  Series query returned {len(series_rows)} rows", file=sys.stderr)

    # Small delay to be polite to the endpoint
    time.sleep(3)

    # Fetch sequel/prequel data
    sequel_rows = fetch_sequel_data()
    print(f"  Sequel query returned {len(sequel_rows)} rows", file=sys.stderr)

    # Build sequel lookup: game_qid -> {follows_label, followed_by_label}
    sequel_map = {}
    for row in sequel_rows:
        game_qid = qid_from_uri(row["game"]["value"])
        if game_qid not in sequel_map:
            sequel_map[game_qid] = {"follows": None, "followed_by": None}
        if "follows" in row and "followsLabel" in row:
            sequel_map[game_qid]["follows"] = row["followsLabel"]["value"]
        if "followedBy" in row and "followedByLabel" in row:
            sequel_map[game_qid]["followed_by"] = row["followedByLabel"]["value"]

    # Statistics counters
    stats = {
        "total_series_rows": len(series_rows),
        "total_sequel_rows": len(sequel_rows),
        "console_matches": 0,
        "arcade_matches": 0,
        "discarded": 0,
    }

    # Process series rows into output entries
    entries = []
    seen = set()  # Deduplicate (game_qid, system, series_qid)

    for row in series_rows:
        game_qid = qid_from_uri(row["game"]["value"])
        series_qid = qid_from_uri(row["series"]["value"])

        system = classify_row(row, arcade_names)
        if not system:
            stats["discarded"] += 1
            continue

        # Deduplicate by (game_qid, system, series_qid)
        dedup_key = (game_qid, system, series_qid)
        if dedup_key in seen:
            continue
        seen.add(dedup_key)

        game_title = row.get("gameLabel", {}).get("value", "")
        series_name = row.get("seriesLabel", {}).get("value", "")

        # Skip entries where the label is just the QID (unlabeled)
        if game_title.startswith("Q") and game_title[1:].isdigit():
            continue
        if series_name.startswith("Q") and series_name[1:].isdigit():
            continue

        ordinal = None
        if "ordinal" in row:
            try:
                ordinal = int(row["ordinal"]["value"])
            except (ValueError, TypeError):
                pass

        # Get sequel/prequel info
        sequel_info = sequel_map.get(game_qid, {})

        entry = {
            "game_title": game_title,
            "series_name": series_name,
            "system": system,
        }
        if ordinal is not None:
            entry["series_order"] = ordinal
        if sequel_info.get("follows"):
            entry["follows"] = sequel_info["follows"]
        if sequel_info.get("followed_by"):
            entry["followed_by"] = sequel_info["followed_by"]

        entries.append(entry)

        if system == "arcade_fbneo":
            stats["arcade_matches"] += 1
        else:
            stats["console_matches"] += 1

    # Also add sequel-only entries (games with P155/P156 but no P179 series)
    series_game_qids = {qid_from_uri(r["game"]["value"]) for r in series_rows}
    for row in sequel_rows:
        game_qid = qid_from_uri(row["game"]["value"])

        if game_qid in series_game_qids:
            continue  # Already covered by series data

        system = classify_row(row, arcade_names)
        if not system:
            stats["discarded"] += 1
            continue

        game_title = row.get("gameLabel", {}).get("value", "")
        if game_title.startswith("Q") and game_title[1:].isdigit():
            continue

        dedup_key = (game_qid, system, "sequel_only")
        if dedup_key in seen:
            continue
        seen.add(dedup_key)

        entry = {
            "game_title": game_title,
            "system": system,
        }
        if "followsLabel" in row:
            entry["follows"] = row["followsLabel"]["value"]
        if "followedByLabel" in row:
            entry["followed_by"] = row["followedByLabel"]["value"]

        entries.append(entry)

        if system == "arcade_fbneo":
            stats["arcade_matches"] += 1
        else:
            stats["console_matches"] += 1

    # Sort for deterministic output
    entries.sort(key=lambda e: (
        e.get("series_name", ""),
        e.get("series_order", 999999),
        e["game_title"],
        e["system"],
    ))

    # Log statistics to stderr
    print(f"\n--- Statistics ---", file=sys.stderr)
    print(f"  SPARQL series rows:  {stats['total_series_rows']}", file=sys.stderr)
    print(f"  SPARQL sequel rows:  {stats['total_sequel_rows']}", file=sys.stderr)
    print(f"  Console matches:     {stats['console_matches']}", file=sys.stderr)
    print(f"  Arcade matches:      {stats['arcade_matches']}", file=sys.stderr)
    print(f"  Discarded:           {stats['discarded']}", file=sys.stderr)
    print(f"  Output entries:      {len(entries)}", file=sys.stderr)
    print(f"    With series:       {sum(1 for e in entries if 'series_name' in e)}", file=sys.stderr)
    print(f"    With ordinal:      {sum(1 for e in entries if 'series_order' in e)}", file=sys.stderr)
    print(f"    With sequel info:  {sum(1 for e in entries if 'follows' in e or 'followed_by' in e)}", file=sys.stderr)

    json.dump(entries, sys.stdout, indent=2, ensure_ascii=False)
    sys.stdout.write("\n")


if __name__ == "__main__":
    main()
