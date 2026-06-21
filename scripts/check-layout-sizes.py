#!/usr/bin/env python3
"""Capture a small viewport matrix for layout spot-checks.

This is an ad hoc validation helper, not a gallery generator. It loads a
small set of core pages at several common screen sizes and writes screenshots
to a local directory so the layouts can be inspected side by side.

Usage:
    python scripts/check-layout-sizes.py
    APP_URL=https://192.168.10.30:8443 python scripts/check-layout-sizes.py
    python scripts/check-layout-sizes.py --page settings --viewport 1440x900
"""

from __future__ import annotations

import argparse
import os
import sys
from pathlib import Path

from playwright.sync_api import sync_playwright, TimeoutError as PlaywrightTimeout

APP_URL = os.environ.get("APP_URL", "http://localhost:8091")

PAGES = [
    ("home", "/"),
    ("search", "/search?q=mario"),
    ("favorites", "/favorites"),
    ("settings", "/settings"),
    ("metadata", "/settings/metadata"),
]

DEFAULT_VIEWPORTS = [
    ("mobile", {"width": 375, "height": 812}),
    ("tablet", {"width": 768, "height": 1024}),
    ("desktop", {"width": 1280, "height": 800}),
    ("wide", {"width": 1600, "height": 900}),
]


def parse_viewport(spec: str) -> tuple[str, dict[str, int]]:
    try:
        width_str, height_str = spec.lower().split("x", 1)
        width = int(width_str)
        height = int(height_str)
    except ValueError as exc:
        raise argparse.ArgumentTypeError(f"invalid viewport '{spec}' (expected WxH)") from exc
    label = f"{width}x{height}"
    return label, {"width": width, "height": height}


def main() -> int:
    parser = argparse.ArgumentParser(description="Capture layout screenshots at multiple viewport sizes")
    parser.add_argument(
        "--output-dir",
        default="tmp/layout-checks",
        help="Directory for screenshots (default: tmp/layout-checks/)",
    )
    parser.add_argument(
        "--page",
        action="append",
        default=[],
        help="Page name to capture (repeatable). Defaults to a small core set.",
    )
    parser.add_argument(
        "--viewport",
        action="append",
        type=parse_viewport,
        default=[],
        help="Viewport spec WxH (repeatable). Defaults to mobile/tablet/desktop/wide.",
    )
    args = parser.parse_args()

    selected_pages = {name for name, _ in PAGES if not args.page or name in args.page}
    pages = [(name, path) for name, path in PAGES if name in selected_pages]
    if not pages:
        print("No pages selected.")
        return 1

    viewports = args.viewport or DEFAULT_VIEWPORTS

    project_root = Path(__file__).resolve().parent.parent
    output_dir = project_root / args.output_dir
    output_dir.mkdir(parents=True, exist_ok=True)

    with sync_playwright() as pw:
        browser = pw.chromium.launch()
        context = browser.new_context(ignore_https_errors=True)
        page = context.new_page()
        try:
            page.goto(APP_URL, timeout=5000)
        except (PlaywrightTimeout, Exception) as exc:
            print(f"Error: cannot reach {APP_URL} ({exc})")
            context.close()
            browser.close()
            return 1

        for page_name, path in pages:
            url = f"{APP_URL}{path}"
            for viewport_name, viewport in viewports:
                page.set_viewport_size(viewport)
                print(f"{page_name} @ {viewport_name}: {path}")
                page.goto(url, wait_until="load", timeout=30000)
                page.wait_for_timeout(2500)
                out_path = output_dir / f"{page_name}-{viewport_name}.png"
                page.screenshot(path=str(out_path), full_page=False)

        context.close()
        browser.close()

    print(f"Saved screenshots to {output_dir}/")
    return 0


if __name__ == "__main__":
    sys.exit(main())
