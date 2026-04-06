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
    {"name": "home", "path": "/", "wait": "main"},
    {"name": "system-megadrive", "path": "/systems/sega_smd", "wait": ".content", "extra_wait": 8000},
    {"name": "detail-sonic2", "path": "/game/sega_smd/Sonic%20The%20Hedgehog%202%20(World).md", "wait": ".content", "extra_wait": 8000},
    {"name": "search-zelda", "path": "/search?q=zelda", "wait": ".content", "extra_wait": 8000},
    {"name": "search-capcom", "path": "/search?q=capcom", "wait": ".content", "extra_wait": 8000},
    {"name": "search-mario", "path": "/search?q=mario", "wait": ".content", "extra_wait": 8000},
    {"name": "favorites", "path": "/favorites", "wait": "main"},
    {"name": "more-page", "path": "/more", "wait": "main"},
    {"name": "more-settings", "path": "/more", "wait": "main"},
    {"name": "metadata", "path": "/metadata", "wait": "main"},
]


def capture(page, name, url, wait, extra_wait, viewport, output_dir):
    page.set_viewport_size(viewport)
    print(f"  {name}: {url} ...", end=" ", flush=True)
    page.goto(url, wait_until="load", timeout=30000)
    page.wait_for_selector(wait, timeout=30000)
    page.wait_for_timeout(extra_wait)
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

            try:
                capture(page, p["name"], url, wait, extra_wait, DESKTOP, output_dir)
            except Exception as e:
                print(f"FAILED: {e}")

            try:
                capture(page, f"{p['name']}-mobile", url, wait, extra_wait, MOBILE, output_dir)
            except Exception as e:
                print(f"FAILED: {e}")

        browser.close()
        print("Done.")


if __name__ == "__main__":
    main()
