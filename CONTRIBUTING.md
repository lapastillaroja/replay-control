# Contributing

Replay Control is a personal project — I built it for my own retro gaming setup and share it because others might find it useful too. Contributions are welcome! That said, I reserve the right to decline pull requests or feature suggestions that don't align with the project's direction. If you're thinking about a large change, open an issue first so we can discuss it.

## Prerequisites

- **Rust** (stable) via [rustup](https://rustup.rs/)
- **wasm-bindgen-cli**: `cargo binstall wasm-bindgen-cli`
- **mold** (fast linker): `sudo dnf install mold` (Fedora) / `sudo apt install mold` (Debian/Ubuntu)
- **brotli** (release builds pre-compress the WASM with it; `build.sh` fails if it's missing): `sudo dnf install brotli` / `sudo apt install brotli`
- **binaryen** (provides `wasm-opt`; release builds run `wasm-opt -Oz` to shrink the WASM and `build.sh` fails without it): `sudo dnf install binaryen` / `sudo apt install binaryen`
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

This builds both WASM (`wasm-dev-fast` profile) and the SSR server (`dev-fast` profile), then starts `cargo-watch` for auto-rebuild and reload on file changes. The app runs on port 8091 by default.

Options:
- `--storage-path /path/to/roms` — path to ROM storage (required for local dev)
- `--port 8091` — override the default port

Set `REPLAY_DEV_WASM_PROFILE=wasm-dev` and/or `REPLAY_DEV_SERVER_PROFILE=dev` when smaller dev artifacts are more important than rebuild latency.


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

Four test layers; each is independent and runs from the repo root.

```bash
# 1. Rust tests (unit + in-process integration). Fastest. ~1100 tests.
cargo test --features ssr
cargo test -p replay-control-core-server --all-features

# 2. Container HTTP smoke. Boots the app inside a network-isolated
#    podman/docker container and runs HTTP-level assertions with curl.
./build.sh
cargo run -p generate-test-fixtures
./tests/integration/run.sh

# 3. Container Playwright (the same suite CI runs in `e2e`). Boots the
#    Containerfile.replayos image with a mock GitHub server attached and
#    drives the e2e pytest suite against it. This is the recommended
#    way to reproduce CI failures locally.
./tests/container/run.sh
# To re-use an existing build: SKIP_BUILD=1 ./tests/container/run.sh
# To force docker on a podman host: CONTAINER_ENGINE=docker ./tests/container/run.sh

# 4. Browser e2e (Playwright) against a live target on the LAN — the Pi
#    or a localhost dev server. ~60 tests across page-health,
#    response-cache, auto-update, and the corruption banner. One-time
#    setup uses a venv because system Pythons block plain `pip install`
#    under PEP 668:
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
# Single deploy (fast dev profile, cross-compiled for aarch64)
./dev.sh --pi [IP]
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

For pure metadata contributions (descriptions, box art links, manuals for games not covered by upstream sources like No-Intro / TheGamesDB / MAME / LaunchBox — e.g. AmigaVision, aftermarket ROMs, homebrew compilations), no Rust changes are needed. See [Contributing community metadata](docs/contributing/community-metadata.md) for the JSON schema and submission flow.

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

### Integration checklist (adapting a feature from elsewhere)

CI does not run automatically on PRs from forks (GitHub requires a maintainer
to approve the workflow run first), so none of this is caught by a green
check — it only surfaces in manual review or on a real device. If you're
porting a feature from another branch/fork/codebase, verify before opening
the PR:

- **Server functions are registered.** This repo calls
  `server_fn::axum::register_explicit::<...>()` for every `#[server]` fn in
  `main.rs` — inventory auto-registration doesn't work when the functions
  live in a library crate. A new fn that's only defined but never listed
  there 404s at runtime even though it compiles clean.
- **Auth role is set.** `is_user_server_function` in `api/mod.rs` is an
  allowlist — anything not listed defaults to Admin-only. If the feature is
  meant for a normal Net Control user, add its fn names there or it 403s.
- **CSS uses this repo's design tokens**, not a source repo's. Check
  `style/*.css` for the actual custom properties (`--surface`, `--text`,
  `--border`, `--error`, `--accent`, ...) before writing new rules — a
  `var(--card-bg)` or `--accent-color` copied from elsewhere resolves to
  nothing and silently renders unstyled, especially visible on dark skins.
- **DB naming matches convention**: singular table names
  (`game_manual_resource`, `game_resource_link`, not `game_notes`), and no
  `updated_at`/timestamp columns unless something actually reads them.
- **Errors are surfaced, not swallowed.** Avoid `let _ = fallible_call()` on
  save/mutate paths — show the same error state the other detail-page
  sections use.
- **Tests + CHANGELOG** — a round-trip test following the existing
  `server_fn_tests.rs` pattern, and an entry under Unreleased.

This list grew out of real review rounds where a ported PR looked complete
but didn't work at all on a real device. Going through it before requesting
review saves a round trip for both sides.

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
