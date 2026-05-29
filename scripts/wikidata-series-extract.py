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
# User-Agent format per WDQS policy
# (https://meta.wikimedia.org/wiki/User-Agent_policy):
# "Tool/Version (contact)". The contact URL lets WDQS operators reach the
# project if this script ever misbehaves, and identifies us as a polite
# repeat client (compliant UAs are subject to a higher rate-limit tier).
USER_AGENT = (
    "ReplayControl-SeriesExtract/1.2 "
    "(+https://github.com/lapastillaroja/replay-control)"
)

# WDQS public-endpoint limits, from the official User Manual
# (https://www.mediawiki.org/wiki/Wikidata_Query_Service/User_Manual#Query_limits):
#   - "One client is allowed 60 seconds of processing time each 60 seconds"
#     (a token bucket bucketed by User-Agent + IP).
#   - "One client is allowed 30 error queries per minute."
#   - Per-query server-side timeout is 60 seconds.
#   - Exceeding either budget returns HTTP 429 with a Retry-After header;
#     ignoring 429 escalates to a longer ban, then to HTTP 403.
# We respect the processing budget with an adaptive courtesy delay (see
# `_courtesy_pause`): after each query we idle for as long as that query
# took, keeping the duty cycle at ~=50% so cumulative processing can never
# overrun the 60s-per-60s bucket. This is far more important than the
# request *count* — the budget is processing-seconds, not requests.
WDQS_QUERY_TIMEOUT_SECS = 60
# Cumulative wall-clock the run will spend honoring `Retry-After` before
# giving up. A *single* 429 Retry-After can be ~1000s on a busy/shared IP;
# honoring one is usually enough for the processing budget to refill, so we
# wait it out rather than fail (the earlier per-response cap was too eager and
# bailed on exactly that recoverable case). What we must NOT do is wait out
# an unbounded *series* of 1000s windows on a collectively-throttled IP — so
# the limit is on the running total, not any one response. Once the total
# would be exceeded we raise `WdqsThrottled` and let the IP cool down.
#
# Override via env (the CI job sets it to match its own timeout-minutes):
# default 1500s (25 min) comfortably rides out one ~1000s window while still
# fitting a 45-minute job with room for the fetch itself.
MAX_TOTAL_RETRY_WAIT_SECS = int(os.environ.get("WDQS_MAX_TOTAL_WAIT_SECS", "1500"))

# Running total of seconds slept on Retry-After across this whole run, checked
# against MAX_TOTAL_RETRY_WAIT_SECS before each honored wait.
_total_retry_wait_secs = 0.0

# Wall-clock seconds the previous successful query took, used by
# `_courtesy_pause` to size the next inter-query idle. Module-global so pacing
# is enforced centrally in `sparql_query` for *every* caller, not sprinkled at
# call sites.
_last_query_secs = 0.0

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
    "Q1067380": "arcade_stv",       # Sega Titan Video (ST-V)
}


def qid_from_uri(uri):
    """Extract QID from a Wikidata entity URI."""
    if uri and "/" in uri:
        return uri.rsplit("/", 1)[-1]
    return uri


class WdqsThrottled(Exception):
    """WDQS returned a Retry-After above `MAX_HONORED_RETRY_AFTER_SECS`,
    meaning the client is in the escalated ban tier. Not worth waiting out
    in-process; the caller should abort and let the IP cool down."""


class WdqsTimeout(Exception):
    """A query repeatedly hit the 60 s server-side timeout (manifesting as a
    truncated/HTML body that won't JSON-parse). Signals the query is too heavy
    and should be split, not blindly retried — see `fetch_series_data`."""


def _parse_retry_after(value):
    """Parse a Retry-After header value into seconds.

    WDQS sends `Retry-After` on 429 (delta-seconds form, per RFC 7231 §7.1.3).
    Returns `None` if the header is missing or unparseable so the caller can
    fall back to exponential backoff.
    """
    if value is None:
        return None
    value = value.strip()
    if value.isdigit():
        return int(value)
    # HTTP-date form: rare from WDQS but allowed by the spec.
    try:
        import email.utils
        parsed = email.utils.parsedate_to_datetime(value)
        if parsed is None:
            return None
        import datetime
        delta = (parsed - datetime.datetime.now(parsed.tzinfo)).total_seconds()
        return max(0, int(delta))
    except (TypeError, ValueError):
        return None


