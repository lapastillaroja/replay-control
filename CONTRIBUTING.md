# Contributing

Replay Control is a personal project — I built it for my own retro gaming setup and share it because others might find it useful too. Contributions are welcome! That said, I reserve the right to decline pull requests or feature suggestions that don't align with the project's direction. If you're thinking about a large change, open an issue first so we can discuss it.

## Prerequisites

- **Rust** (stable) via [rustup](https://rustup.rs/)
- **wasm-bindgen-cli**: `cargo binstall wasm-bindgen-cli`
- **mold** (fast linker): `sudo dnf install mold` (Fedora) / `sudo apt install mold` (Debian/Ubuntu)
- **Rust targets**:
  ```
  rustup target add wasm32-unknown-unknown
  rustup target add aarch64-unknown-linux-gnu   # only for Pi cross-compilation
  ```

### Cross-compilation (for deploying to Pi)

The server binary must be compiled for aarch64 to run on RePlayOS (Raspberry Pi).

**Fedora:**
```
sudo dnf install gcc-aarch64-linux-gnu
```

**Ubuntu / Debian:**
```
sudo apt install gcc-aarch64-linux-gnu
```

**macOS:**
```
brew install aarch64-unknown-linux-gnu
```

The Cargo linker config is already set up in `.cargo/config.toml`.


## Building

### Release build

```bash
./build.sh              # x86_64
./build.sh aarch64      # Cross-compile for Pi
```

Produces a server binary and site assets at `target/site/`.

### Development build

```bash
./dev.sh --storage-path /path/to/roms
```

This builds both WASM (wasm-dev profile) and the SSR server (dev profile), then starts `cargo-watch` for auto-rebuild and reload on file changes. The app runs on port 8091 by default.

Options:
- `--storage-path /path/to/roms` — path to ROM storage (required for local dev)
- `--port 8091` — override the default port


## Running Locally

```bash
# Development mode with auto-reload
./dev.sh --storage-path /path/to/roms

# Or run a release build manually
./build.sh
./target/release/replay-control-app --storage-path /path/to/roms --site-root target/site
```

The storage path should point to a directory with ROMs organized by system (e.g., `roms/Nintendo - Super Nintendo Entertainment System/`).


## Running Tests

Three test layers; each is independent and runs from the repo root.

```bash
# 1. Rust tests (unit + in-process integration). Fastest. ~1100 tests.
cargo test --features ssr

# 2. Container integration. Boots the app inside a network-isolated
#    podman/docker container and runs HTTP-level assertions with curl.
./build.sh
cargo run -p generate-test-fixtures
./tests/integration/run.sh

# 3. Browser e2e (Playwright). Hits a running app on the Pi, container,
#    or localhost. ~60 tests across page-health, response-cache,
#    auto-update, and the corruption banner. One-time setup uses a venv
#    because system Pythons block plain `pip install` under PEP 668:
python3 -m venv tests/e2e/.venv
tests/e2e/.venv/bin/pip install playwright pytest pytest-timeout
tests/e2e/.venv/bin/playwright install chromium
# Then run:
PI_IP=192.168.10.30 tests/e2e/.venv/bin/python -m pytest tests/e2e/ -v --timeout=180
```

Useful subsets:

```bash
# Only the corruption-recovery suite (Rust):
cargo test --features ssr --test corruption_tests

# Only the DbPool property tests (locks in the WAL-unlink regression):
cargo test --features ssr -p replay-control-core-server --lib db_pool

# Only the e2e corruption-banner test (stops + restarts the service on
# the target — run on a Pi you're OK with seeing a 30 s blip):
PI_IP=192.168.10.30 tests/e2e/.venv/bin/python -m pytest \
    tests/e2e/test_corruption_banner.py -v
```

E2e against a local dev server (page-health and response-cache only —
the corruption-banner test needs SSH/docker to clobber DB files):

```bash
APP_URL=http://localhost:8091 tests/e2e/.venv/bin/python -m pytest \
    tests/e2e/test_page_health.py tests/e2e/test_response_cache.py -v
```

(Or `source tests/e2e/.venv/bin/activate` once per shell to drop the
prefix; `deactivate` to leave.)

When adding new behaviour, add unit/integration tests in-tree and an e2e
test only for cross-layer flows (SSE push → DOM signal → user action).
Match the existing pattern: write a *content-survival* assertion when
the fix touches data — flag-flip-only tests pass even if the underlying
work is no-oped.


## Deploying to Pi

### Fast iteration (recommended)

```bash
# Single deploy (dev profile, cross-compiled for aarch64)
./dev.sh --pi [IP]

# Watch mode: auto-rebuild + redeploy on file changes
./dev.sh --pi [IP] --watch
```

If no IP is specified, defaults to `replay.local`.

### Release deploy

```bash
./build.sh aarch64
bash install.sh --local --ip <pi-address>
```


## Project Structure

See [README.md](README.md) for a full overview. The key crates:

- **`replay-control-core/`** — shared library (game databases, ROM parsing, metadata, settings). Native only.
- **`replay-control-app/`** — Leptos 0.7 SSR web app with WASM hydration. Has two feature flags: `ssr` (server) and `hydrate` (browser).
- **`replay-control-libretro/`** — standalone libretro core for TV display (.so). Separate workspace, not part of the main Cargo workspace.


## Pull Requests

Pull requests are welcome for bug fixes, documentation improvements, and small enhancements. For larger features, please open an issue first — the maintainer may have different plans or may prefer a different approach.

- Describe what changed and why
- Test on Pi if possible (or note if you haven't)
- Follow [Conventional Commits](https://www.conventionalcommits.org/):
  ```
  feat: add region preference setting
  fix: resolve hydration warnings on games page
  refactor: rename rom_cache -> game_library
  docs: update metadata design doc
  ```
- See [AI_POLICY.md](AI_POLICY.md) for guidelines on AI-assisted contributions


### Faster builds (optional)

Install [sccache](https://github.com/mozilla/sccache) for compilation caching. `dev.sh` auto-detects and uses it when available:

```
cargo binstall sccache
```

To clear all caches (cargo + sccache):
```bash
./dev.sh --clean
```

## Development Tips

- Always rebuild both WASM and server after changing shared types (`types.rs`, `server_fns.rs`). Stale WASM causes hydration failures.
- If `cargo clean -p replay-control-app` doesn't pick up changes, delete `target/wasm32-unknown-unknown/release/deps/replay_control_app*` manually.
- Server functions from `replay-control-core` need explicit registration in `main.rs` — the linker strips them otherwise.
- The app auto-detects storage at `/media/sd`, `/media/usb`, `/media/nfs`, or falls back to `--storage-path`.
