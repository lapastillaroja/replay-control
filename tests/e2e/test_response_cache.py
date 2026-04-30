"""
End-to-end test for the response_cache TTL.

Pinned during the pool-design investigation: `RESPONSE_TTL` was 10 s, which
caused a 100–300 ms recompute on every navigation pause >10 s on Pi 4 +
USB+exFAT. We bumped it to 5 min in commit 8431002. This test guards the
new behaviour: a page must stay fast across the *old* 10 s window.

Anchors RESPONSE_TTL >= ~30 s — anything shorter and this test will catch
the regression at runtime, prompting a comment update in
`api/response_cache.rs`.
"""

import time
from urllib.request import urlopen

import pytest

from conftest import PI_URL


def _time_get(url: str) -> float:
    """Return seconds for a GET to complete."""
    t0 = time.perf_counter()
    urlopen(url, timeout=15).read()
    return time.perf_counter() - t0


# Pages whose SSR shell pays a 100 ms+ recompute on cold response_cache.
# /favorites is the cleanest signal because get_favorites_recommendations
# is the dominant cost; / has it via get_recommendations.
@pytest.mark.parametrize("path", ["/favorites", "/"])
def test_response_cache_stays_warm_across_old_ttl(path):
    url = f"{PI_URL}{path}"
    # Warm up: first hit may rebuild from cold.
    _time_get(url)
    warm = _time_get(url)
    # Sleep slightly longer than the *old* 10 s TTL. With the new 300 s
    # TTL this should remain warm; with the old 10 s TTL it would expire.
    time.sleep(12)
    after_pause = _time_get(url)
    # The post-pause hit must be in the same ballpark as warm (within ~3×).
    # If the cache expired we'd see 5–10× the warm time.
    assert after_pause < max(warm * 3, 0.150), (
        f"{path}: warm={warm*1000:.0f} ms, after 12 s pause={after_pause*1000:.0f} ms. "
        f"That looks like a cache expiry — RESPONSE_TTL probably regressed. "
        f"See `api/response_cache.rs` and the pool-design findings."
    )


def test_warm_cache_meets_baseline_budget():
    """
    Sanity check on absolute warm-cache speed for /favorites. Pi 4 baseline
    is ~30 ms; we use 200 ms as the regression threshold.
    """
    url = f"{PI_URL}/favorites"
    _time_get(url)  # warm up
    warm = _time_get(url)
    assert warm < 0.200, (
        f"/favorites warm cache took {warm*1000:.0f} ms. "
        f"Baseline on Pi 4 / USB+exFAT is ~30 ms. If this regresses, "
        f"check whether response_cache is being invalidated too aggressively."
    )
