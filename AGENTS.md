# Code Rules

## Start Here

Before making non-trivial changes, read the relevant docs. The source of truth is `docs/`; `site/content/docs/` is generated.

- `docs/README.md` — documentation map for features and architecture
- `docs/architecture/technical-foundation.md` — crate responsibilities, stack, key file paths, runtime data flow
- `docs/architecture/design-decisions.md` — Raspberry Pi constraints, memory/performance tradeoffs, rejected alternatives
- `docs/architecture/server-functions.md` — Leptos SSR patterns, server function registration, response cache, SSE
- `docs/architecture/startup-pipeline.md` — background initialization, metadata refresh, cache verification, watchers
- `docs/architecture/database-schema.md` — SQLite tables, indexes, migrations, corruption handling
- `docs/architecture/connection-pooling.md` — read/write pools, journal modes, WriteGate, pool lifecycle
- `docs/known-issues.md` — known product issues before treating behavior as a regression

Read task-specific docs as needed:

- Library browsing, ROM changes, favorites, recents: `docs/features/game-library.md`
- Now-playing detector and UI surfaces (top-bar pill, home hero, manual deep link): `docs/features/now-playing.md`
- Game detail pages, variants, media, launch actions: `docs/features/game-detail.md`
- Metadata imports, built-in catalog data, ROM tags: `docs/features/metadata.md`
- Box art, screenshots, thumbnail matching: `docs/features/thumbnails.md`
- Search and developer pages: `docs/features/search.md`
- Settings, skin, locale, update channel: `docs/features/settings.md`
- Storage modes, NFS/exFAT behavior, corruption recovery: `docs/features/storage.md`
- Auto-update behavior: `docs/features/updates.md`
- Recommendations and related games: `docs/features/recommendations.md`
- Series/franchise behavior: `docs/features/game-series.md`
- TV/libretro proof of concept: `docs/features/libretro-core.md`

## Commits

