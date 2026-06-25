"""
End-to-end coverage for global search.

Search queries the (scanned) library, so a seeded ROM is findable by a fragment
of its filename even with no catalog match. One browser session checks both the
hit and the no-results states to stay fast. Container only; the `page` fixture
also asserts no JS console errors.
"""

import pytest
from playwright.sync_api import expect

from conftest import CONTAINER, goto_hydrated

pytestmark = pytest.mark.skipif(
    not CONTAINER,
    reason="search e2e needs a seeded library and is container only",
)


def test_search_finds_seeded_game_and_handles_no_results(page, seeded_game):
    # "E2E Seed Game.nes" -> display name "E2E Seed Game"; findable by "Seed".
    goto_hydrated(page, "/search?q=Seed")
    expect(page.locator(".search-group")).to_have_count(1, timeout=15000)
    # Assert the result group contains the game's display name (the per-result
    # name span can be visually hidden until interaction, so check text content).
    expect(page.locator(".search-group")).to_contain_text("E2E Seed Game", timeout=10000)

    # A query that matches nothing renders the empty state, not a stale result.
    goto_hydrated(page, "/search?q=zzqqnomatchqq")
    expect(page.locator("p.empty-state")).to_be_visible(timeout=10000)
    expect(page.locator(".search-group")).to_have_count(0)
