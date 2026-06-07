#!/usr/bin/env python3
"""Capture screenshots of the Replay Control app for the documentation site.

Always captures both desktop (1280x800) and mobile (375x812) for every page.
Desktop files: {name}.png, mobile files: {name}-mobile.png.

For reproducible galleries the run starts with a state-preparation step:
locale forced to English, skin sync off + default skin, favorites reset to a
curated list, and (device only) recents markers rewritten over SSH with
staggered mtimes (NOTE: this replaces the device's real play history). At
the very end one multi-disc game is launched for real so the now-playing
shots show the sticky player bar with a disc indicator — last, because the
bar renders on every page while a game runs; its recents marker is removed
again so it stays out of the Last played list.

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
import re
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
    ("nintendo_n64", "/roms/nintendo_n64/00 Clean Romset/Mario Kart 64 (USA).z64"),
    ("sega_smd", "/roms/sega_smd/00 Clean Romset/Shinobi III - Return of the Ninja Master (USA).md"),
    ("sega_smd", "/roms/sega_smd/00 Clean Romset/Rocket Knight Adventures (USA).md"),
    ("sega_dc", "/roms/sega_dc/Sonic Adventure 2 v1.008 (2001)(Sega)(PAL)(M5)[!]/Sonic Adventure 2 v1.008 (2001)(Sega)(PAL)(M5)[!].gdi"),
    ("sega_smd", "/roms/sega_smd/00 Clean Romset/Streets of Rage (World).md"),
    ("arcade_fbneo", "/roms/arcade_fbneo/Vertical/00 Clean Romset/ddpdoj.zip"),
    ("sega_smd", "/roms/sega_smd/00 Clean Romset/Atomic Runner (USA).md"),
    ("arcade_fbneo", "/roms/arcade_fbneo/Vertical/00 Clean Romset/gunlock.zip"),
    ("arcade_fbneo", "/roms/arcade_fbneo/Horizontal/00 Clean Romset/ffight.zip"),
]

# Curated recents, oldest -> newest (so the LAST entry shows first in the
# UI). Device only: the prep step writes the `_recent/` marker files directly
# over SSH with staggered mtimes — no game launches needed.
RECENTS = [
    "/roms/nintendo_n64/00 Clean Romset/Mario Kart 64 (USA).z64",
    "/roms/nintendo_snes/00 Clean Romset/Legend of Zelda, The - A Link to the Past (USA).sfc",
    "/roms/nintendo_gba/00 Clean Romset/Castlevania - Aria of Sorrow (USA).gba",
    "/roms/sega_smd/00 Clean Romset/Shinobi III - Return of the Ninja Master (USA).md",
    "/roms/ibm_pc/Indiana Jones and the Fate of Atlantis (Spanish).zip",
    "/roms/sega_smd/00 Clean Romset/Cool Spot (USA).md",
    "/roms/arcade_fbneo/Horizontal/00 Clean Romset/sfiii3.zip",
    "/roms/ibm_pc/Alone in the Dark.zip",
    "/roms/sega_smd/00 Clean Romset/Atomic Runner (USA).md",
    "/roms/sega_smd/00 Clean Romset/Teenage Mutant Ninja Turtles - The Hyperstone Heist (USA).md",
    "/roms/sega_smd/00 Clean Romset/Sonic The Hedgehog 2 (World) (Rev A).md",
    "/roms/arcade_fbneo/Vertical/00 Clean Romset/gunlock.zip",
]

# Launched for real (the now-playing shots need a live game) and multi-disc
# on purpose so the "Disc 1/3" indicator shows. Deliberately NOT part of the
# curated recents: its marker is deleted again right after the launch.
NOW_PLAYING_ROM = "/roms/sony_psx/Final Fantasy VII (Spain).m3u"
FINAL_LAUNCH_BOOT_SECS = 15  # let the game boot before now-playing shots

# SSH access for the recents-marker prep (same defaults as dev.sh).
PI_USER = os.environ.get("PI_USER", "root")
PI_PASS = os.environ.get("PI_PASS", "replayos")

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
    {"name": "metadata", "path": "/settings/metadata", "wait": "main"},
    # ── Device-only (skipped automatically in standalone mode) ──
    # RePlayOS Net Control settings: status card ("Connected to RePlayOS ...")
    {"name": "net-control", "path": "/settings/replay-net-control", "wait": ".net-control-status", "extra_wait": 3000, "device_only": True},
    # Same page with the setup sections open (via the "Re-enter code" button)
    {"name": "net-control-setup", "path": "/settings/replay-net-control", "wait": ".net-control-status", "extra_wait": 3000, "click": ".form-btn-secondary", "click_wait": ".net-control-code-input", "device_only": True},
]

# Captured LAST, after NOW_PLAYING_ROM is launched — the sticky now-playing
# bar renders on every page while a game runs, so the launch must happen
# after everything else (including the locale shots) is in the can.
NOW_PLAYING_PAGES = [
    # Home with a live game: the sticky now-playing bar (player controls +
    # "Disc 1/3" indicator from the multi-disc game)
    {"name": "home-now-playing", "path": "/", "wait": ".now-playing-bar", "extra_wait": 9000, "device_only": True},
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

# fn name -> route path, built by build_sfn_routes(). Server fn routes are
# /sfn/<snake_case_name><macro_hash>; the hash comes from the #[server]
# macro and changes between builds, so the paths can't be hardcoded.
_sfn_routes = {}


def build_sfn_routes(page):
    """Map server fn names to routes by extracting the /sfn/... strings from
    the served wasm bundle — it always matches the running server build."""
    html = page.request.get(f"{APP_URL}/").text()
    match = re.search(r'"(/static/pkg/[^"]+\.wasm)"', html)
    if not match:
        sys.exit("Error: could not find the wasm bundle URL in the app HTML")
    wasm = page.request.get(f"{APP_URL}{match.group(1)}").body()
    for route_bytes in set(re.findall(rb"/sfn/[a-z_0-9]+", wasm)):
        route = route_bytes.decode()
        name = re.sub(r"\d+$", "", route.rsplit("/", 1)[1])
        # If both a bare and a hashed spelling surface for a name, the hashed
        # (longer) one is the registered route.
        if len(route) > len(_sfn_routes.get(name, "")):
            _sfn_routes[name] = route
    print(f"  resolved {len(_sfn_routes)} server fn routes from the wasm bundle")


def sfn(page, name, data="", accept_json=False):
    """POST a Leptos server function by fn name (e.g. "set_skin")."""
    route = _sfn_routes.get(name)
    if route is None:
        sys.exit(f"Error: no server fn route found for '{name}' — renamed?")
    headers = dict(SFN_HEADERS)
    if accept_json:
        headers["Accept"] = "application/json"
    resp = page.request.post(f"{APP_URL}{route}", headers=headers, data=data)
    if not resp.ok:
        print(f"  WARNING: {name}({data!r}) returned {resp.status}")
    return resp


def set_skin(page, index):
    """Set the active skin via server function POST."""
    sfn(page, "set_skin", f"index={index}")


def disable_skin_sync(page):
    """Disable skin sync so the skin stays fixed during the run."""
    sfn(page, "set_skin_sync", "enabled=false")


def set_locale(page, locale):
    """Force the UI language so the gallery is consistent."""
    sfn(page, "save_locale", f"locale={locale}")


def detect_device_mode(page):
    """True when the target app runs on the RePlayOS device."""
    resp = sfn(page, "get_mode", accept_json=True)
    return resp.ok and "device" in resp.text().lower()


def reset_favorites(page):
    """Replace whatever favorites exist with the curated list."""
    import json
    from urllib.parse import quote

    resp = sfn(page, "get_favorites", accept_json=True)
    if resp.ok:
        try:
            existing = json.loads(resp.text())
        except json.JSONDecodeError:
            existing = []
        markers = [
            (f["marker_filename"], f.get("subfolder", ""))
            for f in existing
            if isinstance(f, dict) and f.get("marker_filename")
        ]
        for marker, subfolder in markers:
            data = f"filename={quote(marker)}"
            if subfolder:
                data += f"&subfolder={quote(subfolder)}"
            sfn(page, "remove_favorite", data)
        if markers:
            print(f"  cleared {len(markers)} existing favorite(s)")

    added = 0
    # Newest-added shows first in the UI: add in reverse so FAVORITES[0]
    # ends up displayed first. date_added has SECONDS resolution — without
    # the sleep all adds tie and the display order is readdir luck.
    for index, (system, rom_path) in enumerate(reversed(FAVORITES)):
        if index:
            page.wait_for_timeout(1100)
        resp = sfn(
            page,
            "add_favorite",
            f"system={quote(system)}&rom_path={quote(rom_path)}&grouped=false",
        )
        if resp.ok:
            added += 1
        else:
            print(f"  WARNING: could not favorite {rom_path} (missing from library?)")
    print(f"  set {added}/{len(FAVORITES)} curated favorites")


def ssh_run(script):
    """Run a shell script on the Pi (password auth via SSH_ASKPASS, same
    pattern as dev.sh). The target host comes from APP_URL."""
    import subprocess
    import tempfile

    host = re.sub(r"^https?://", "", APP_URL).split("/")[0].split(":")[0]
    askpass = tempfile.NamedTemporaryFile("w", suffix=".sh", delete=False)
    askpass.write(f"#!/bin/sh\necho '{PI_PASS}'\n")
    askpass.close()
    os.chmod(askpass.name, 0o700)
    env = dict(os.environ, SSH_ASKPASS=askpass.name, SSH_ASKPASS_REQUIRE="force")
    try:
        result = subprocess.run(
            ["ssh", "-o", "StrictHostKeyChecking=no", f"{PI_USER}@{host}", "sh -s"],
            input=script,
            text=True,
            capture_output=True,
            env=env,
            timeout=60,
        )
    finally:
        os.unlink(askpass.name)
    if result.returncode != 0:
        print(f"  WARNING: ssh command failed: {result.stderr.strip()}")
    return result.returncode == 0


def recent_marker_name(rom_path):
    """`<system>@<rom_filename>.rec` — the marker format the app writes."""
    system = rom_path.split("/")[2]
    return f"{system}@{rom_path.rsplit('/', 1)[-1]}.rec"


def recents_dir(page):
    """The active storage's `_recent/` directory, via get_info."""
    import json

    resp = sfn(page, "get_info", accept_json=True)
    storage_root = json.loads(resp.text()).get("storage_root", "") if resp.ok else ""
    return f"{storage_root}/roms/_recent" if storage_root else None


