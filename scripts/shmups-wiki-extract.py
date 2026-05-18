#!/usr/bin/env python3
"""
Extract the game-page index from Shmups Wiki (https://shmups.wiki/library/).

For every page in the wiki's "Games" category tree, emits a row mapping the
title we'd see in `game_library.base_title` (after RePlayOS title
normalization) to the exact wiki page title used in the URL. Every
incoming MediaWiki redirect for those pages is emitted as an additional
synonym row pointing at the same target, so regional/alternate names like
`Gunlock → RayForce` resolve without needing a runtime alias lookup.

Output: JSON to stdout, one object per page:
    [
      {"normalized_title": "battlegarrega", "page_title": "Battle Garegga"},
      {"normalized_title": "donpachi",      "page_title": "DonPachi"},
      ...
    ]

The Rust side embeds this JSON at compile time and uses it to render a
"Strategy guide on Shmups Wiki" deep link on the game-detail page.

The wiki is licensed under CC BY-SA 4.0. We only embed the page-title
mapping (factual data), not the wiki prose; the runtime renders attribution
next to the link.

Usage:
    python3 scripts/shmups-wiki-extract.py > data/shmups-wiki/games.json
"""

import json
import re
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

API_ENDPOINT = "https://shmups.wiki/api.php"
USER_AGENT = "ReplayControl-ShmupsWiki/1.0 (+https://github.com/lapastillaroja/replay-control)"

# Strategy: enumerate every top-level main-namespace non-redirect page, then
# filter out things that aren't game articles. A pure category walk misses
# games whose pages have no category tags assigned (e.g. Thunder Force III as
# of 2026-05-17), so we go broader and prune.
#
# Pruning rules:
#   1. Exclude subpages: any title containing '/'  (these are
#      `Battle Garegga/Strategy`, `1942/Video Index`, etc. — guide pages
#      under a parent game article).
#   2. Exclude pages whose title is also a category name (developer pages
#      like "CAVE", "Capcom", "Compile", "MOSS", mechanic pages, etc. —
#      every developer that has its own per-publisher category also has a
#      page with that same name).
#   3. Exclude an explicit denylist of meta pages whose names don't collide
#      with categories but are obviously not games (style guides, glossary,
#      record lists, etc.).
META_PAGE_DENYLIST = frozenset(
    {
        "Archival Efforts",
        "Beginner's Guide to Shooting Games",
        "Boghog's bullet hell shmup 101",
        "Book of Star Mythology",
        "Category guidelines",
        "Glossary",
        "Glossary of shmups",
        "Graze",
        "Grazing",
        "Hall of Records",
        "Help:Contents",
        "Main Page",
        "STG Hall of Records",
    }
)

# MediaWiki namespace IDs.
NS_MAIN = 0
NS_CATEGORY = 14


def http_get_json(params: dict, retries: int = 5, backoff: float = 2.0) -> dict:
    """GET API_ENDPOINT with the given query params, retried on transient
    failures. Returns the parsed JSON response."""
    encoded = urllib.parse.urlencode(params)
    url = f"{API_ENDPOINT}?{encoded}"
    headers = {"User-Agent": USER_AGENT, "Accept": "application/json"}
    req = urllib.request.Request(url, headers=headers)

    for attempt in range(retries):
        try:
            with urllib.request.urlopen(req, timeout=60) as resp:
                return json.loads(resp.read().decode("utf-8"))
        except urllib.error.HTTPError as e:
            if e.code == 429 or e.code >= 500:
                wait = backoff * (2**attempt)
                print(
                    f"HTTP {e.code} on {url!r}, retrying in {wait}s "
                    f"(attempt {attempt + 1}/{retries})",
                    file=sys.stderr,
                )
                time.sleep(wait)
                continue
            raise
        except urllib.error.URLError as e:
            wait = backoff * (2**attempt)
            print(
                f"Network error {e!r} on {url!r}, retrying in {wait}s "
                f"(attempt {attempt + 1}/{retries})",
                file=sys.stderr,
            )
            time.sleep(wait)
        except json.JSONDecodeError as e:
            wait = backoff * (2**attempt)
            print(
                f"Truncated JSON on {url!r}: {e}, retrying in {wait}s "
                f"(attempt {attempt + 1}/{retries})",
                file=sys.stderr,
            )
            time.sleep(wait)

    raise RuntimeError(f"shmups.wiki API call failed after {retries} retries: {url}")


