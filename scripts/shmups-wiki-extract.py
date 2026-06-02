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

Entries whose page has its own `/Video Index` sub-page (enumerated from
`Category:Video Index`) get a `"video_index": true` flag. Entries
without one whose `page_title` is a word-boundary prefix of another
entry that does have one (e.g. "DoDonPachi DaiFukkatsu Ver 1.5"
inheriting from "DoDonPachi DaiFukkatsu") get a
`"video_index_inherits_from": "<parent page title>"` field, so the
runtime can link them to the parent's `/Video Index`.

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


def list_section_anchors(page: str) -> list[tuple[str, str]]:
    """Return (heading_text, anchor) pairs for every section of `page`, via
    `action=parse&prop=sections`. The `anchor` is MediaWiki's own URL fragment,
    already encoded the way the wiki expects (e.g. "Version_1.5",
    "Guides_.26_Commentaries"), so a section deep link is exactly
    `<page_url>#<anchor>`. Used to point a variant's *inherited* Video Index
    link at the matching section instead of the page top."""
    data = http_get_json(
        {
            "action": "parse",
            "page": page,
            "prop": "sections",
            "format": "json",
            "formatversion": "2",
        }
    )
    return [
        (section["line"], section["anchor"])
        for section in data.get("parse", {}).get("sections", [])
        if section.get("line") and section.get("anchor")
    ]


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


_VERSION_WORD_RE = re.compile(r"\bversion\b", re.IGNORECASE)


def _section_match_key(text: str) -> str:
    """Key for matching a variant's residual title against a Video Index
    heading. Same alnum-lowercasing as the title normalizer, but first
    collapses "Version" -> "Ver" so a page residual like "Ver 1.5" matches a
    heading written "Version 1.5"."""
    return normalize_title_for_metadata(_VERSION_WORD_RE.sub("Ver", text))


def _sections_for(vi_page, section_cache):
    """Memoized section fetch; degrades to an empty list on failure so a
    missing/erroring Video Index page just yields no anchor (page-top link)."""
    if vi_page not in section_cache:
        try:
            section_cache[vi_page] = list_section_anchors(vi_page)
        except Exception as exc:  # noqa: BLE001 - degrade to page-top link
            print(
                f"shmups-wiki-extract: section fetch failed for {vi_page!r}: {exc}",
                file=sys.stderr,
            )
            section_cache[vi_page] = []
    return section_cache[vi_page]


def _full_title_section_anchor(vi_page, normalized_title, section_cache):
    """Anchor for the section of `vi_page` whose heading, normalized, equals
    `normalized_title` — for variants that are a *section* of a parent's Video
    Index rather than their own page (e.g. the ROM "DoDonPachi DaiOuJou Black
    Label" redirects to "DoDonPachi DaiOuJou", whose Video Index has a
    "DoDonPachi DaiOuJou Black Label" section).

    Returns the anchor only on a UNIQUE match against a NON-FIRST section. The
    first section is the page's main game; a synonym of it should keep the
    page-top link, not anchor to its own heading. Ambiguous or absent matches
    return None so the caller falls back to the page top."""
    matches = [
        (idx, anchor)
        for idx, (line, anchor) in enumerate(_sections_for(vi_page, section_cache))
        if normalize_title_for_metadata(line) == normalized_title
    ]
    distinct = {anchor for _, anchor in matches}
    if len(distinct) == 1 and min(idx for idx, _ in matches) > 0:
        return matches[0][1]
    return None


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------