def _courtesy_pause():
    """Idle for as long as the previous successful query spent processing.

    WDQS grants "60 seconds of processing time each 60 seconds". Sleeping for
    the previous query's wall-clock keeps the duty cycle at ~=50% (one part
    work, one part idle), so cumulative processing can never overrun the
    bucket no matter how many chunks we issue. Wall-clock over-estimates true
    server processing time (it includes network), which only makes this more
    conservative — the safe direction. The first query pauses for nothing
    (`_last_query_secs` starts at 0)."""
    if _last_query_secs <= 0:
        return
    pause = min(_last_query_secs, WDQS_QUERY_TIMEOUT_SECS)
    print(f"  (courtesy pause {pause:.1f}s — staying under WDQS 60s/60s budget)", file=sys.stderr)
    time.sleep(pause)


def sparql_query(query, retries=5, backoff=10):
    """Execute a SPARQL query against the Wikidata endpoint.

    Enforces the WDQS processing budget centrally: every call first idles via
    `_courtesy_pause`, then issues the request, then records its wall-clock so
    the *next* call paces itself. Because all queries route through here, no
    caller has to remember to throttle.

    Error handling:
      - 429: honor the server's `Retry-After` header (the canonical wait
        window), even when it's large (~1000s) — one such wait usually lets
        the processing budget refill. We only give up once the *cumulative*
        Retry-After wait this run would exceed `MAX_TOTAL_RETRY_WAIT_SECS`,
        which means the IP is stuck (repeating long windows); then raise
        `WdqsThrottled`. Without a header, fall back to exponential backoff.
      - 5xx: transient; exponential backoff and retry.
      - Truncated/non-JSON body: usually the 60 s server timeout silently
        cutting the response. Retried a couple of times (a less-loaded shard
        may complete it), then raised as `WdqsTimeout` so the caller can split
        the query instead of hammering an oversized one against the error-rate
        budget (30 error queries/min).
    """
    global _last_query_secs, _total_retry_wait_secs

    # POST, not GET. The chunked series queries embed up to `chunk_size`
    # `wd:Q…` IRIs in a `VALUES` block; at chunk_size=600 that query string is
    # several KB and a GET (query in the URL) is rejected with HTTP 414 URI Too
    # Long. WDQS supports POST with the query in the body
    # (application/x-www-form-urlencoded), which has no URL-length ceiling — the
    # recommended transport for large queries. `data=` makes urllib use POST.
    body = urllib.parse.urlencode({
        "query": query,
        "format": "json",
    }).encode("utf-8")
    headers = {
        "User-Agent": USER_AGENT,
        "Accept": "application/sparql-results+json",
        "Content-Type": "application/x-www-form-urlencoded",
    }
    req = urllib.request.Request(SPARQL_ENDPOINT, data=body, headers=headers)

    _courtesy_pause()

    # Truncated responses rarely self-heal for an oversized query, so cap their
    # retries low to conserve the 30-errors/minute budget before giving up to
    # the caller's split logic.
    max_truncation_retries = min(2, retries)
    truncation_attempts = 0

    for attempt in range(retries):
        start = time.monotonic()
        try:
            with urllib.request.urlopen(req, timeout=300) as resp:
                data = json.loads(resp.read().decode("utf-8"))
                _last_query_secs = time.monotonic() - start
                return data
        except urllib.error.HTTPError as e:
            if e.code == 429:
                retry_after = _parse_retry_after(e.headers.get("Retry-After"))
                wait = retry_after if retry_after is not None else backoff * (2 ** attempt)
                source = "Retry-After" if retry_after is not None else "backoff"
                if _total_retry_wait_secs + wait > MAX_TOTAL_RETRY_WAIT_SECS:
                    raise WdqsThrottled(
                        f"WDQS 429: honoring this {wait}s wait would push the "
                        f"cumulative Retry-After wait to "
                        f"{_total_retry_wait_secs + wait:.0f}s, over the "
                        f"{MAX_TOTAL_RETRY_WAIT_SECS}s budget. The IP is stuck in "
                        f"a repeating throttle window; aborting to let it cool down. "
                        f"Re-run later (or raise WDQS_MAX_TOTAL_WAIT_SECS)."
                    )
                _total_retry_wait_secs += wait
                print(
                    f"SPARQL 429, retrying in {wait}s ({source}, attempt {attempt + 1}/{retries}; "
                    f"cumulative wait {_total_retry_wait_secs:.0f}/{MAX_TOTAL_RETRY_WAIT_SECS}s)...",
                    file=sys.stderr,
                )
                time.sleep(wait)
            elif e.code >= 500:
                wait = backoff * (2 ** attempt)
                print(f"SPARQL error {e.code}, retrying in {wait}s (attempt {attempt + 1}/{retries})...", file=sys.stderr)
                time.sleep(wait)
            else:
                raise
        except urllib.error.URLError as e:
            wait = backoff * (2 ** attempt)
            print(f"Network error: {e}, retrying in {wait}s (attempt {attempt + 1}/{retries})...", file=sys.stderr)
            time.sleep(wait)
        except json.JSONDecodeError as e:
            truncation_attempts += 1
            if truncation_attempts > max_truncation_retries:
                raise WdqsTimeout(
                    f"Query body truncated {truncation_attempts}x (likely WDQS 60s timeout): {e}"
                )
            wait = backoff * (2 ** attempt)
            print(f"Truncated SPARQL response (likely WDQS timeout): {e}, retrying in {wait}s (attempt {truncation_attempts}/{max_truncation_retries})...", file=sys.stderr)
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
    core_data_dir = os.path.join(script_dir, '..', 'data', 'arcade')
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

