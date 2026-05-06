# Now Playing

The companion app shows what is running on the appliance right now: a "Now Playing" pill in the top bar, a hero card on the home page, and a "live" badge on the detail page of the active game. Updates are pushed to every connected client over Server-Sent Events.

## What you see

- **Top-bar pill** — present on every page while a game is loaded. Pulsing dot, game name, system, elapsed time. Tap it to jump to the game's detail page.
- **Home hero card** — replaces "Last Played" while a game is active. Quick links to the detail page, favorite toggle, and a one-tap **Manual** button that deep-links to the manuals section.
- **Game detail "Now Playing" pill** — the title bar of the active game's detail page shows a pill so you know "this is the one running right now".
- **Elapsed timer** — counts up once per second, anchored at the timestamp the appliance first detected the active session.

The state has three values: `Playing` (a non-menu libretro core is loaded and the active ROM path was extracted from heap), `Menu` (the appliance is up but no game core is mapped, or one is mapped but the active path can't be read yet), and `NotRunning` (no `replay` PID).

## How it works

Detection is RePlayOS-only and runs server-side in `replay-control-app/src/api/now_playing.rs`.

1. **Find the `replay` PID** via `/proc/*/comm` (cached across ticks; re-verified by reading `/proc/<pid>/comm` directly).
2. **Read `/proc/<pid>/maps`** and check for a `*libretro.so` mapping that isn't part of `NON_GAME_CORES` (`replay_libretro` is the menu, `avtest` is the A/V test tool). The shared exclusion list lives at `replay-control-core-server/src/replay_proc.rs`.
3. **Walk `/proc/<pid>/mem`** through the heap range listed in `maps`, scanning for `/media/.../roms/...` strings.
4. **Pick the LAST match** in the heap — newer allocations sit at higher addresses, while earlier hits are stale paths from previous cores or from the menu's recents list.
5. **Resolve to a game** via `LibraryDb::lookup_game_entries`, falling back to a longest-prefix `rom_path LIKE` match if the heap-walked filename has trailing junk (heap re-reads during a core transition can produce strings like `"…sfiii3.zip in cache"`).

The full validation log — 27 launches across 13 cores, 100 % detection, perf benchmarks on a Pi 5 — lives in the private repo at `investigations/2026-04-07-now-playing-detection.md`. The feature phasing (sessions DB, contextual recs, auto-nav) lives at `investigations/2026-04-07-now-playing-features.md`. The current implementation covers Phases 1–3 (detector, top-bar indicator, home hero).

## Robustness defenses

- **Two-poll debounce.** A state change is only published after two consecutive ticks agree on `(pid, system, filename)`. This filters the ~2 s core-transition window where `/proc/<pid>/mem` returns truncated or mixed reads. `NotRunning` is exempt — losing the PID is unambiguous.
- **"Core loaded but path missing" → `Menu`.** During a fresh launch the core appears in `maps` before the active ROM path is allocated in heap. We hold the user at `Menu` (not `NotRunning`) so the UI doesn't flap.
- **In-core game switch.** Switching from one game to another on the same core (e.g. Sonic 1 → Sonic 2 on `genesis_plus_gx`) doesn't change `/proc/<pid>/maps`. The detector therefore re-walks the heap on every tick — the heap content is the only signal that distinguishes ROMs within a single core. There is no fast-path that skips the walk on "unchanged maps".
- **PID cache.** The cached PID is re-verified via `/proc/<pid>/comm`; only a cache miss falls back to the full `/proc` walk. Saves the dirent scan on the steady state.
- **Service restart resets elapsed.** When the cached PID disappears or `comm` no longer reads `replay`, the session timer is reset on the next confirmed `Playing`.

## Performance notes

The supported cores keep the `replay` heap at ~50 MB. The detector reads it through a 1 MB chunked buffer with a small overlap; cost on Pi 5 is ~80–120 ms per 4 s tick, with detector RSS around 2–3 MB. Polling cadence is 4 s.

When nothing changes between ticks, `AppState::set_now_playing` short-circuits on equality and the broadcast doesn't fire. The wire format omits any per-tick fields (no `elapsed_secs`); clients derive elapsed time from `started_at_unix_secs` plus a single shared 1 s clock signal in `replay-control-app/src/hooks/use_clock.rs`.

## Plumbing

- **Server**: detector loop spawned in `BackgroundManager::start` (Linux-gated). Broadcasts via `state.now_playing_tx`.
- **SSR seed**: `Resource::new_blocking(|| (), |_| get_initial_now_playing())` reads `AppState.now_playing()` during render and serializes it into the HTML so hydration adopts the same value with no flash.
- **Live updates**: `SseNowPlayingListener` (in `replay-control-app/src/lib.rs`) consumes `/sse/now-playing` and writes new states straight into the same `Resource` via `Resource::set` — single source of truth.
- **Consumer hook**: `use_now_playing()` returns a `Signal<NowPlayingState>` derived from the resource.
- **Live elapsed**: `use_live_elapsed_secs(started_at)` returns a `Signal<u64>` driven by the shared `Clock`.

## Manual deep link

The home hero card's **Manual** button links to `/games/{system}/{filename}#manuals`. The `MANUALS_FRAGMENT` constant (`replay-control-app/src/pages/game_detail.rs`) is read on the destination page; `use_focus_scroll` (`replay-control-app/src/hooks/use_focus_scroll.rs`) installs a `ResizeObserver` on `<body>` and re-issues `scrollIntoView` until the user manually scrolls (wheel, touch, keydown) — necessary because cover-art images and lazily-rendered sections finish laying out *after* the initial scroll, pushing the target down.
