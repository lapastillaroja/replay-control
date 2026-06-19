#!/usr/bin/env python3
"""Extract the RetroAchievements per-system game list into committed data files.

Mirrors the wikidata/shmups committed-data pattern (see
`scripts/wikidata-series-extract.py` and the refresh-*.yml workflows): fetch
facts from an upstream API, write them under the repo, and let `build-catalog`
ingest them at catalog-build time. See the RetroAchievements plan §2/§3.

What it writes
--------------
For each of OUR supported systems that RetroAchievements covers, this fetches
`API_GetGameList?f=1&h=1` (games WITH achievements, plus ROM hashes) and writes
`data/retroachievements/<system>.json` as a facts-only array:

    [ { "title": "...", "ra_id": "1234", "num_achievements": 48,
        "hashes": ["<ra_hash>", ...] }, ... ]

Every entry carries RA's `hashes` (the `ra_hash` values). `build-catalog` matches
them per realm: whole-file carts join `ra_hash == No-Intro md5`; header carts
(NES/SNES/N64) match the runtime-computed rc_hash; arcade matches
`md5(romset name)`. `title` is kept only as a fallback. Achievement text and
badges are NEVER bundled — only these facts (plan §4).

Why resolve IDs from the API
----------------------------
RA `ConsoleID`s are easy to get wrong from memory, so instead of hardcoding
numbers we map each of our system names to RA's console NAME and resolve the
numeric id from `API_GetConsoleIDs` at runtime. An unresolved name is reported
(and skipped); any HTTP/JSON failure is fatal — we never write a partial or
empty file silently.

Usage
-----
    # key from scripts/.env (RETROACHIEVEMENTS_KEY=...), or the environment:
    python3 scripts/retroachievements-gamelist-extract.py [--out data/retroachievements] [--only sega_smd,nintendo_snes]

The key is read from `RETROACHIEVEMENTS_KEY`. On startup the script loads
`scripts/.env` (next to this file) if present — see `scripts/.env.example` —
without overriding variables already set in the environment. The Web API key
comes from retroachievements.org/controlpanel ("Keys"); it is read-only and
distinct from the rcheevos username/password RePlayOS stores.
"""

import argparse
import json
import os
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

API_BASE = "https://retroachievements.org/API"

# Transient HTTP statuses worth retrying. RA returns 401 when it throttles a key
# (not only for bad auth), so it's treated as transient alongside 429/5xx.
RETRYABLE_STATUS = {401, 429, 500, 502, 503, 504}
MAX_ATTEMPTS = 5

# Our system name -> the RetroAchievements console NAME(s) it maps to. Matched
# case-insensitively as a substring against API_GetConsoleIDs names, so we don't
# depend on exact RA spelling or hardcoded numeric ids.
#
# Scope: the systems the catalog actually builds metadata for — the same set
# `scripts/download-metadata.sh` fetches No-Intro DATs for — plus a single
# `arcade` entry. The catalog has no `canonical_game` rows for disc systems
# (PSX, PS2, DS, Saturn, Dreamcast, …), Atari, NEC, SNK, or 3DO, so RA data for
# those could never be ingested and is deliberately not fetched. The catalog's
# `commodore_ami` is also omitted: RA has no Amiga console (it exposes Amstrad
# CPC, which we don't catalog).
#
# Arcade is matched differently from the console systems. RA's "Arcade" console
# identifies games by `md5(lowercase romset_name)` (rc_hash hashes the ROM set
# NAME, not bytes), so for `arcade` we keep RA's `hashes` list in the output and
# `build-catalog` matches it against `md5(arcade_game.rom_name)` — far more
# reliable than title-matching RA's hack-heavy display names. The three arcade
# romset sources (FBNeo / MAME / MAME 2003-plus) all map to this one console, so
# we fetch it once.
SYSTEM_TO_RA_CONSOLE = {
    "nintendo_nes": ["NES/Famicom", "NES"],
    "nintendo_snes": ["SNES/Super Famicom", "SNES"],
    "nintendo_gb": ["Game Boy"],
    "nintendo_gbc": ["Game Boy Color"],
    "nintendo_gba": ["Game Boy Advance"],
    "nintendo_n64": ["Nintendo 64"],
    "sega_smd": ["Genesis/Mega Drive", "Mega Drive"],
    "sega_sms": ["Master System"],
    "sega_gg": ["Game Gear"],
    "sega_sg": ["SG-1000"],
    "sega_32x": ["32X"],
    "microsoft_msx": ["MSX"],
    "arcade": ["Arcade"],
    # Disc systems — RA matched at runtime by the boot-file rc_hash (the `ra_hash`
    # here), independent of disc identification/cataloguing. See plan §10.6.
    "sony_psx": ["PlayStation"],
    "sony_ps2": ["PlayStation 2"],
    "sony_psp": ["PlayStation Portable"],
    "sega_st": ["Saturn"],
    "sega_cd": ["Sega CD"],
    "sega_dc": ["Dreamcast"],
    "panasonic_3do": ["3DO Interactive Multiplayer"],
    "nec_pcecd": ["PC Engine CD/TurboGrafx-CD"],
    "snk_ngcd": ["Neo Geo CD"],
}