- Use [Conventional Commits](https://www.conventionalcommits.org/): `feat:`, `fix:`, `refactor:`, `docs:`, `style:`, `test:`, `chore:`, `perf:`, `build:`
- Dependency updates use `build:` (e.g., `build: bump actions/checkout to v6`)
- Never commit without explicit user approval
- **NEVER run `git push` without explicit user approval. ALWAYS ask first.**
- Do not add `Co-authored-by: Claude ...` trailers to commit messages — commits are authored by the user only.

## Releases

- **Never create or push git tags manually.** Releases are driven by [release-plz](release-plz.toml): bump `version` in [replay-control-app/Cargo.toml](replay-control-app/Cargo.toml), update [CHANGELOG.md](CHANGELOG.md), and push the commit to `main`. Release-plz then creates the tag and the GitHub Release, and chains [build-release.yml](.github/workflows/build-release.yml) via `workflow_call` to attach artifacts.
- If a tag is pushed by hand, `build-release.yml` fires via its `push: tags: ['v*']` trigger with no release behind it. The upload step self-heals by creating the release if missing, but this is a fallback — don't rely on it.

## Testing

- Add unit, integration, and/or e2e tests for new features and bug fixes to prevent regressions
- Unit tests: in-module `#[cfg(test)]` blocks for pure logic (settings, config, utilities)
- Integration tests: `replay-control-app/tests/*.rs` (e.g. `corruption_tests.rs`) — server functions exercised via `test_router`
- Container integration: `tests/integration/run.sh` — boots the app in a podman/docker container and asserts HTTP responses
- Browser e2e: `tests/e2e/*.py` — Playwright; see `tests/e2e/README.md`
- During local e2e development, prefer the debug container path to avoid slow release rebuilds: `BUILD_PROFILE=debug SKIP_DATA=1 PODMAN_DIRECT_BRIDGE=1 PYTEST_ARGS='tests/e2e/test_library_build_pipeline.py::<test_name> -v' ./tests/container/run.sh`. This uses `./dev.sh --build-only` for fast dev artifacts; use the default release path before final validation when performance or release-only behavior matters.
- See [CONTRIBUTING.md "Running Tests"](CONTRIBUTING.md#running-tests) for the full command set, including useful subsets and the local-dev-server path
- **Wait for hydration before any client interaction in Playwright.** The app renders a global loading overlay (`.initial-loading-shell`) that only gets its `is-hidden` class from a client-only `Effect` once hydration completes — so `page.locator(".initial-loading-shell").wait_for(state="hidden")` is the reliable "app is interactive" signal. Until it hides, the Leptos router has not attached its click interceptor, so clicking an in-app `<a>`/`<A>` link triggers a **full page reload instead of a client-side transition** — silently masking any transition-only bug. This matters most with the unoptimized dev/`dev-fast` WASM bundle (tens of MB), where hydration can take ~15s. Prefer this global signal, or a per-feature `.is-hydrated` marker where one exists (e.g. `.update-controls-row.is-hydrated`), over fixed `wait_for_timeout` sleeps.

## Docs Site

- Feature/architecture docs live in `docs/` — this is the source of truth
- Feature docs are user-facing and should avoid Rust code, file paths, crate/type names, API names, and implementation mechanics. Put design decisions and implementation details in `docs/architecture/`.
- `site/content/docs/` is auto-generated by `site/sync-docs.sh` and gitignored — **never edit those files directly**
- After editing docs, run `site/sync-docs.sh` to sync, then restart Hugo (`pkill -f 'hugo server'; cd site && ./dev.sh &`)
- Hugo auto-reload usually fails after content changes — always restart
- Use `./site/dev.sh` to serve locally (auto LAN IP + subpath)

## Raspberry Pi Access

- Default Pi host is `replay.local`; use a specific IP only when the user provides one.
- Use `root` as the SSH user. The default password is `replayos`, or `PI_PASS` if the environment overrides it.
- Prefer `./dev.sh --pi` for build/deploy because it already handles the default host, `SSH_ASKPASS`, SSH options, ControlMaster reuse, rsync, service stop/start, and catalog/site deployment.
- To deploy to a specific address, run `./dev.sh --pi <ip-or-hostname>`.
- Never set or unset `CARGO_TARGET_DIR` unless the user explicitly asks for it. Use the environment as-is for build and deploy commands.
- For ad hoc SSH commands, use the askpass pattern from `dev.sh` — plain `ssh` with a password requires `SSH_ASKPASS`:
  ```sh
  ASKPASS=$(mktemp) && printf '#!/bin/sh\necho "%s"\n' "${PI_PASS:-replayos}" > "$ASKPASS" && chmod +x "$ASKPASS"
  SSH_ASKPASS="$ASKPASS" SSH_ASKPASS_REQUIRE=force ssh -o StrictHostKeyChecking=no root@192.168.10.30 "<command>"
  rm -f "$ASKPASS"
  ```

### Switching active storage (sd / usb / nfs)

The active library lives on whichever storage RePlayOS' frontend config selects. To switch (e.g. to benchmark USB vs NFS), edit the config and reboot — the app re-opens the per-storage `library.db` on boot:

1. The config is `/media/sd/config/replay.cfg`; the key is `system_storage = "<sd|usb|nfs>"` (NFS also reads `nfs_server` / `nfs_share` / `nfs_version` from the same file).
2. Stop the service, change the value in place, then reboot the Pi (a full reboot remounts cleanly and avoids the busy-mount fallback):
   ```sh
   systemctl stop replay-control
   sed -i 's/^system_storage = .*/system_storage = "usb"/' /media/sd/config/replay.cfg
   reboot
   ```
3. **Wait for the startup scan to finish before benchmarking.** After reboot the app rescans the newly-active library; page/stat timings are not representative until it settles. Confirm via the service logs or by polling that per-system stats report `Fresh` (the `game_library_system_stats.refresh_state` column) and the home page renders without a "scanning" banner.
4. Per-storage DBs live at `/var/lib/replay-control/storages/{sd,usb,nfs}-<id>/library.db`; the one held open by the service PID (`ls -l /proc/<pid>/fd | grep library.db`) is the active one.

Note: a launch-triggered restart can flip `system_storage` when a mount is busy, so the value may change underneath you — prefer a clean `reboot` over a bare service restart when switching, and re-check the value after boot.

## General

- Follow best practices for the language and framework in use
- Avoid unnecessary complexity — keep solutions simple and focused
- Write readable code that is easy to maintain
- Prefer small, focused functions and components over large monoliths
- Extract logic from templates — setup above, structure below
- Don't over-engineer: solve the current problem, not hypothetical future ones. If a 200-line version genuinely collapses to 50 honest lines that address the same root cause, prefer the 50.
- Don't under-engineer either. A 2-line workaround under a misnamed key, a phase that quietly does more than its name says, a constant repurposed without renaming, or a "composite-string trick" smuggling new meaning into an old field — these are hacks even when tests pass. If a fix changes what something *represents*, rename it and centralize the new meaning. Token budget is not the bottleneck; the next person reading the code is — spend tokens on solid, simple fixes, not on verbose complicated ones, and never on a hack just to be shorter.
- Band-aids are acceptable only when the real fix is genuinely deferred. In that case, label the band-aid as such, name the underlying design issue, and capture the deferred work explicitly — don't let it quietly become the final state.
- Investigate first, ask if uncertain. Read-only investigation (grep, read the file, run a probe, check memory) is free relative to interrupting the user. Only ask when investigation genuinely can't resolve the question — and then ask specifically ("I found X and Y, which?") rather than vaguely ("what should I do?").

## Metadata Analysis

When analyzing metadata source coverage or quality across a ROM set (release dates, developers, genres, cover art, etc.), **exclude** ROM hacks, translations, homebrew, and aftermarket. Filter by No-Intro filename tags: `[h]`, `[h1-3]`, `(Hack)`, `[T+XX]`, `[T-XX]`, `[a]`, `[b]`, `[o]`, `[f]`, `(Unl)`, `(Aftermarket)`, `(Homebrew)`, `[BIOS]`. Keep prototypes, betas, official regional variants, and Virtual Console re-releases. Official metadata databases index only original releases, so matching hacks/translations systematically inflates "missing" counts and pollutes per-source comparisons. This rule applies only to **analysis** — runtime code that displays the user's actual library must not filter their ROMs.

## Rust

- Use `clippy` conventions — no unnecessary `clone()`, prefer references where possible
- Prefer `Result` propagation over manual error matching when the caller handles errors
- Use descriptive variable names; avoid single-letter names outside short closures
- **Never inline long crate paths.** Add a `use` at the top of the file and reference the short name in the body. Bodies must not contain `crate_a::mod_b::Thing::method(...)`; the import goes above, the call below.
  - Bad: `replay_control_core_server::config::ReplayConfig::from_file(&path)`
  - Good: `use replay_control_core_server::config::ReplayConfig;` at the top, then `ReplayConfig::from_file(&path)` in the body.
  - **Dual-target files** (a module that compiles for both `ssr` and `hydrate`/`wasm32`) gate SSR-only imports with `#[cfg(feature = "ssr")]`. Existing convention in `replay-control-app/src/server_fns/mod.rs`:
    ```rust
    #[cfg(feature = "ssr")]
    use replay_control_core_server::library_db::LibraryDb;
    ```
    There is **no** "wasm makes me write the full path inline" exception — the cfg-gated `use` works everywhere the path was reachable from inside a `#[cfg(feature = "ssr")]` or `#[server]` body.
  - **Sweep edited files before declaring done** — a fix that ships with a fresh long inline reference is not done. `grep -nE 'replay_control_core(_server)?::[a-z_]+::[A-Za-z]' <files you touched>` and clean any new hits.

### Crate boundary: `replay-control-core` vs `replay-control-core-server`

New code goes in whichever crate matches its dependencies:

- **`replay-control-core`** (pure, wasm-safe) — types, enums, wire contracts, pure domain logic, reference data, error types. **Forbidden deps**: `rusqlite`, `tokio`, `reqwest`, `deadpool-*`, `quick-xml`, `std::fs::*` (beyond path manipulation), `std::process::*`. Compiles for both native and `wasm32-unknown-unknown`.
- **`replay-control-core-server`** (native-only) — everything else. SQL queries, filesystem operations, HTTP clients, process spawning, XML parsing, tokio async code. Compiled for native targets only.

Quick test: does the code name any forbidden dep? If no → core. If yes → core-server.

Re-export pattern inside core-server: modules that wrap a core module do `pub use replay_control_core::<module>::*;` at the top, then add native fns below. Consumers reach both type and native fn via `replay_control_core_server::<module>::`.

**Orphan-rule workaround**: when a native trait (like `rusqlite::ToSql`) needs to apply to a core type, create a local newtype in the consuming core-server module (e.g. `DpSql(DatePrecision)` in `library_db`). Don't move the core type to core-server just to satisfy the orphan rule.

### No inline SQL in `replay-control-app`

The app crate must not contain SQL strings — no `conn.prepare("...")`, `conn.query_row("SELECT ...", ...)`, `conn.execute("INSERT ...", ...)`, etc. Schema knowledge belongs in `replay-control-core-server::library_db::LibraryDb` (and the equivalent helpers for other DBs). Drift across the boundary (a column rename, an index change) should never require hunting through the app crate.

When you need a new query from app code, add a method to `LibraryDb` and call it via the pool: `state.library_reader.read(LibraryDb::my_new_query)`. Tests get the same treatment — no `INSERT` strings in `tests/*.rs` either.

### Read paths must not write to the library DB

Writes to `library.db` are restricted to scan / rebuild / enrichment / watcher / explicit-user-action paths. Read-time SSR or HTTP handlers must not write — not even via a "just-in-time" L3 fallback. The `LibraryReadPool` / `LibraryWritePool` newtypes (in `replay-control-app/src/api/library_pool.rs`) enforce this at compile time: handlers and request-time code receive `&LibraryReadPool`, which doesn't expose `write` / `try_write` / `reopen` / `reset_to_empty`.

If a handler "needs" to populate a missing system on miss, the right answer is to return what's in L2 (possibly empty) and trigger a background populate via `BackgroundManager`. Don't add an escape hatch from a read path. See `docs/architecture/database-schema.md` "Write-isolation rule" and the regression suite at `replay-control-app/tests/cold_nfs_tests.rs`.

### Read-then-write must be atomic when correctness depends on it

`pool.read(...)` and `pool.write(...)` (or `try_write`) are two separate `interact()` calls on two **different** deadpool connections — not in a shared SQLite transaction. Anything between them can change underneath: another writer can land, the filesystem can drift, the row you just checked can disappear. This is the TOCTOU class of bug.

`LibraryWritePool` deliberately does **not** expose `read` / `try_read`. To do a read inside a writer codepath you must pick an explicit shape:

- **Atomic** (the read and the write must agree): do the read **inside** the same `db.write(|conn| ...)` (or `db.transaction(|tx| ...)`) closure on the same connection. Multiple statements inside one closure share a SQLite write transaction. Don't put network or CPU-heavy work inside — that closure holds the single write connection for its entire duration.
- **Intentionally non-atomic** (network or slow work happens between read and write): route through `pool.as_reader().read(|c| ...)`. The `.as_reader()` token at the call site is the signal that this read is on a different connection and is not in the same transaction as any later write. The downstream writes in this case must tolerate the race — typically `INSERT OR REPLACE` upserts plus `ON DELETE CASCADE` for cleanup.

Use `db.transaction(...)` when you want explicit `BEGIN IMMEDIATE` / commit / rollback-on-`Err`; use `db.write(...)` for the auto-commit-on-end-of-closure shorthand.

## Leptos Components

### Structure: setup above, view below

Compute derived signals and closures **above** the `view!` macro. The template should be pure structure:

```rust
#[component]
fn MyComponent() -> impl IntoView {
    // Setup phase: signals, derived values, callbacks
    let count = RwSignal::new(0);
    let label = move || if count.get() % 2 == 0 { "Even" } else { "Odd" };
    let on_click = move |_| count.update(|n| *n += 1);

    // View phase: declarative template
    view! {
        <button on:click=on_click>{label}</button>
    }
}
```

### Use `Suspend` + `ErrorBoundary` for async data

Instead of deeply nested `resource.get().map(|r| match r { Ok/Err })`, use:

```rust
<ErrorBoundary fallback=|errors| view! { <ErrorList errors /> }>
    <Suspense fallback=move || view! { <Loading /> }>
        {move || Suspend::new(async move {
            let data = my_resource.await;
            view! { <DataView data /> }
        })}
    </Suspense>
</ErrorBoundary>
```

- `Suspend::new(async { resource.await })` eliminates `.get().map()` boilerplate
- `ErrorBoundary` catches `Err` from any child rendering a `Result` — no manual match needed
- Use `Transition` instead of `Suspense` when reloading data to avoid flickering

### Signals are Copy — don't clone them

`ReadSignal`, `WriteSignal`, `RwSignal`, and `Memo` are all `Copy`. Move them into multiple closures freely:

```rust
let count = RwSignal::new(0);
// Both closures capture `count` — no clone needed
let increment = move |_| count.update(|n| *n += 1);
let display = move || count.get().to_string();
```

### Use `.read()` instead of `.get()` to avoid cloning values

`.get()` clones the inner value. `.read()` returns a borrow guard:

```rust
// Clones the Vec
let len = move || my_vec.get().len();
// Borrows without cloning
let len = move || my_vec.read().len();
```

### Break large views into child components

Instead of inline `.map()` chains with complex per-item logic inside `view!`, extract a component:

```rust
// Good: each item is its own component
#[component]
fn ItemRow(item: Item) -> impl IntoView {
    view! { <div class="item">{item.name.clone()}</div> }
}
```

### Use `StoredValue` for non-reactive data in closures

When a `String` or other non-`Copy` value needs to be used in multiple closures within `view!`, wrap it in `StoredValue` to make it `Copy`. This eliminates the "clone explosion" pattern:

```rust
// Bad: clone for every closure
let filename = rom.filename.clone();
let filename2 = rom.filename.clone();
let filename3 = rom.filename.clone();

// Good: StoredValue is Copy
let filename = StoredValue::new(rom.filename.clone());
// use filename.get_value() everywhere — no clones needed
```

### Use `<Show>` for conditional rendering

Prefer `<Show>` over `if/else` closures inside `view!`:

```rust
<Show when=move || is_active.get() fallback=|| view! { <Inactive /> }>
    <Active />
</Show>
```

### Keep hydration structure stable

For SSR pages, avoid changing the root element shape across server render and hydration. If a control can be enabled or disabled based on async/auth/device state, prefer one stable element with disabled styling/attributes over switching between different tags such as `<A>` and `<div>`.

A component created inside `Suspend`/`Transition`/other async closures that reads a context (`use_context`/`expect_context`) can panic ("expected context … to be present") on a **client-side navigation**: the async-resolved reactive owner no longer chains to where the context was provided. It works on the initial SSR+hydrate load, so the bug only surfaces on SPA nav. Two cases:

- **App-global singleton contexts (e.g. i18n):** make the accessor resilient instead of threading props through every component. The client has exactly one instance per session, so cache it at provide time and fall back to it when `use_context` returns `None`. See `i18n::{provide_i18n, use_i18n}` (`replay-control-app/src/i18n/mod.rs`): the fallback is `#[cfg(target_arch = "wasm32")]`-only, so SSR (per-request locales on shared threads) keeps using `expect_context`, which is always in scope server-side. Components can then call `use_i18n()` anywhere, including inside Suspense.
- **Non-global contexts (per-page/per-component):** there is no single instance to cache, so capture the value in the parent and pass it into the async-rendered children as props.

### Use `#[prop(into)]` for flexible component APIs

```rust
#[component]
fn Badge(#[prop(into)] label: Signal<String>) -> impl IntoView { ... }
```

### Use `bind:` for two-way binding

Replaces `prop:value` + `on:input` boilerplate:

```rust
let name = RwSignal::new("".to_string());
view! { <input type="text" bind:value=name /> }
```
