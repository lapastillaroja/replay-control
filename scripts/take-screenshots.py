#!/usr/bin/env python3
"""Capture screenshots of the Replay Control app for the documentation site.

Always captures both desktop (1280x800) and mobile (375x812) for every page.
Desktop files: {name}.png, mobile files: {name}-mobile.png.

For reproducible galleries the run starts with a state-preparation step:
locale forced to English, skin sync off + default skin, favorites reset to a
curated list, and (device only) recents rebuilt by launching a curated game
sequence via the RePlayOS API — the last launch stays running so the
now-playing shots have a live game with a disc indicator.

Pages marked device-only (Net Control settings, now-playing) are skipped
automatically when the target runs in standalone mode; point APP_URL at the
Pi to capture everything.

A final pass re-captures the mobile home page in Spanish and Japanese for the
site's language gallery (home-mobile-es.png / home-mobile-ja.png), restoring
the locale to English afterwards.

Usage:
    python scripts/take-screenshots.py
    APP_URL=http://replay.local:8080 python scripts/take-screenshots.py
    python scripts/take-screenshots.py --skip-prep   # capture only
"""

import argparse
import os
import sys
from pathlib import Path

from playwright.sync_api import sync_playwright, TimeoutError as PlaywrightTimeout

APP_URL = os.environ.get("APP_URL", "http://localhost:8091")

DESKTOP = {"width": 1280, "height": 800}
MOBILE = {"width": 375, "height": 812}

# Skin indices (0-10): REPLAY, MEGA TECH, PLAY CHOICE, ASTRO, SUPER VIDEO,
#                       MVS, RPG, FANTASY, SIMPLE PURPLE, METAL, UNICOLORS
DEFAULT_SKIN = 0  # REPLAY — clean default
DEFAULT_LOCALE = "en"  # gallery language; the prep step forces it

# Curated favorites: recognizable titles across systems, in display order
# (first entry shows first in the UI; the newest-added favorite displays
# first, so the prep step adds them in reverse). (system, rom_path). Missing
# files are skipped with a warning so the run still works against libraries
# that lack an entry.
FAVORITES = [
    ("sega_smd", "/roms/sega_smd/00 Clean Romset/Sonic The Hedgehog 2 (World) (Rev A).md"),
    ("nintendo_snes", "/roms/nintendo_snes/00 Clean Romset/Super Mario World (USA).smc"),
    ("nintendo_snes", "/roms/nintendo_snes/00 Clean Romset/Legend of Zelda, The - A Link to the Past (USA).sfc"),
    ("nintendo_nes", "/roms/nintendo_nes/00 NES/00 Clean Romset/Super Mario Bros. 3 (USA) (Rev 1).nes"),
    ("nintendo_gba", "/roms/nintendo_gba/00 Clean Romset/Castlevania - Aria of Sorrow (USA).gba"),
    ("nintendo_n64", "/roms/nintendo_n64/00 Clean Romset/Mario Kart 64 (USA).z64"),
    ("sega_smd", "/roms/sega_smd/00 Clean Romset/Shinobi III - Return of the Ninja Master (USA).md"),
    ("sega_smd", "/roms/sega_smd/00 Clean Romset/Rocket Knight Adventures (USA).md"),
    ("sega_dc", "/roms/sega_dc/Sonic Adventure 2 v1.008 (2001)(Sega)(PAL)(M5)[!]/Sonic Adventure 2 v1.008 (2001)(Sega)(PAL)(M5)[!].gdi"),
]

# Curated recents, oldest -> newest. Device only: each entry is launched for
# real through the RePlayOS API so the `_recent/` markers get honest ordered
# mtimes. The LAST entry stays running — it powers the now-playing shots, and
# is multi-disc on purpose so the "Disc 1/4" indicator shows.
RECENTS_LAUNCH_ORDER = [
    "/roms/nintendo_n64/00 Clean Romset/Mario Kart 64 (USA).z64",
    "/roms/nintendo_snes/00 Clean Romset/Legend of Zelda, The - A Link to the Past (USA).sfc",
    "/roms/nintendo_gba/00 Clean Romset/Castlevania - Aria of Sorrow (USA).gba",
    "/roms/sega_smd/00 Clean Romset/Shinobi III - Return of the Ninja Master (USA).md",
    "/roms/arcade_fbneo/Vertical/00 Clean Romset/gunlock.zip",
    "/roms/ibm_pc/Indiana Jones and the Fate of Atlantis (Spanish).zip",
    "/roms/sega_smd/00 Clean Romset/Cool Spot (USA).md",
    "/roms/sega_smd/00 Clean Romset/Sonic The Hedgehog 2 (World) (Rev A).md",
    "/roms/sega_32x/01 Sega CD 32X/Slam City with Scottie Pippen (Sega CD 32X) (USA).m3u",
]
LAUNCH_SETTLE_SECS = 5  # gap between launches so recents mtimes are ordered
FINAL_LAUNCH_BOOT_SECS = 15  # let the last game boot before now-playing shots