def list_main_ns_pages() -> list[str]:
    """Every non-redirect main-namespace page title, paginated through
    `apcontinue` until exhausted."""
    titles: list[str] = []
    apcontinue: str | None = None
    while True:
        params = {
            "action": "query",
            "list": "allpages",
            "apnamespace": str(NS_MAIN),
            "apfilterredir": "nonredirects",
            "aplimit": "500",
            "apdir": "ascending",
            "format": "json",
            "formatversion": "2",
        }
        if apcontinue:
            params["apcontinue"] = apcontinue
        data = http_get_json(params)
        for row in data.get("query", {}).get("allpages", []):
            title = row.get("title")
            if title:
                titles.append(title)
        apcontinue = data.get("continue", {}).get("apcontinue")
        if not apcontinue:
            break
    return titles


def list_all_category_names() -> set[str]:
    """Every category name on the wiki, lowercased. Used to filter out
    developer/genre/mechanic pages whose article shares the category name
    (CAVE, Capcom, Bullet Hell, etc. — those are developer/topic pages, not
    games)."""
    names: set[str] = set()
    accontinue: str | None = None
    while True:
        params = {
            "action": "query",
            "list": "allcategories",
            "aclimit": "500",
            "format": "json",
            "formatversion": "2",
        }
        if accontinue:
            params["accontinue"] = accontinue
        data = http_get_json(params)
        for row in data.get("query", {}).get("allcategories", []):
            name = row.get("category")
            if name:
                names.add(name)
        accontinue = data.get("continue", {}).get("accontinue")
        if not accontinue:
            break
    return names


def iter_categorymembers(category: str):
    """Yield every main-namespace member title of `Category:<category>`,
    paginated through `cmcontinue` until exhausted. Shared by the
    orientation walk and the `Category:Video Index` walk so both have one
    place to update pagination/throttling concerns."""
    cmcontinue: str | None = None
    while True:
        params = {
            "action": "query",
            "list": "categorymembers",
            "cmtitle": f"Category:{category}",
            "cmnamespace": str(NS_MAIN),
            "cmlimit": "500",
            "format": "json",
            "formatversion": "2",
        }
        if cmcontinue:
            params["cmcontinue"] = cmcontinue
        data = http_get_json(params)
        for row in data.get("query", {}).get("categorymembers", []):
            title = row.get("title")
            if title:
                yield title
        cmcontinue = data.get("continue", {}).get("cmcontinue")
        if not cmcontinue:
            break


def list_orientation_games() -> set[str]:
    """Game titles enumerated by the wiki's orientation/origin categories.
    These are the authoritative game pages; any title here is kept even if
    it also matches a category name (e.g. games with their own per-game
    subcategory like `Category:DoDonPachi DaiOuJou` or `Category:Espgaluda`)."""
    titles: set[str] = set()
    for category in [
        "Vertical orientation",
        "Horizontal orientation",
        "Independent/Doujin shooting games",
        "Free to Play shooting games",
    ]:
        titles.update(iter_categorymembers(category))
    return titles


VIDEO_INDEX_SUFFIX = "/Video Index"