# All systems keep RA's per-game `hashes` list — it's the build input for
# hash-based matching across every realm: arcade is `md5(romset name)`, whole-file
# carts join `ra_hash == No-Intro md5`, and header carts (NES/SNES/N64) match the
# runtime-computed rc_hash against these. (Title matching is only a fallback now.)
HASH_MATCHED_SYSTEMS = set(SYSTEM_TO_RA_CONSOLE)


def _get(endpoint: str, params: dict) -> object:
    """GET an RA API endpoint as JSON.

    Retries transient failures (rate-limit / network / 5xx) with exponential
    backoff (2s, 4s, 8s, 16s). Fatal (SystemExit) on a clearly non-transient
    error or once retries are exhausted, so a run never silently writes partial
    data.
    """
    query = urllib.parse.urlencode(params)
    url = f"{API_BASE}/{endpoint}?{query}"
    req = urllib.request.Request(url, headers={"User-Agent": "replay-control-catalog"})
    last_err = ""
    for attempt in range(MAX_ATTEMPTS):
        if attempt:
            backoff = 2**attempt  # 2, 4, 8, 16 s
            print(
                f"  retry {attempt}/{MAX_ATTEMPTS - 1} for {endpoint} after {last_err}; "
                f"sleeping {backoff}s",
                file=sys.stderr,
            )
            time.sleep(backoff)
        try:
            with urllib.request.urlopen(req, timeout=60) as resp:
                body = resp.read().decode("utf-8")
        except urllib.error.HTTPError as e:
            last_err = f"HTTP {e.code}"
            if e.code in RETRYABLE_STATUS:
                continue
            raise SystemExit(f"FATAL: request to {endpoint} failed: {last_err}")
        except urllib.error.URLError as e:
            last_err = f"network error: {e.reason}"
            continue
        try:
            return json.loads(body)
        except json.JSONDecodeError as e:
            raise SystemExit(f"FATAL: {endpoint} returned non-JSON ({e}): {body[:200]}")
    raise SystemExit(
        f"FATAL: request to {endpoint} failed after {MAX_ATTEMPTS} attempts ({last_err})"
    )


def resolve_console_ids(auth: dict) -> dict:
    """Map each supported system -> RA console id via API_GetConsoleIDs.

    Matching is **exact** (case-insensitive) on the RA console name, trying each
    candidate in order. Substring matching is deliberately avoided: short
    aliases like `NES` are substrings of other consoles (`SNES/Super Famicom`),
    so a substring search could silently bind the wrong console and commit a bad
    per-system extract. An unresolved system is reported and skipped (fail-closed).
    """
    consoles = _get("API_GetConsoleIDs.php", {**auth, "g": 1, "a": 1})
    by_name = {c["Name"].lower(): int(c["ID"]) for c in consoles if c.get("Name")}
    resolved = {}
    for system, names in SYSTEM_TO_RA_CONSOLE.items():
        cid = next(
            (by_name[name.lower()] for name in names if name.lower() in by_name),
            None,
        )
        if cid is None:
            print(f"  ! {system}: no RA console exactly matched {names} — skipping", file=sys.stderr)
        else:
            resolved[system] = cid
    return resolved