def recents_marker_script(recent_dir, now):
    """Shell script that replaces `_recent/` with the curated markers (same
    format RePlayOS writes on launch) with staggered mtimes, oldest first.
    NOTE: running it wipes the device's real play history."""
    lines = [f"mkdir -p '{recent_dir}'", f"rm -f '{recent_dir}'/*.rec"]
    for i, rom_path in enumerate(RECENTS):
        marker = f"{recent_dir}/{recent_marker_name(rom_path)}"
        mtime = now - 60 * (len(RECENTS) - i)
        lines.append(f"printf '%s\\n' '{rom_path}' > '{marker}'")
        lines.append(f"touch -d @{mtime} '{marker}'")
    return "\n".join(lines)


def prepare_recents(page):
    """Device only: make 'Last played' deterministic by writing the curated
    recents markers over SSH."""
    import time

    recent_dir = recents_dir(page)
    if not recent_dir:
        print("  WARNING: no storage_root from get_info; skipping recents prep")
        return

    if not ssh_run(recents_marker_script(recent_dir, int(time.time()))):
        # A gallery captured with stale recents is worse than no gallery —
        # bail instead of burning a full capture run.
        sys.exit("Error: recents markers not written (SSH to the Pi failed)")
    print(f"  wrote {len(RECENTS)} recents markers")