def fetch_distinct_series_qids():
    """Return the list of series QIDs that have at least one video game (P31=Q7889)
    attached via P179. This is intentionally a small, fast query — only the
    series URIs come back, not the full game list — so we can use the result
    as the chunk axis for `fetch_series_data`."""

    query = """
SELECT DISTINCT ?series WHERE {
  ?game wdt:P31 wd:Q7889 .
  ?game wdt:P179 ?series .
}
"""
    print("Fetching distinct series QIDs (chunk axis)...", file=sys.stderr)
    result = sparql_query(query)
    return [qid_from_uri(b["series"]["value"]) for b in result["results"]["bindings"]]


def _series_chunk_query(qids):
    """SPARQL for all video games in the given series QIDs, with platform and
    P1545 ordinal. `VALUES ?series` bounds the query so it stays under WDQS's
    60 s per-query timeout."""
    values_block = " ".join(f"wd:{qid}" for qid in qids)
    return f"""
SELECT DISTINCT
  ?game
  ?gameLabel
  ?platform
  ?series
  ?seriesLabel
  ?ordinal
WHERE {{
  VALUES ?series {{ {values_block} }}
  ?game wdt:P31 wd:Q7889 .
  ?game wdt:P179 ?series .

  OPTIONAL {{ ?game wdt:P400 ?platform . }}

  OPTIONAL {{
    ?game p:P179 ?seriesStmt .
    ?seriesStmt ps:P179 ?series .
    ?seriesStmt pq:P1545 ?ordinal .
  }}

  SERVICE wikibase:label {{
    bd:serviceParam wikibase:language "en,mul" .
  }}
}}
"""


def _fetch_series_qids(qids):
    """Fetch one batch of series QIDs, halving and recursing if the batch is
    too heavy for the 60 s server timeout (`WdqsTimeout`). This lets the
    caller start with a large `chunk_size` for fewer round-trips while staying
    safe: an oversized chunk self-tunes down instead of repeatedly timing out
    and burning the error-query budget."""
    try:
        result = sparql_query(_series_chunk_query(qids))
        return result["results"]["bindings"]
    except WdqsTimeout:
        if len(qids) <= 1:
            # A single series shouldn't time out; if it does, skip it rather
            # than fail the whole extract.
            print(f"  WARN: series {qids} timed out even alone — skipping", file=sys.stderr)
            return []
        mid = len(qids) // 2
        print(f"  Chunk of {len(qids)} timed out; splitting into {mid} + {len(qids) - mid}", file=sys.stderr)
        return _fetch_series_qids(qids[:mid]) + _fetch_series_qids(qids[mid:])


def fetch_series_data(chunk_size=600):
    """Fetch P179 series data with P1545 ordinals for video games, chunked by
    series QID.

    The naive "all video games with P179, no platform filter" query exceeds
    WDQS's 60 s per-query timeout when the result set crosses ~500k rows
    (which it does: ~1k series × multiple games × multiple platform rows).
    We instead enumerate distinct series QIDs once (fast, single query) and
    then issue one bounded query per chunk of `chunk_size` series.

    `chunk_size` is the *starting* batch size: `_fetch_series_qids` halves any
    batch that hits the server timeout, so a generous default keeps the
    round-trip count low (each round-trip pays a courtesy pause) while still
    self-tuning down for unusually dense series. Inter-query pacing is handled
    centrally in `sparql_query`, so there is no manual sleep here."""

    series_qids = fetch_distinct_series_qids()
    total = len(series_qids)
    print(f"  {total} distinct series; fetching games in chunks of {chunk_size}...", file=sys.stderr)

    all_rows = []
    chunks = [series_qids[i:i + chunk_size] for i in range(0, total, chunk_size)]
    for idx, chunk in enumerate(chunks, start=1):
        print(f"  Series chunk {idx}/{len(chunks)} ({len(chunk)} series)...", file=sys.stderr)
        all_rows.extend(_fetch_series_qids(chunk))

    print(f"  Series total: {len(all_rows)} rows across {len(chunks)} chunks", file=sys.stderr)
    return all_rows


