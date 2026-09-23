"""Check the status-row layout without requiring a connected RePlayOS device."""

from pathlib import Path

import pytest


@pytest.mark.parametrize("width", [375, 1280])
def test_net_control_indicator_has_text_spacing(browser, width):
    styles = Path(__file__).resolve().parents[2] / "replay-control-app" / "style"
    page = browser.new_page(viewport={"width": width, "height": 720})
    try:
        page.set_content(
            '<div class="net-control-status net-control-status--ok">'
            '<span class="net-control-status-dot"></span>'
            '<span>Connected to RePlayOS</span></div>'
        )
        page.add_style_tag(content="\n".join(
            path.read_text() for path in sorted(styles.glob("_*.css"))
        ))
        row = page.locator(".net-control-status")
        assert row.evaluate("el => getComputedStyle(el).display") == "flex"
        dot = row.locator(".net-control-status-dot").bounding_box()
        label = row.locator("span").nth(1).bounding_box()
        assert dot is not None and label is not None
        assert label["x"] - (dot["x"] + dot["width"]) >= 8
        assert abs((dot["y"] + dot["height"] / 2)
                   - (label["y"] + label["height"] / 2)) < 1
    finally:
        page.close()


@pytest.mark.parametrize("width,expected_gap", [(375, "8px"), (1280, "24px")])
def test_info_rows_use_mobile_spacing(browser, width, expected_gap):
    styles = Path(__file__).resolve().parents[2] / "replay-control-app" / "style"
    page = browser.new_page(viewport={"width": width, "height": 720})
    try:
        page.set_content('<div class="info-grid"><div class="info-row"><span>Label</span><span>Value</span></div></div>')
        page.add_style_tag(content="\n".join(path.read_text() for path in sorted(styles.glob("_*.css"))))
        assert page.locator(".info-row").evaluate("el => getComputedStyle(el).columnGap") == expected_gap
        assert page.locator(".info-grid").evaluate("el => getComputedStyle(el).padding") == "0px"
    finally:
        page.close()


@pytest.mark.parametrize("width", [320, 375, 540, 768, 1280])
@pytest.mark.parametrize("label", ["Horizontal position", "Posición horizontal", "水平方向の画面位置"])
def test_compact_override_controls_fit(browser, width, label):
    styles = Path(__file__).resolve().parents[2] / "replay-control-app" / "style"
    page = browser.new_page(viewport={"width": width, "height": 720})
    try:
        page.set_content('''<div class="page game-detail"><div class="active-game-settings-panel"><div class="active-game-overrides-grid">
          <div class="game-override-row"><div class="game-override-summary"><span class="compact-control-label"></span><span class="compact-control-value">Predeterminado</span></div>
          <div class="game-override-stepper"><button class="compact-control compact-control-icon step-control">−</button><button class="compact-control compact-control-icon step-control">+</button></div>
          </div></div></div></div>''')
        page.locator(".compact-control-label").evaluate("(el, text) => el.textContent = text", label)
        page.add_style_tag(content="\n".join(path.read_text() for path in sorted(styles.glob("_*.css"))))
        assert page.evaluate("document.documentElement.scrollWidth <= innerWidth")
        row = page.locator(".game-override-row").bounding_box()
        summary = page.locator(".game-override-summary").bounding_box()
        stepper = page.locator(".game-override-stepper").bounding_box()
        assert stepper["x"] >= summary["x"] + summary["width"]
        assert abs((summary["y"] + summary["height"] / 2)
                   - (stepper["y"] + stepper["height"] / 2)) < 1
        for button in page.locator("button").all():
            box = button.bounding_box()
            assert box["height"] >= 44
            assert box["x"] + box["width"] <= row["x"] + row["width"] + 1
    finally:
        page.close()
