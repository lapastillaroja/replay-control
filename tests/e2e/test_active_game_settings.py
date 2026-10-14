"""Opt-in device test against an already running game; never launches or saves.
Set ACTIVE_GAME_DETAIL_PATH to its /games/<system>/<encoded-filename> path.
Uses the English UI and a game with inherited horizontal position and room to step.
Apply is intercepted in the browser, so no override reaches RePlayOS.
"""
import os
import re
import json
from urllib.parse import parse_qs
import pytest
from playwright.sync_api import expect
from conftest import PI_URL

@pytest.mark.skipif(not os.environ.get("ACTIVE_GAME_DETAIL_PATH"), reason="requires an already running game")
def test_override_steps_hold_and_explicit_apply(page):
    base = PI_URL
    detail_path = os.environ["ACTIVE_GAME_DETAIL_PATH"]
    errors = []
    writes = []
    removals = []
    saved = {'game': False, 'system': False}
    page.on('pageerror', lambda error: errors.append(str(error)))
    def intercept(route):
        url = route.request.url.lower()
        if 'get_active_override_configs' in url:
            response = route.fetch()
            config = response.json()
            config['game']['configured'] = saved['game']
            config['system']['configured'] = saved['system']
            system_values = {entry['setting']: entry for entry in config['system']['overrides']}
            system_volume = system_values['Volume']
            system_volume['value'] = None
            if system_volume['global_default'] is None:
                system_volume['global_default'] = 5
            system_volume['inherited'] = system_volume['global_default']
            for entry in config['game']['overrides']:
                system = system_values[entry['setting']]
                entry['value'] = None
                if entry['setting'] == 'Volume':
                    entry['global_default'] = system_volume['global_default']
                entry['inherited'] = system['value'] if system['value'] is not None else system['global_default']
            route.fulfill(response=response, body=json.dumps(config))
        elif 'save_active_override_config' in url:
            writes.append(route.request.post_data)
            if 'System' in route.request.post_data:
                saved['system'] = True
            if 'Game' in route.request.post_data:
                saved['game'] = True
            route.fulfill(status=200, content_type='application/json', body='"Applied"')
        elif 'remove_active_override_settings' in url:
            removals.append(route.request.post_data)
            if 'System' in route.request.post_data:
                saved['system'] = False
            if 'Game' in route.request.post_data:
                saved['game'] = False
            route.fulfill(status=200, content_type='application/json', body='true')
        else:
            route.continue_()
    page.route('**/sfn/**', intercept)
    page.goto(base + '/settings/replayos', wait_until='domcontentloaded', timeout=60000)
    page.locator('.initial-loading-shell').wait_for(state='hidden', timeout=120000)
    if '/login' in page.url:
        page.locator('#login-admin-password').fill(os.environ.get('PI_PASS','replayos'))
        page.locator('form').filter(has=page.locator('#login-admin-password')).locator('button').click()
        expect(page).not_to_have_url(re.compile(r'/login(?:\?|$)'), timeout=60000)
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
    visible_rows = page.locator('.active-game-settings-scope-content:not([hidden]) .game-override-row')
    visible_apply = page.locator('.active-game-settings-scope-content:not([hidden]) .active-game-settings-apply')
    expect(visible_rows.first).to_be_visible(timeout=30000)
    page.set_viewport_size({'width': 390, 'height': 844})
    expect(visible_apply.locator('svg')).to_be_visible()
    expect(visible_apply.locator('.active-game-settings-action-label')).to_be_hidden()
    page.set_viewport_size({'width': 1280, 'height': 800})
    expect(visible_apply.locator('.active-game-settings-action-label')).to_be_visible()
    row_count = visible_rows.count()
    assert row_count in (5, 7), row_count
    volume = visible_rows.filter(has=page.get_by_text('Volume', exact=True))
    expect(volume.locator('.compact-control-value')).to_have_text('Default')
    if row_count == 5:
        expect(page.get_by_text('Integer scale (LCD only)')).to_have_count(0)
        expect(page.get_by_text('Video filter (LCD only)')).to_have_count(0)
    else:
        expect(visible_rows.get_by_text('Integer scale (LCD only)')).to_be_visible()
        expect(visible_rows.get_by_text('Video filter (LCD only)')).to_be_visible()
    monitor_x = visible_rows.filter(has=page.get_by_text('Horizontal position', exact=True))
    page.get_by_role('button', name='System', exact=True).click()
    expect(page.get_by_role('button', name='System', exact=True)).to_have_class(re.compile(r'\bis-active\b'))
    expect(visible_apply).to_have_text('Apply')
    expect(page.locator('.active-game-settings-scope-content:not([hidden]) .active-game-settings-reset')).not_to_have_class(re.compile(r'\bactive-game-settings-reset-saved\b'))
    expect(visible_rows).to_have_count(row_count)
    monitor_x.locator('.step-control').nth(1).click()
    system_value = int(monitor_x.locator('.compact-control-value').inner_text())
    visible_apply.click()
    expect(page.get_by_text('System settings saved', exact=True)).to_be_visible(timeout=15000)
    expect(page.locator('.active-game-settings-scope-content:not([hidden]) .active-game-settings-reset')).to_have_class(re.compile(r'\bactive-game-settings-reset-saved\b'))
    expect(page.locator('.active-game-settings-scope-content:not([hidden]) .active-game-settings-remove')).to_have_text('Remove System Settings')
    assert len(writes) == 1
    assert 'System' in writes[0] and 'MonitorX' in writes[0]
    assert 'Ambiscan' not in writes[0]
    page.get_by_role('button', name='Game', exact=True).click()
    expect(visible_apply).to_have_text('Apply')
    expect(visible_rows.locator('.step-control')).to_have_count(row_count * 2)
    volume_plus = volume.locator('.step-control').nth(1)
    volume_minus = volume.locator('.step-control').nth(0)
    expect(volume_plus).to_be_disabled()
    expect(volume_minus).to_be_enabled()
    step_away, step_back = (volume_plus, volume_minus) if volume_plus.is_enabled() else (volume_minus, volume_plus)
    step_away.click()
    expect(volume.locator('.compact-control-value')).to_have_text('100%')
    step_back.click()
    expect(volume.locator('.compact-control-value')).to_have_text('Default')
    row = monitor_x
    plus = row.locator('.step-control').nth(1)
    expect(plus).to_be_enabled(timeout=30000)
    plus.click()
    first_value = int(row.locator('.compact-control-value').inner_text())
    assert first_value == system_value + 2, 'Game must inherit a saved System value'
    page.get_by_role('button', name='System', exact=True).click()
    expect(visible_rows).to_have_count(row_count)
    page.get_by_role('button', name='Game', exact=True).click()
    expect(row.locator('.compact-control-value')).to_have_text(f'{first_value:+}')
    assert len(writes) == 1, 'adjustment must not apply automatically'
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
    assert len(writes) == 1
    page.get_by_role('button', name='Reset all').click()
    expect(visible_rows.locator('.compact-control-value')).to_have_text(['Default'] * row_count)
    assert len(writes) == 1, 'reset must not save unsaved edits'
    plus.click()
    visible_apply.click()
    expect(page.get_by_text('Game settings applied',exact=True)).to_be_visible(timeout=15000)
    expect(page.locator('.active-game-settings-scope-content:not([hidden]) .active-game-settings-remove')).to_have_text('Remove Game Settings')
    assert len(writes) == 2
    assert 'identity' in writes[1]
    assert 'MonitorX' in writes[1]
    assert 'Ambiscan' not in writes[1], 'untouched settings must not be written'
    page.get_by_role('button', name='Reset all').click()
    expect(visible_rows.locator('.compact-control-value')).to_have_text(['Default'] * row_count)
    assert len(writes) == 2, 'reset must wait for Apply'
    page.get_by_role('button', name='System', exact=True).click()
    expect(page.get_by_text('Game settings take priority.', exact=False)).to_be_visible()
    expect(page.locator('.active-game-settings-scope-content:not([hidden]) .active-game-settings-footer .active-game-settings-remove')).to_have_text('Remove System Settings')
    monitor_x.locator('.step-control').nth(1).click()
    visible_apply.click()
    expect(page.get_by_text('System settings saved', exact=True)).to_be_visible(timeout=15000)
    assert len(writes) == 3
    assert 'System' in writes[2] and 'MonitorX' in writes[2]
    assert 'Ambiscan' not in writes[2]
    step_away, step_back = (volume_plus, volume_minus) if volume_plus.is_enabled() else (volume_minus, volume_plus)
    step_away.click()
    visible_apply.click()
    expect(page.get_by_text('System settings saved', exact=True)).to_be_visible(timeout=15000)
    assert len(writes) == 4 and 'Volume' in writes[3]
    step_back.click()
    expect(volume.locator('.compact-control-value')).to_have_text('Default')
    visible_apply.click()
    expect(page.get_by_text('System settings saved', exact=True)).to_be_visible(timeout=15000)
    assert len(writes) == 5 and 'Volume' in writes[4]
    assert 'update[changes][0][value]' not in parse_qs(writes[4])
    page.reload(wait_until='domcontentloaded')
    page.locator('.initial-loading-shell').wait_for(state='hidden', timeout=120000)
    page.get_by_role('button', name='System', exact=True).click()
    expect(page.locator('.active-game-settings-scope-content:not([hidden]) .active-game-settings-remove')).to_have_text('Remove System Settings')
    page.get_by_role('button', name='Remove System Settings').click()
    expect(page.get_by_text('including emulator options', exact=False)).to_be_visible()
    expect(page.get_by_text('Unsaved progress may be lost', exact=False)).to_be_visible()
    assert not removals, 'removal must wait for confirmation'
    page.get_by_role('dialog').get_by_role('button', name='Remove and restart').click()
    removed_notice = page.get_by_text('System settings removed', exact=False)
    expect(removed_notice).to_be_visible(timeout=15000)
    assert len(removals) == 1 and 'System' in removals[0]
    expect(removed_notice).to_be_hidden(timeout=7000)
    page.reload(wait_until='domcontentloaded')
    page.locator('.initial-loading-shell').wait_for(state='hidden', timeout=120000)
    page.get_by_role('button', name='System', exact=True).click()
    expect(page.locator('.active-game-settings-scope-content:not([hidden]) .active-game-settings-remove')).to_have_count(0)
    page.get_by_role('button', name='Game', exact=True).click()
    page.get_by_role('button', name='Remove Game Settings').click()
    expect(page.get_by_text('including emulator options', exact=False)).to_be_visible()
    page.get_by_role('dialog').get_by_role('button', name='Remove and restart').click()
    expect(page.get_by_text('Game settings removed', exact=False)).to_be_visible(timeout=15000)
    assert len(removals) == 2 and 'Game' in removals[1]
    assert not errors, errors
