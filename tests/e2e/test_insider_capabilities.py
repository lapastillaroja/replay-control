"""Opt-in, read-only device checks. Never changes the token or launches a game."""
import json
import os
import re

import pytest
from playwright.sync_api import expect

from conftest import PI_URL

pytestmark = pytest.mark.skipif(
    not os.environ.get("INSIDER_CAPABILITY_TEST"),
    reason="requires a connected device with GameCube and PS2 games",
)


def open_authenticated(page, path):
    page.goto(PI_URL + path, wait_until="domcontentloaded", timeout=60000)
    page.locator(".initial-loading-shell").wait_for(state="hidden", timeout=120000)
    if "/login" in page.url:
        password = page.locator("#login-admin-password")
        password.fill(os.environ.get("PI_PASS", "replayos"))
        page.locator("form").filter(has=password).locator("button").click()
        page.wait_for_url(lambda url: "/login" not in str(url), timeout=60000)
        page.goto(PI_URL + path, wait_until="domcontentloaded")
        page.locator(".initial-loading-shell").wait_for(state="hidden", timeout=120000)


@pytest.mark.parametrize("system", ["nintendo_gc", "sony_ps2"])
@pytest.mark.parametrize("version,enabled,features,locked", [
    ("2.0.47", True, ["gamecube", "playstation2"], False),
    ("2.0.47", False, ["gamecube", "playstation2"], True),
    ("2.0.47", False, ["vulkan"], False),
    ("2.0.0", None, [], False),
    ("2.0.47", "error", [], True),
    ("1.8.0", None, [], True),
    ("1.8.0", True, ["gamecube", "playstation2"], True),
    ("unknown", True, [], True),
])
def test_launch_access_without_changing_device(page, system, version, enabled, features, locked):
    payload = {} if enabled is None else {"insider_enabled": enabled, "insider_features": features}
    payload["version"] = version
    reason = "Could not check RePlayOS capabilities" if enabled == "error" else "requires active RePlayOS Insider"
    if version == "1.8.0":
        reason = "requires RePlayOS 2.0.0 or newer"
    elif version == "unknown":
        reason = "version is unknown"
    capability_requests = []
    def intercept(route):
        if "get_replay_capabilities" in route.request.url:
            capability_requests.append(route.request.url)
            route.fulfill(status=503 if enabled == "error" else 200,
                          content_type="application/json",
                          body="{}" if enabled == "error" else json.dumps(payload))
        else:
            route.continue_()
    page.route("**/sfn/**", intercept)
    open_authenticated(page, "/games/" + system)

    page.wait_for_function("() => document.querySelector('.game-list-launch-btn') !== null")
    button = page.locator(".game-list-launch-btn").first
    if locked:
        expect(button).to_be_disabled(timeout=30000)
        expect(button).to_have_attribute("title", re.compile(reason))
    else:
        expect(button).to_be_enabled(timeout=30000)
    assert capability_requests
    # Navigate without reloading to cover async-owner context access.
    page.evaluate("window.__capabilityNavigation = true")
    page.locator(".game-list-row-link").first.click()
    launch = page.locator(".game-launch-action button.game-action-launch")
    expect(launch).to_be_visible(timeout=30000)
    if locked:
        expect(launch).to_be_disabled()
        expect(page.locator(".game-launch-action .game-action-note")).to_contain_text(reason)
    else:
        expect(launch).to_be_enabled()
    assert page.evaluate("window.__capabilityNavigation === true")


def test_insider_activation_is_separate_from_saved_token(page):
    responses = []
    page.on("response", lambda response: responses.append(response)
            if "get_replay_insider_config" in response.url and response.status == 200 else None)
    open_authenticated(page, "/settings/replayos")
    status = page.locator(".insider-activation-status")
    expect(status).to_be_visible(timeout=30000)
    assert responses, "expected the live Insider configuration response"
    enabled = responses[-1].json().get("enabled")
    expected = "Insider activation status is unavailable"
    if enabled is True:
        expected = "Insider access is active"
    elif enabled is False:
        expected = "Insider access is not active"
    expect(status).to_have_text(expected)
    expect(page.locator("#replayos-insider-token")).to_have_value("")


@pytest.mark.parametrize("version,available", [("1.8.0", False), ("2.0.0", True), ("unknown", False)])
def test_link_play_version_gate(page, version, available):
    requests = []
    def intercept(route):
        if "get_replay_capabilities" in route.request.url:
            route.fulfill(status=200, content_type="application/json", body=json.dumps({"version": version}))
        else:
            if "get_link_play_config" in route.request.url:
                requests.append(route.request.url)
            route.continue_()
    page.route("**/sfn/**", intercept)
    open_authenticated(page, "/settings/replayos")
    if available:
        expect(page.locator("#link-play-mode")).to_be_visible(timeout=30000)
    else:
        reason = "requires RePlayOS 2.0.0" if version == "1.8.0" else "version is unknown"
        expect(page.locator(".link-play-unavailable")).to_contain_text(reason, timeout=30000)
        expect(page.locator("#link-play-mode")).to_have_count(0)
        assert not requests


@pytest.mark.parametrize("expanded", [False, True])
def test_missing_playtime_is_not_zero(page, expanded):
    payload = {"availability": "Tracked", "all_seconds": 3600, "systems": []}
    if expanded:
        payload.update(session_seconds=0, week_seconds=0, games_played=0, longest_session_seconds=0)
    def intercept(route):
        if "get_library_playtime" in route.request.url:
            route.fulfill(status=200, content_type="application/json", body=json.dumps(payload))
        else:
            route.continue_()
    page.route("**/sfn/**", intercept)
    open_authenticated(page, "/settings/game-library")
    for label in ["This Session", "This Week", "Games Played", "Longest Session"]:
        card = page.locator(".stat-card").filter(has=page.get_by_text(label, exact=True))
        expect(card).to_be_visible(timeout=30000)
        if expanded:
            expect(card.locator(".stat-value")).to_have_text("0" if label == "Games Played" else "0m")
        else:
            expect(card.locator(".stat-value")).to_have_text("Not available")