def list_video_index_pages() -> set[str]:
    """Parent page titles that have a `<title>/Video Index` sub-page,
    enumerated from `Category:Video Index`. Members come back as full
    subpage titles like `"DoDonPachi DaiOuJou/Video Index"`; strip the
    trailing suffix to get the parent game's page title.

    Wiki editors hand-curate this category, so it's the authoritative
    list of games whose article has a separate video-walkthrough page.
    Used to flag `video_index: true` on the matching index rows; the
    runtime then renders a second deep link next to the strategy guide.
    """
    return {
        title[: -len(VIDEO_INDEX_SUFFIX)]
        for title in iter_categorymembers("Video Index")
        if title.endswith(VIDEO_INDEX_SUFFIX)
    }


def list_redirects_for_targets(targets: set[str]) -> dict[str, str]:
    """For every accepted game page, fetch its incoming main-namespace
    redirects via `prop=redirects`. Returns `{redirect_source: target}`.

    Wiki editors curate redirects to handle regional/Romanized/alternate
    names (e.g. `Gunlock → RayForce`, `怒首領蜂 → DonPachi`), so each
    redirect becomes another `normalized_title → page_title` entry in the
    bundled index. Without this, base-titles like "Gunlock" never match.
    """
    redirects: dict[str, str] = {}
    target_list = sorted(targets)
    batch_size = 50  # mediawiki `titles=` cap for non-bot accounts
    for i in range(0, len(target_list), batch_size):
        batch = target_list[i : i + batch_size]
        rdcontinue: str | None = None
        while True:
            params = {
                "action": "query",
                "prop": "redirects",
                "titles": "|".join(batch),
                "rdnamespace": str(NS_MAIN),
                "rdlimit": "500",
                "format": "json",
                "formatversion": "2",
            }
            if rdcontinue:
                params["rdcontinue"] = rdcontinue
            data = http_get_json(params)
            for page in data.get("query", {}).get("pages", []):
                target_title = page.get("title")
                if not target_title:
                    continue
                for entry in page.get("redirects", []):
                    src = entry.get("title")
                    if src:
                        redirects[src] = target_title
            rdcontinue = data.get("continue", {}).get("rdcontinue")
            if not rdcontinue:
                break
    return redirects


def collect_game_pages() -> list[str]:
    """Apply the strategy described at the top of the file: enumerate every
    main-ns page, then drop subpages, the explicit meta-page denylist, and
    category-named pages (developers, genres, mechanics) — except when the
    page is also enumerated by an orientation category, which means it's a
    real game whose article happens to share its name with a per-game
    subcategory."""
    all_titles = list_main_ns_pages()
    category_names = list_all_category_names()
    known_games = list_orientation_games()
    accepted: list[str] = []
    rejected_subpages = 0
    rejected_category_match = 0
    rejected_meta = 0
    for title in all_titles:
        if "/" in title:
            rejected_subpages += 1
            continue
        if title in META_PAGE_DENYLIST:
            rejected_meta += 1
            continue
        if title in category_names and title not in known_games:
            rejected_category_match += 1
            continue
        accepted.append(title)
    print(
        f"shmups-wiki-extract: {len(all_titles)} total pages, "
        f"{len(known_games)} game pages confirmed by orientation categories, "
        f"{rejected_subpages} subpages skipped, "
        f"{rejected_category_match} non-game category pages skipped, "
        f"{rejected_meta} meta pages skipped, "
        f"{len(accepted)} game candidates",
        file=sys.stderr,
    )
    return sorted(accepted)


# ---------------------------------------------------------------------------
# Title normalization (mirror of replay_control_core::game::title_utils::
# normalize_title_for_metadata)
#
# Keep this in lock-step with the Rust function. Algorithm:
#   1. Drop characters inside `(...)` and `[...]` (any nesting).
#   2. If a title ends with ", The" / ", A" / ", An", reorder to "The Foo".
#   3. Strip TOSEC-style trailing version (`v1.000`).
#   4. Keep only alphanumerics, lowercased.
# ---------------------------------------------------------------------------

VERSION_RE = re.compile(r"\s+v\d+(\.\d+)*\s*$", re.IGNORECASE)