def main() -> int:
    print("shmups-wiki-extract: enumerating main-namespace pages...", file=sys.stderr)
    titles = collect_game_pages()

    # A healthy crawl always finds hundreds of pages. Zero means the wiki API
    # returned nothing (transient outage / rate-limit) — abort with a non-zero
    # exit so the snapshot is never overwritten with an empty result, rather
    # than emitting `[]` that downstream would treat as "all shmups removed".
    if not titles:
        sys.exit(
            "shmups-wiki-extract: 0 pages returned by the wiki API; "
            "aborting without writing an empty snapshot"
        )

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
    # Shared across the redirect-section pass below and the inherited-variant
    # pass further down so each Video Index page's sections are fetched once.
    section_cache: dict[str, list[tuple[str, str]]] = {}
    flagged = 0
    sectioned = 0
    for row in rows:
        if row["page_title"] not in video_index_parents:
            continue
        # A redirect/synonym row points at a Video Index page it does NOT own
        # (its normalized title differs from the page's). When that name matches
        # a specific section of the page, the game is a *section* of the parent's
        # Video Index, not the whole page — e.g. "DoDonPachi DaiOuJou Black
        # Label" redirects to "DoDonPachi DaiOuJou" and is the Black Label
        # section. Deep-link to the section via the same inherits_from + anchor
        # shape build-catalog already understands. The article's own row (whose
        # normalized title equals the page) keeps the whole-page link.
        page = row["page_title"]
        if row["normalized_title"] != normalize_title_for_metadata(page):
            anchor = _full_title_section_anchor(
                f"{page}{VIDEO_INDEX_SUFFIX}",
                row["normalized_title"],
                section_cache,
            )
            if anchor is not None:
                row["video_index_inherits_from"] = page
                row["video_index_anchor"] = anchor
                sectioned += 1
                continue
        row["video_index"] = True
        flagged += 1
    print(
        f"shmups-wiki-extract: flagged {flagged} row(s) as having a video index "
        f"({len(video_index_parents)} parent pages in Category:Video Index); "
        f"deep-linked {sectioned} redirect/synonym row(s) to a section",
        file=sys.stderr,
    )

    # Orphan Video Index pages: a "<Game>/Video Index" sub-page can be a member
    # of Category:Video Index while the game's *main article* doesn't exist yet
    # (e.g. "Shikigami no Shiro", "Pulstar", "19XX: The War Against Destiny",
    # "Zero Gunner 2"). collect_game_pages only enumerates main-namespace
    # articles, so those games never produced a row and the flag loop above had
    # nothing to mark. Editors curate Category:Video Index by hand, so a
    # membership entry is authoritative proof the game exists and has a
    # walkthrough — emit a row for it directly, keyed on the parent title. These
    # also become valid inherit targets for the variant pass below.
    existing_titles = {row["page_title"] for row in rows}
    orphan_video_index = 0
    for parent in sorted(video_index_parents):
        if parent in existing_titles:
            continue
        key = normalize_title_for_metadata(parent)
        if not key or key in seen_keys:
            continue
        seen_keys.add(key)
        rows.append(
            {"normalized_title": key, "page_title": parent, "video_index": True}
        )
        orphan_video_index += 1
    print(
        f"shmups-wiki-extract: added {orphan_video_index} orphan video-index "
        f"row(s) for games whose main article is missing",
        file=sys.stderr,
    )

    # Inherited video index: variant pages (e.g. "DoDonPachi DaiFukkatsu
    # Ver 1.5", "… Arrange A") typically don't have their own /Video Index
    # sub-page — the videos live at the parent's /Video Index. The wiki
    # doesn't structurally express the variant-to-parent link, so we infer
    # it from the title: strip a known *variant suffix* and see if the
    # result is another page that does have a video index.
    #
    # Suffix list is intentionally narrow. A blanket "longest prefix that
    # also exists" rule pulls in sequels and series-overview pages
    # ("Deathsmiles II" → "Deathsmiles", "Gradius series" → "Gradius",
    # "Darius Force" → "Darius") whose videos belong to a different game.
    # Pattern-matched suffixes catch the cases the user actually means
    # (release versions, arrangements, label/edition reissues) and skip
    # the rest. Add new patterns when a real variant turns up uncovered.
    #
    # Patterns are tried in order; the first one that strips down to a
    # known-video-index page wins. List shorter strips first so e.g.
    # "DaiFukkatsu Black Label Arrange" inherits from "Black Label" (its
    # own video index) rather than skipping all the way to "DaiFukkatsu".
    variant_suffix_patterns = [
        re.compile(p, re.IGNORECASE)
        for p in [
            r"\s+Arrange(?:\s+[A-Z])?\s*$",  # Arrange, Arrange A
            r"\s+Ver\.?\s*\d+(?:\.\d+)*\s*$",  # Ver 1.5, Ver.1.5
            r"\s+v\d+(?:\.\d+)*\s*$",  # v1.5
            r"\s+exA\s+Label\s*$",  # exA Label
            r"\s+\S+\s+Edition\s*$",  # Special Edition, Swing-by Edition
            r"\s+Black\s+Label\s*$",  # Black Label
        ]
    ]
    video_index_titles = {row["page_title"] for row in rows if row.get("video_index")}
    inferred = 0
    anchored = 0
    for row in rows:
        # Skip rows already resolved: own Video Index pages and the
        # redirect/synonym rows the section pass above linked to a section.
        if row.get("video_index") or row.get("video_index_inherits_from"):
            continue
        title = row["page_title"]
        for pattern in variant_suffix_patterns:
            m = pattern.search(title)
            if not m:
                continue
            candidate = title[: m.start()].rstrip()
            if candidate not in video_index_titles:
                continue
            row["video_index_inherits_from"] = candidate
            inferred += 1
            # Deep-link to the matching section of the parent's Video Index —
            # but only when the variant's residual title (the part after the
            # parent, e.g. "Ver 1.5") UNIQUELY matches one heading. Ambiguous,
            # absent, or fetch-failure cases leave the anchor unset, so the link
            # falls back to the page top and can never point at a wrong section.
            residual = title[len(candidate) :].strip()
            if residual:
                want = _section_match_key(residual)
                matches = [
                    anchor
                    for line, anchor in _sections_for(
                        f"{candidate}{VIDEO_INDEX_SUFFIX}", section_cache
                    )
                    if _section_match_key(line) == want
                ]
                if len(matches) == 1:
                    row["video_index_anchor"] = matches[0]
                    anchored += 1
            break
    print(
        f"shmups-wiki-extract: inferred video_index_inherits_from for "
        f"{inferred} variant row(s); resolved a section anchor for {anchored}",
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