# Error indicators to check after each screenshot.
ERROR_SELECTORS = [".error", ".error-message", ".not-found"]
ERROR_TEXT_PATTERNS = ["Something went wrong", "Server error", "Internal Server Error", "panicked"]

PAGES = [
    # First-run setup checklist (force-shown via ?setup)
    {"name": "setup", "path": "/?setup", "wait": ".setup-checklist", "extra_wait": 3000},
    # Home & navigation
    {"name": "home", "path": "/", "wait": "main"},
    # System browser
    {"name": "system-megadrive", "path": "/games/sega_smd?hide_hacks=true&hide_translations=true&hide_betas=true&min_rating=4", "wait": ".content", "extra_wait": 8000},
    # Game detail
    {"name": "detail-sonic2", "path": "/games/sega_smd/Sonic%20The%20Hedgehog%202%20(World)%20(Rev%20A).md", "wait": ".content", "extra_wait": 8000},
    # Game detail scrolled to info card
    {"name": "detail-info", "path": "/games/sega_smd/Sonic%20The%20Hedgehog%202%20(World)%20(Rev%20A).md", "wait": ".content", "extra_wait": 8000, "scroll": 350, "skin": 10},  # UNICOLORS
    # Box art picker (mobile only — click "Change cover" to open)
    {"name": "boxart-picker", "path": "/games/sega_smd/Rocket%20Knight%20Adventures%20(Europe).md", "wait": ".content", "extra_wait": 8000, "click": ".change-cover-link", "click_wait": ".boxart-picker-overlay", "mobile_only": True},
    # Search
    {"name": "search-zelda", "path": "/search?q=zelda", "wait": ".content", "extra_wait": 8000},
    {"name": "search-capcom", "path": "/search?q=capcom", "wait": ".content", "extra_wait": 8000, "skin": 3},  # ASTRO
    {"name": "search-mario", "path": "/search?q=mario", "wait": ".content", "extra_wait": 8000},
    # Favorites
    {"name": "favorites", "path": "/favorites", "wait": "main"},
    # Favorites scrolled to stats + organize button
    {"name": "favorites-stats", "path": "/favorites", "wait": "main", "scroll": 400},
    # Favorites organize UI (scroll further to show full organize block)
    {"name": "favorites-organize-ui", "path": "/favorites", "wait": "main", "scroll": 400, "click": ".organize-toggle", "click_wait": ".organize-panel"},
    # Favorites grouped by system view
    {"name": "favorites-grouped", "path": "/favorites", "wait": "main", "scroll": 1400},
    # Home page recommendations (scrolled)
    {"name": "recommendations", "path": "/", "wait": "main", "scroll": 500, "skin": 5},  # MVS
    # Settings page
    {"name": "settings", "path": "/settings", "wait": "main"},
    # Skin selection page
    {"name": "skins-page", "path": "/settings/skin", "wait": "main", "extra_wait": 5000},
    # Metadata page
    {"name": "metadata", "path": "/metadata", "wait": "main"},
    # ── Device-only (skipped automatically in standalone mode) ──
    # RePlayOS Net Control settings: status card ("Connected to RePlayOS ...")
    {"name": "net-control", "path": "/settings/replay-net-control", "wait": ".net-control-status", "extra_wait": 3000, "device_only": True},
    # Same page with the setup sections open (via the "Re-enter code" button)
    {"name": "net-control-setup", "path": "/settings/replay-net-control", "wait": ".net-control-status", "extra_wait": 3000, "click": ".form-btn-secondary", "click_wait": ".net-control-code-input", "device_only": True},
    # Home with a live game: play-state badge + "Disc 1/4" indicator (the
    # recents prep leaves the multi-disc Slam City running)
    {"name": "home-now-playing", "path": "/", "wait": ".hero-card-playing", "extra_wait": 9000, "device_only": True},
]

