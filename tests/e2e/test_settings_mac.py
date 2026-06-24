"""
Validates that the Settings "System" section shows Ethernet/Wi-Fi MAC rows.

The MAC rows render from `get_live_stats` (the same fetch as the IP rows),
reading hardware MACs from sysfs — so they appear whenever the adapter exists.
"""

import re

from playwright.sync_api import expect

from conftest import goto_settings

MAC_RE = re.compile(r"^([0-9a-f]{2}:){5}[0-9a-f]{2}$", re.IGNORECASE)


def _info_row_value(page, label):
    """Return the trimmed value of the .info-row whose label is `label`."""
    row = page.locator(".info-row", has=page.get_by_text(label, exact=True))
    expect(row).to_be_visible(timeout=15000)
    return row.locator(".info-value").inner_text().strip()


def test_settings_system_shows_mac_rows(page):
    goto_settings(page)

    eth = _info_row_value(page, "Ethernet MAC")
    wifi = _info_row_value(page, "Wi-Fi MAC")

    # Both rows always render; the value is a MAC, or "—" when no such adapter.
    assert MAC_RE.match(eth) or eth == "—", f"unexpected Ethernet MAC: {eth!r}"
    assert MAC_RE.match(wifi) or wifi == "—", f"unexpected Wi-Fi MAC: {wifi!r}"

    # The container has at least one real network interface, so the full
    # pipeline (sysfs -> server fn -> DTO -> hydrated row) must surface a real
    # MAC in at least one of the two rows.
    assert MAC_RE.match(eth) or MAC_RE.match(wifi), (
        f"expected a real MAC in the System section, got eth={eth!r} wifi={wifi!r}"
    )