def _strip_bracketed(name: str) -> str:
    out: list[str] = []
    depth = 0
    for ch in name:
        if ch in "([":
            depth += 1
        elif ch in ")]":
            if depth > 0:
                depth -= 1
        elif depth == 0:
            out.append(ch)
    return "".join(out).strip()


def _reorder_trailing_article(name: str) -> str:
    idx = name.rfind(", ")
    if idx == -1:
        return name
    before = name[:idx]
    after_comma = name[idx + 2 :]
    first_word_end = 0
    for first_word_end, ch in enumerate(after_comma):
        if not ch.isalpha():
            break
    else:
        first_word_end = len(after_comma)
    first_word = after_comma[:first_word_end]
    if first_word.lower() in ("the", "a", "an"):
        rest = after_comma[first_word_end:].lstrip(" -")
        return f"{first_word} {before} {rest}".rstrip()
    return name


def normalize_title_for_metadata(name: str) -> str:
    """Python mirror of the Rust `normalize_title_for_metadata`. Keep the
    two in sync; any divergence here silently breaks lookups."""
    stripped = _strip_bracketed(name)
    reordered = _reorder_trailing_article(stripped)
    versionless = VERSION_RE.sub("", reordered)
    return "".join(ch.lower() for ch in versionless if ch.isalnum())


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------


def main() -> int:
    print("shmups-wiki-extract: enumerating main-namespace pages...", file=sys.stderr)
    titles = collect_game_pages()

    rows: list[dict[str, object]] = []
    seen_keys: set[str] = set()
    collisions: list[tuple[str, str, str]] = []

    for page_title in titles:
        key = normalize_title_for_metadata(page_title)
        if not key:
            continue
        if key in seen_keys:
            # Multiple pages collapse to the same normalized title. Keep the
            # first, log the collision so a reviewer can decide if
            # disambiguation is warranted.
            prior = next(r["page_title"] for r in rows if r["normalized_title"] == key)
            collisions.append((key, prior, page_title))
            continue
        seen_keys.add(key)
        rows.append({"normalized_title": key, "page_title": page_title})

    print(
        "shmups-wiki-extract: enumerating incoming redirects for synonym rows...",
        file=sys.stderr,
    )
    redirect_map = list_redirects_for_targets(set(titles))
    synonyms_added = 0
    for src, target in sorted(redirect_map.items()):
        key = normalize_title_for_metadata(src)
        if not key or key in seen_keys:
            continue
        seen_keys.add(key)
        rows.append({"normalized_title": key, "page_title": target})
        synonyms_added += 1
    print(
        f"shmups-wiki-extract: added {synonyms_added} synonym row(s) "
        f"from {len(redirect_map)} redirects",
        file=sys.stderr,
    )

    print(
        "shmups-wiki-extract: enumerating Category:Video Index members...",
        file=sys.stderr,
    )
    video_index_parents = list_video_index_pages()
    flagged = 0
    for row in rows:
        if row["page_title"] in video_index_parents:
            row["video_index"] = True
            flagged += 1
    print(
        f"shmups-wiki-extract: flagged {flagged} row(s) as having a video index "
        f"({len(video_index_parents)} parent pages in Category:Video Index)",
        file=sys.stderr,
    )

    rows.sort(key=lambda r: r["normalized_title"])

    if collisions:
        print(
            f"shmups-wiki-extract: {len(collisions)} normalized-title collision(s); "
            "keeping first occurrence",
            file=sys.stderr,
        )
        for key, kept, dropped in collisions[:20]:
            print(f"  {key!r}: kept {kept!r}, dropped {dropped!r}", file=sys.stderr)

    json.dump(rows, sys.stdout, ensure_ascii=False, indent=2)
    sys.stdout.write("\n")
    print(f"shmups-wiki-extract: wrote {len(rows)} rows", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
