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

PAGES = [
    # Home & navigation
    {"name": "home", "path": "/", "wait": "main"},
    # System browser
    {"name": "system-megadrive", "path": "/games/sega_smd", "wait": ".content", "extra_wait": 8000},
    # Game detail
    {"name": "detail-sonic2", "path": "/games/sega_smd/Sonic%20The%20Hedgehog%202%20(World)%20(Rev%20A).md", "wait": ".content", "extra_wait": 8000},
    # Game detail scrolled to info card
    {"name": "detail-info", "path": "/games/sega_smd/Sonic%20The%20Hedgehog%202%20(World)%20(Rev%20A).md", "wait": ".content", "extra_wait": 8000, "scroll": 350},
    # Search
    {"name": "search-zelda", "path": "/search?q=zelda", "wait": ".content", "extra_wait": 8000},
    {"name": "search-capcom", "path": "/search?q=capcom", "wait": ".content", "extra_wait": 8000},
    {"name": "search-mario", "path": "/search?q=mario", "wait": ".content", "extra_wait": 8000},
    # Favorites
    {"name": "favorites", "path": "/favorites", "wait": "main"},
    # Favorites scrolled to stats + organize button
    {"name": "favorites-stats", "path": "/favorites", "wait": "main", "scroll": 400},
    # Favorites grouped by system view
    {"name": "favorites-grouped", "path": "/favorites", "wait": "main", "scroll": 1400},
    # Home page recommendations (scrolled)
    {"name": "recommendations", "path": "/", "wait": "main", "scroll": 500},
    # More / settings page
    {"name": "more-page", "path": "/more", "wait": "main"},
    # Skin selection page
    {"name": "skins-page", "path": "/more/skin", "wait": "main", "extra_wait": 5000},
    # Metadata page
    {"name": "metadata", "path": "/metadata", "wait": "main"},
]


def capture(page, name, url, wait, extra_wait, viewport, output_dir, scroll=0):
    page.set_viewport_size(viewport)
    print(f"  {name}: {url} ...", end=" ", flush=True)
    page.goto(url, wait_until="load", timeout=30000)
    page.wait_for_selector(wait, timeout=30000)
    page.wait_for_timeout(extra_wait)
    if scroll:
        page.evaluate(f"window.scrollBy(0, {scroll})")
        page.wait_for_timeout(1500)
    page.screenshot(path=str(output_dir / f"{name}.png"), full_page=False)
    print("ok")


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

        print(f"Capturing screenshots to {output_dir}/")

        for p in PAGES:
            url = f"{APP_URL}{p['path']}"
            wait = p["wait"]
            extra_wait = p.get("extra_wait", 2000)
            scroll = p.get("scroll", 0)

            try:
                capture(page, p["name"], url, wait, extra_wait, DESKTOP, output_dir, scroll)
            except Exception as e:
                print(f"FAILED: {e}")

            try:
                capture(page, f"{p['name']}-mobile", url, wait, extra_wait, MOBILE, output_dir, scroll)
            except Exception as e:
                print(f"FAILED: {e}")

        browser.close()
        print("Done.")


if __name__ == "__main__":
    main()