def fetch_system(auth: dict, console_id: int, include_hashes: bool = False) -> list:
    """All games WITH achievements for one console (facts only).

    When `include_hashes` is set, each entry also carries RA's `hashes` list
    (lowercased, deduped, sorted) — needed for arcade, where matching is by
    `md5(romset_name)` rather than by title.
    """
    games = _get(
        "API_GetGameList.php",
        {**auth, "i": console_id, "f": 1, "h": 1, "c": 0},
    )
    out = []
    for g in games:
        title = (g.get("Title") or "").strip()
        ra_id = g.get("ID")
        if not title or ra_id is None:
            continue
        entry = {
            "title": title,
            "ra_id": str(ra_id),
            "num_achievements": int(g.get("NumAchievements") or 0),
        }
        if include_hashes:
            entry["hashes"] = sorted({h.lower() for h in (g.get("Hashes") or []) if h})
        out.append(entry)
    return out


def load_dotenv(path: str) -> None:
    """Load `KEY=VALUE` lines from a `.env` file into the environment.

    No-op when the file is absent. Existing environment variables win (an
    exported var is never overridden), matching standard dotenv behaviour, so
    `RETROACHIEVEMENTS_KEY=… python3 …` still takes precedence. Minimal parser:
    skips blanks/comments, tolerates a leading `export`, strips surrounding
    quotes. No interpolation. Never logs values.
    """
    try:
        with open(path, encoding="utf-8") as fh:
            lines = fh.readlines()
    except FileNotFoundError:
        return
    for raw in lines:
        line = raw.strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        if line.startswith("export "):
            line = line[len("export ") :]
        name, _, value = line.partition("=")
        name = name.strip()
        value = value.strip().strip('"').strip("'")
        if name:
            os.environ.setdefault(name, value)


def main() -> None:
    parser = argparse.ArgumentParser(description="Fetch RetroAchievements game lists for our systems.")
    parser.add_argument("--out", default="data/retroachievements", help="Output directory")
    parser.add_argument("--only", default="", help="Comma-separated subset of systems to fetch")
    parser.add_argument("--delay", type=float, default=2.0, help="Seconds between requests (rate-limit courtesy)")
    args = parser.parse_args()

    # Load scripts/.env (next to this file) without overriding the environment.
    load_dotenv(os.path.join(os.path.dirname(os.path.abspath(__file__)), ".env"))

    key = os.environ.get("RETROACHIEVEMENTS_KEY")
    if not key:
        raise SystemExit(
            "FATAL: set RETROACHIEVEMENTS_KEY (in scripts/.env or the environment) "
            "— get a read-only Web API key at retroachievements.org/controlpanel -> Keys"
        )
    auth = {"y": key}
    if os.environ.get("RA_USERNAME"):
        auth["z"] = os.environ["RA_USERNAME"]

    only = {f.strip() for f in args.only.split(",") if f.strip()}
    os.makedirs(args.out, exist_ok=True)

    print("Resolving RA console ids…", file=sys.stderr)
    resolved = resolve_console_ids(auth)

    total = 0
    written = 0
    for system, console_id in sorted(resolved.items()):
        if only and system not in only:
            continue
        time.sleep(args.delay)
        entries = fetch_system(
            auth, console_id, include_hashes=system in HASH_MATCHED_SYSTEMS
        )
        path = os.path.join(args.out, f"{system}.json")
        if not entries:
            # RA lists the console but no games carry achievements yet (e.g.
            # Amiga). Don't write an empty file — the build no-ops without one.
            print(f"  {system} (console {console_id}): 0 games — skipping write", file=sys.stderr)
            continue
        with open(path, "w", encoding="utf-8") as fh:
            json.dump(entries, fh, ensure_ascii=False, indent=1, sort_keys=True)
            fh.write("\n")
        total += len(entries)
        written += 1
        print(f"  {system} (console {console_id}): {len(entries)} games -> {path}", file=sys.stderr)

    print(f"Done. {total} games across {written} systems.", file=sys.stderr)


if __name__ == "__main__":
    main()
