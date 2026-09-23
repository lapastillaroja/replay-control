"""Opt-in device test against an already running game; never launches or saves.
Set ACTIVE_GAME_DETAIL_PATH to its /games/<system>/<encoded-filename> path.
Uses the English UI and a game with inherited horizontal position and room to step.
Apply is intercepted in the browser, so no override reaches RePlayOS.
"""
import os
import pytest
from playwright.sync_api import expect
from conftest import PI_URL

@pytest.mark.skipif(not os.environ.get("ACTIVE_GAME_DETAIL_PATH"), reason="requires an already running game")
def test_override_steps_hold_and_explicit_apply(page):
    base = PI_URL
    detail_path = os.environ["ACTIVE_GAME_DETAIL_PATH"]
    errors = []
    writes = []
    page.on('pageerror', lambda error: errors.append(str(error)))
    def intercept(route):
        if 'save_active_game_config' in route.request.url.lower():
            writes.append(route.request.post_data)
            route.fulfill(status=200, content_type='application/json', body='"Applied"')
        else:
            route.continue_()
    page.route('**/sfn/**', intercept)
    page.goto(base + '/settings/replayos', wait_until='domcontentloaded', timeout=60000)
    page.locator('.initial-loading-shell').wait_for(state='hidden', timeout=120000)
    if '/login' in page.url:
        page.locator('#login-admin-password').fill(os.environ.get('PI_PASS','replayos'))
        page.locator('form').filter(has=page.locator('#login-admin-password')).locator('button').click()
        page.wait_for_url(lambda url: '/login' not in str(url), timeout=60000)
        page.goto(base+'/settings/replayos',wait_until='domcontentloaded')
        page.locator('.initial-loading-shell').wait_for(state='hidden', timeout=120000)
    expect(page.locator('#link-play-mode')).to_be_visible(timeout=30000)
    expect(page.locator('#replayos-insider-token')).to_be_visible()
    # Exercise the async component owners on client-side navigation too.
    page.evaluate('window.__settingsNavigationCheck = true')
    page.locator('a.back-btn[href="/settings"]').click()
    page.locator('a[href="/settings/replayos"]').click()
    expect(page.locator('#link-play-mode')).to_be_visible(timeout=30000)
    expect(page.locator('#replayos-insider-token')).to_be_visible()
    assert page.evaluate('window.__settingsNavigationCheck === true')
    # Read-only navigation to the game already running; no launch or API writes.
    page.goto(base+detail_path,wait_until='domcontentloaded',timeout=60000)
    page.locator('.initial-loading-shell').wait_for(state='hidden', timeout=120000)
    expect(page.locator('.game-override-row')).to_have_count(7,timeout=30000)
    expect(page.locator('.game-override-row button')).to_have_count(14)
    row = page.locator('.game-override-row').nth(4)
    plus = row.locator('.step-control').nth(1)
    expect(plus).to_be_enabled(timeout=30000)
    plus.click()
    first_value = int(row.locator('.compact-control-value').inner_text())
    assert not writes, 'adjustment must not apply automatically'
    plus.hover()
    page.mouse.down()
    page.wait_for_timeout(750)  # intentional hold duration, not hydration readiness
    page.mouse.up()
    held = int(row.locator('.compact-control-value').inner_text())
    assert held > first_value + 2, held
    page.wait_for_timeout(200)
    assert int(row.locator('.compact-control-value').inner_text()) == held
    plus.dispatch_event('pointerdown', {'button':0,'isPrimary':True,'pointerId':1})
    plus.dispatch_event('pointercancel', {'button':0,'isPrimary':True,'pointerId':1})
    plus.focus()
    page.keyboard.press('Space')
    expect(row.locator('.compact-control-value')).to_have_text(f'{held + 4:+}')
    assert not writes
    page.locator('.active-game-settings-apply').click()
    expect(page.get_by_text('Game settings applied',exact=True)).to_be_visible(timeout=15000)
    assert len(writes) == 1
    assert 'identity' in writes[0]
    assert not errors, errors