# Localized home shots for the site's language gallery (mobile only, exact
# file names referenced by site/layouts/home.html). Captured in a dedicated
# pass after PAGES so the locale flip can't leak into the main gallery; the
# locale is restored to DEFAULT_LOCALE afterwards.
LOCALE_SHOTS = [
    ("es", "home-mobile-es"),
    ("ja", "home-mobile-ja"),
]

MAX_RETRIES = 2


SFN_HEADERS = {
    "Content-Type": "application/x-www-form-urlencoded",
    "Accept": "application/x-www-form-urlencoded",
}


def sfn(page, name, data="", accept_json=False):
    """POST a Leptos server function (/sfn/<PascalCaseName>). Returns response."""
    headers = dict(SFN_HEADERS)
    if accept_json:
        headers["Accept"] = "application/json"
    resp = page.request.post(f"{APP_URL}/sfn/{name}", headers=headers, data=data)
    if not resp.ok:
        print(f"  WARNING: {name}({data!r}) returned {resp.status}")
    return resp


def set_skin(page, index):
    """Set the active skin via server function POST."""
    sfn(page, "SetSkin", f"index={index}")


def disable_skin_sync(page):
    """Disable skin sync so the skin stays fixed during the run."""
    sfn(page, "SetSkinSync", "enabled=false")


def set_locale(page, locale):
    """Force the UI language so the gallery is consistent."""
    sfn(page, "SaveLocale", f"locale={locale}")


def detect_device_mode(page):
    """True when the target app runs on the RePlayOS device."""
    resp = sfn(page, "GetMode", accept_json=True)
    return resp.ok and "device" in resp.text().lower()


def reset_favorites(page):
    """Replace whatever favorites exist with the curated list."""
    import json
    from urllib.parse import quote

    resp = sfn(page, "GetFavorites", accept_json=True)
    if resp.ok:
        try:
            existing = json.loads(resp.text())
        except json.JSONDecodeError:
            existing = []
        markers = [
            f.get("marker_filename")
            for f in existing
            if isinstance(f, dict) and f.get("marker_filename")
        ]
        for marker in markers:
            sfn(page, "RemoveFavorite", f"filename={quote(marker)}")
        if markers:
            print(f"  cleared {len(markers)} existing favorite(s)")

    added = 0
    # Newest-added shows first in the UI: add in reverse so FAVORITES[0]
    # ends up displayed first.
    for system, rom_path in reversed(FAVORITES):
        resp = sfn(
            page,
            "AddFavorite",
            f"system={quote(system)}&rom_path={quote(rom_path)}&grouped=false",
        )
        if resp.ok:
            added += 1
        else:
            print(f"  WARNING: could not favorite {rom_path} (missing from library?)")
    print(f"  set {added}/{len(FAVORITES)} curated favorites")


def prepare_recents(page):
    """Device only: launch the curated sequence so 'Last played' is
    deterministic. The final game keeps running for the now-playing shots."""
    from urllib.parse import quote

    for i, rom_path in enumerate(RECENTS_LAUNCH_ORDER):
        last = i == len(RECENTS_LAUNCH_ORDER) - 1
        print(f"  launching {rom_path.rsplit('/', 1)[-1]} ...", flush=True)
        resp = sfn(page, "LaunchGame", f"rom_path={quote(rom_path)}&return_to=%2F")
        if not resp.ok:
            print("  WARNING: launch failed, recents order may be incomplete")
        page.wait_for_timeout((FINAL_LAUNCH_BOOT_SECS if last else LAUNCH_SETTLE_SECS) * 1000)


def check_for_errors(page):
    """Return an error description if the page shows error indicators, else None."""
    for sel in ERROR_SELECTORS:
        if page.query_selector(sel):
            return f"found element matching '{sel}'"
    text = page.inner_text("body")
    for pattern in ERROR_TEXT_PATTERNS:
        if pattern in text:
            return f"page contains '{pattern}'"
    return None


def capture(page, name, url, wait, extra_wait, viewport, output_dir, scroll=0, click=None, click_wait=None):
    """Capture a single screenshot with retry on error."""
    page.set_viewport_size(viewport)
    out_path = str(output_dir / f"{name}.png")

    for attempt in range(1 + MAX_RETRIES):
        if attempt > 0:
            print(f"  {name}: retry {attempt}/{MAX_RETRIES} ...", end=" ", flush=True)
            page.wait_for_timeout(2000)
        else:
            print(f"  {name}: {url} ...", end=" ", flush=True)

        page.goto(url, wait_until="load", timeout=30000)
        page.wait_for_selector(wait, timeout=30000)
        page.wait_for_timeout(extra_wait)

        error = check_for_errors(page)
        if error and attempt < MAX_RETRIES:
            print(f"error ({error}), retrying")
            continue

        if error:
            print(f"WARNING ({error})")
        break

    if scroll:
        page.evaluate(f"window.scrollBy(0, {scroll})")
        page.wait_for_timeout(1500)

    if click:
        page.click(click)
        if click_wait:
            page.wait_for_selector(click_wait, timeout=10000)
        page.wait_for_timeout(1000)

    page.screenshot(path=out_path, full_page=False)
    if not error:
        print("ok")
    return error