def restart_app(page):
    """The app can't see SSH-side marker writes (no filesystem watcher on
    NFS storage), so its recents cache is stale after the prep. Restart it
    for a cold, correct cache before any page is captured."""
    print("  restarting replay-control to drop stale caches ...", flush=True)
    if not ssh_run("systemctl restart replay-control"):
        sys.exit("Error: could not restart replay-control after recents prep")
    wait_for_app(page)


def game_is_running(page):
    """True when the sticky now-playing bar renders (a game is live).
    SSE-driven, so give the hydrated page a moment to receive the state."""
    page.goto(f"{APP_URL}/", wait_until="load", timeout=30000)
    page.wait_for_timeout(8000)
    return page.query_selector(".now-playing-bar") is not None


def wait_for_sfn(page, name, timeout_secs=120):
    """Poll a server fn until it answers (app back up after a restart)."""
    import time

    deadline = time.time() + timeout_secs
    while time.time() < deadline:
        try:
            resp = page.request.post(
                f"{APP_URL}{_sfn_routes[name]}", headers=SFN_HEADERS, data="", timeout=5000
            )
            if resp.ok:
                return
        except Exception:
            pass
        page.wait_for_timeout(2000)
    sys.exit(f"Error: app did not answer {name} within {timeout_secs}s")


def wait_for_app(page, timeout_secs=120):
    """Poll until the app serves the full shell again (post-restart), then
    wait out the startup library scan so the orange activity banner doesn't
    photobomb the first captures."""
    import time

    deadline = time.time() + timeout_secs
    while time.time() < deadline:
        try:
            html = page.request.get(f"{APP_URL}/", timeout=5000).text()
            if "static/pkg" in html:
                break
        except Exception:
            pass
        page.wait_for_timeout(2000)
    else:
        sys.exit(f"Error: app did not come back within {timeout_secs}s")

    # The activity banner is SSE-driven; watch it from a real page. It may
    # take a few seconds to appear after restart, so wait through a short
    # grace period before trusting its absence.
    page.goto(f"{APP_URL}/", wait_until="load", timeout=30000)
    page.wait_for_timeout(8000)
    try:
        page.wait_for_selector(".metadata-busy-banner", state="detached", timeout=240000)
    except PlaywrightTimeout:
        print("  WARNING: library scan still running; captures may show the banner")


