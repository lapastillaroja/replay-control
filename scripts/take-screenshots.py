#!/usr/bin/env python3
"""Capture screenshots of the Replay Control app for the documentation site.

Always captures both desktop (1280x800) and mobile (375x812) for every page.
Desktop files: {name}.png, mobile files: {name}-mobile.png.

Usage:
    python scripts/take-screenshots.py
    APP_URL=http://replay.local:8080 python scripts/take-screenshots.py
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
]

MAX_RETRIES = 2


SFN_HEADERS = {
    "Content-Type": "application/x-www-form-urlencoded",
    "Accept": "application/x-www-form-urlencoded",
}


def set_skin(page, index):
    """Set the active skin via server function POST."""
    resp = page.request.post(
        f"{APP_URL}/api/set_skin",
        headers=SFN_HEADERS,
        data=f"index={index}",
    )
    if not resp.ok:
        print(f"  WARNING: set_skin({index}) returned {resp.status}")


def disable_skin_sync(page):
    """Disable skin sync so the skin stays fixed during the run."""
    resp = page.request.post(
        f"{APP_URL}/api/set_skin_sync",
        headers=SFN_HEADERS,
        data="enabled=false",
    )
    if not resp.ok:
        print(f"  WARNING: set_skin_sync(false) returned {resp.status}")


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

        # Disable skin sync and set default skin before capturing
        print(f"Setting up: disable skin sync, set default skin (index {DEFAULT_SKIN})")
        disable_skin_sync(page)
        set_skin(page, DEFAULT_SKIN)

        print(f"Capturing screenshots to {output_dir}/")

        current_skin = DEFAULT_SKIN

        for p in PAGES:
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

        browser.close()

    if errors:
        print(f"\n{len(errors)} screenshot(s) had issues:")
        for name, err in errors:
            print(f"  - {name}: {err}")
    print("Done.")


if __name__ == "__main__":
    main()