def main():
    parser = argparse.ArgumentParser(description="Capture app screenshots (desktop + mobile)")
    parser.add_argument(
        "--output-dir",
        default="site/static/screenshots",
        help="Output directory (default: site/static/screenshots/)",
    )
    parser.add_argument(
        "--skip-prep",
        action="store_true",
        help="Skip state preparation (locale/skin/favorites/recents) and only capture",
    )
    args = parser.parse_args()

    project_root = Path(__file__).resolve().parent.parent
    output_dir = project_root / args.output_dir
    output_dir.mkdir(parents=True, exist_ok=True)

    errors = []

    with sync_playwright() as pw:
        browser = pw.chromium.launch()
        page = browser.new_page()

        # Check app is running
        try:
            page.goto(APP_URL, timeout=5000)
        except (PlaywrightTimeout, Exception) as e:
            print(f"Error: cannot reach {APP_URL} — is the app running?")
            print(f"  ({e})")
            browser.close()
            sys.exit(1)

        is_device = detect_device_mode(page)
        print(f"Target mode: {'device' if is_device else 'standalone'}")

        # State preparation: consistent language, skin, favorites, recents.
        print(
            f"Setting up: locale={DEFAULT_LOCALE}, skin sync off, skin index {DEFAULT_SKIN}"
        )
        disable_skin_sync(page)
        set_skin(page, DEFAULT_SKIN)
        set_locale(page, DEFAULT_LOCALE)
        if not args.skip_prep:
            print("Preparing favorites (curated list)")
            reset_favorites(page)
            if is_device:
                print("Preparing recents (curated launch sequence — uses the TV)")
                prepare_recents(page)
            else:
                print("Skipping recents prep (standalone: launching is simulated)")

        print(f"Capturing screenshots to {output_dir}/")

        current_skin = DEFAULT_SKIN

        for p in PAGES:
            if p.get("device_only") and not is_device:
                print(f"  {p['name']}: skipped (device only)")
                continue
            url = f"{APP_URL}{p['path']}"
            wait = p["wait"]
            extra_wait = p.get("extra_wait", 2000)
            scroll = p.get("scroll", 0)
            click = p.get("click")
            click_wait = p.get("click_wait")
            desired_skin = p.get("skin", DEFAULT_SKIN)

            # Switch skin if needed
            if desired_skin != current_skin:
                print(f"  Switching skin to index {desired_skin}")
                set_skin(page, desired_skin)
                current_skin = desired_skin

            mobile_only = p.get("mobile_only", False)
            viewports = [("-mobile", MOBILE)] if mobile_only else [("", DESKTOP), ("-mobile", MOBILE)]
            for suffix, viewport in viewports:
                name = f"{p['name']}{suffix}"
                try:
                    err = capture(page, name, url, wait, extra_wait, viewport, output_dir, scroll, click, click_wait)
                    if err:
                        errors.append((name, err))
                except Exception as e:
                    print(f"FAILED: {e}")
                    errors.append((name, str(e)))

            # Restore default skin if we switched
            if desired_skin != DEFAULT_SKIN:
                set_skin(page, DEFAULT_SKIN)
                current_skin = DEFAULT_SKIN

        # Localized home shots (site language gallery), then restore locale.
        for locale, name in LOCALE_SHOTS:
            print(f"  Switching locale to {locale}")
            set_locale(page, locale)
            try:
                err = capture(page, name, f"{APP_URL}/", "main", 2000, MOBILE, output_dir)
                if err:
                    errors.append((name, err))
            except Exception as e:
                print(f"FAILED: {e}")
                errors.append((name, str(e)))
        set_locale(page, DEFAULT_LOCALE)

        browser.close()

    if errors:
        print(f"\n{len(errors)} screenshot(s) had issues:")
        for name, err in errors:
            print(f"  - {name}: {err}")
    print("Done.")


if __name__ == "__main__":
    main()