def launch_now_playing(page):
    """Launch NOW_PLAYING_ROM for the now-playing shots, then drop the
    marker RePlayOS creates for it so it stays out of the recents list.
    Returns True if the game is running."""
    from urllib.parse import quote

    # The marker delete below is invisible to the app (no watcher on NFS),
    # so nothing may repopulate the recents cache between the launch (which
    # empties it) and the delete. Park the page on a blank tab and do NOT
    # follow the launch redirect to "/" — its SSR would re-read recents
    # while the marker still exists.
    page.goto("about:blank")
    name = NOW_PLAYING_ROM.rsplit("/", 1)[-1]
    print(f"  launching {name} (for now-playing shots; kept out of recents) ...", flush=True)
    resp = page.request.post(
        f"{APP_URL}{_sfn_routes['launch_game']}",
        headers=SFN_HEADERS,
        data=f"rom_path={quote(NOW_PLAYING_ROM)}&return_to=",
        max_redirects=0,
    )
    if resp.status >= 400:
        print(f"  WARNING: launch failed ({resp.status}) — now-playing shots will be skipped")
        return False
    # Delete after the boot wait — RePlayOS may write the marker async
    # during boot. The cache stays empty meanwhile: the page is parked on
    # about:blank and the redirect wasn't followed, so nothing may SSR a
    # recents read before the marker is gone.
    page.wait_for_timeout(FINAL_LAUNCH_BOOT_SECS * 1000)
    # The app has been observed dying right after heavy-core launches
    # (silently killed; systemd restarts it within seconds). Poll a cheap
    # endpoint that does NOT read recents until it's back.
    wait_for_sfn(page, "get_mode")
    recent_dir = recents_dir(page)
    if not (recent_dir and ssh_run(f"rm -f '{recent_dir}/{recent_marker_name(NOW_PLAYING_ROM)}'")):
        # Without the delete, "Last played" would show the now-playing game —
        # skip the shots rather than capture wrong ones. (sshd has been seen
        # dying around heavy-core launches; a reboot brings it back.)
        print("  WARNING: marker cleanup failed — skipping now-playing shots")
        return False
    # Now that the marker is gone, the full readiness check (which loads the
    # home page) re-seeds the recents cache fresh — and waits out a startup
    # scan if the app did get restarted.
    wait_for_app(page)
    return True


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


def capture_pages(page, pages, is_device, output_dir, errors):
    """Capture a page list in both viewports, honoring per-page skin/flags."""
    current_skin = DEFAULT_SKIN

    for p in pages:
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

        build_sfn_routes(page)
        is_device = detect_device_mode(page)
        print(f"Target mode: {'device' if is_device else 'standalone'}")

        # Precondition: no game may be running, or the sticky player bar
        # photobombs every shot (RePlayOS has no stop command — only the
        # final now-playing captures want a live game).
        if is_device and game_is_running(page):
            sys.exit(
                "Error: a game is running on the TV — the player bar would appear "
                "in every shot. Reboot RePlayOS (Settings page or the TV) and rerun."
            )

        # State preparation: consistent language, skin, favorites, recents.
        print(
            f"Setting up: locale={DEFAULT_LOCALE}, skin sync off, skin index {DEFAULT_SKIN}"
        )
        disable_skin_sync(page)
        set_skin(page, DEFAULT_SKIN)
        set_locale(page, DEFAULT_LOCALE)
        if not args.skip_prep:
            # SSH work first: sshd has been seen dying within minutes of
            # boot on the dev Pi — grab it while it's alive. An external
            # orchestrator can do the SSH prep itself right after boot and
            # set REPLAY_SHOTS_RECENTS_PREPPED=1 to skip it here.
            externally_prepped = os.environ.get("REPLAY_SHOTS_RECENTS_PREPPED") == "1"
            if is_device and not externally_prepped:
                print("Preparing recents (writing markers over SSH)")
                prepare_recents(page)
            elif not is_device:
                print("Skipping recents prep (standalone: no recents markers)")
            print("Preparing favorites (curated list)")
            reset_favorites(page)
            if is_device:
                if externally_prepped:
                    # The external restart already happened; still wait out
                    # the startup scan so the banner stays out of the shots.
                    wait_for_app(page)
                else:
                    restart_app(page)

        print(f"Capturing screenshots to {output_dir}/")

        capture_pages(page, PAGES, is_device, output_dir, errors)

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

        # Now-playing shots last: once the game launches, the sticky bar
        # renders on every page, so nothing else may be captured after this.
        if is_device and launch_now_playing(page):
            capture_pages(page, NOW_PLAYING_PAGES, is_device, output_dir, errors)

        browser.close()

    if errors:
        print(f"\n{len(errors)} screenshot(s) had issues:")
        for name, err in errors:
            print(f"  - {name}: {err}")
    print("Done.")


if __name__ == "__main__":
    main()