def fetch_sequel_data():
    """Fetch P155/P156 sequel/prequel chain data for ALL video games.
    Platform is fetched optionally but not used as a filter.

    Only games that actually carry a P155/P156 statement match, so the result
    set is far smaller than "all video games" and fits under the 60 s timeout
    as a single query. No `ORDER BY` — server-side sorting forces full
    materialization and burns processing time; `main()` sorts the final output
    deterministically anyway."""

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

    # Fetch series data (chunked by series QID — see fetch_series_data docstring).
    # Inter-query pacing is centralized in `sparql_query`, so no manual sleeps.
    series_rows = fetch_series_data()

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

        # Always emit all 6 keys with stable defaults (series_order: null,
        # follows/followed_by: "") so the snapshot has a uniform shape that the
        # workflow's strict validator accepts. Absent values use the same
        # defaults the consumer (build-catalog) already treats as "missing".
        entry = {
            "game_title": game_title,
            "series_name": series_name,
            "system": system,
            "series_order": ordinal if ordinal is not None else None,
            "follows": sequel_info.get("follows") or "",
            "followed_by": sequel_info.get("followed_by") or "",
        }

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

        # Sequel-only entries have no series; emit the full 6-key shape with
        # series_name "" and series_order null to match the series-row rows.
        entry = {
            "game_title": game_title,
            "series_name": "",
            "system": system,
            "series_order": None,
            "follows": row.get("followsLabel", {}).get("value", ""),
            "followed_by": row.get("followedByLabel", {}).get("value", ""),
        }

        entries.append(entry)

        if system == "arcade_fbneo":
            stats["arcade_matches"] += 1
        else:
            stats["console_matches"] += 1

    # Sort by stable identity (game_title, system) rather than the mutable
    # series fields. A Wikidata edit to series_name / series_order then updates
    # a row *in place* instead of moving it, which keeps monthly refresh diffs
    # small and reviewable. series_name + series_order remain as deterministic
    # tiebreakers for the rare game that appears in more than one series (None
    # ordinal coerced to a high sentinel so it never compares against an int).
    # Order is irrelevant to the consumer (build-catalog reads every row).
    entries.sort(key=lambda e: (
        e["game_title"],
        e["system"],
        e["series_name"],
        e["series_order"] if e["series_order"] is not None else 999999,
    ))

    # Log statistics to stderr
    print(f"\n--- Statistics ---", file=sys.stderr)
    print(f"  SPARQL series rows:  {stats['total_series_rows']}", file=sys.stderr)
    print(f"  SPARQL sequel rows:  {stats['total_sequel_rows']}", file=sys.stderr)
    print(f"  Console matches:     {stats['console_matches']}", file=sys.stderr)
    print(f"  Arcade matches:      {stats['arcade_matches']}", file=sys.stderr)
    print(f"  Discarded:           {stats['discarded']}", file=sys.stderr)
    print(f"  Output entries:      {len(entries)}", file=sys.stderr)
    # All 6 keys are always present now, so count by non-default value.
    print(f"    With series:       {sum(1 for e in entries if e['series_name'])}", file=sys.stderr)
    print(f"    With ordinal:      {sum(1 for e in entries if e['series_order'] is not None)}", file=sys.stderr)
    print(f"    With sequel info:  {sum(1 for e in entries if e['follows'] or e['followed_by'])}", file=sys.stderr)

    json.dump(entries, sys.stdout, indent=2, ensure_ascii=False)
    sys.stdout.write("\n")


if __name__ == "__main__":
    try:
        main()
    except WdqsThrottled as e:
        # Escalated ban tier — waiting it out would burn the CI job. Exit
        # non-zero with a clear message; the IP recovers on its own and the
        # next scheduled/manual run picks up on a clean window.
        print(f"\nAborting: {e}", file=sys.stderr)
        sys.exit(2)
    except WdqsTimeout as e:
        # A non-chunked query (e.g. the sequel pass) kept hitting the 60 s
        # server timeout. Surface it loudly rather than emit a partial snapshot.
        print(f"\nAborting: persistent WDQS timeout: {e}", file=sys.stderr)
        sys.exit(3)
