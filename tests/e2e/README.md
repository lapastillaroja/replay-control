# End-to-End Tests

Browser-based tests for the auto-update UI using Playwright.

## Prerequisites

- Raspberry Pi running Replay Control at `replay.local:8080`
- `sshpass` installed on the test runner
- Python 3.10+ with Playwright:
  ```bash
  pip install playwright pytest
  playwright install chromium
  ```

## Running

```bash
# All tests
PI_IP=192.168.10.30 python -m pytest tests/e2e/ -v --timeout=180

# Just the check/notification tests (no service restart)
PI_IP=192.168.10.30 python -m pytest tests/e2e/test_update_check.py -v

# Just the install tests (triggers real service restart!)
PI_IP=192.168.10.30 python -m pytest tests/e2e/test_update_install.py -v
```

## Test Requirements

For update tests to work, the Pi must be running a version lower than the
available release. Set `version = "0.0.1"` in `replay-control-app/Cargo.toml`
and deploy before running install tests.

The update channel is set to `beta` by the test fixtures (the only release
is `v0.1.0-beta.4`, a prerelease).

## Test Categories

### `test_update_check.py` — Safe, no side effects
- Update banner appears after manual check
- Update banner appears from background check (60s)
- Banner has all action buttons
- Skip hides the banner
- Channel switch hides/shows prereleases
- Check button states
- Version display

### `test_update_install.py` — Triggers real service restart
- "Update Now" navigates to /updating
- /updating shows downloading progress
- /updating shows "do not navigate away"
- /updating shows restarting + auto-reloads
- Direct navigation to /updating without update redirects
- Temp files cleaned after update
