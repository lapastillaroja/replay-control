# Replay

**Replay Control** — a companion web app for **RePlayOS** to manage ROMs, favorites, and settings from any device on the local network.

## About RePlayOS

RePlayOS is a **Linux distribution** featuring a custom **libretro frontend** designed to emulate classic game consoles, arcade machines, and computers. It is optimized for **Raspberry Pi** boards with both LCD and CRT screen support.

**Official site:** https://www.replayos.com/

### Key Technical Details

- **Platform:** Raspberry Pi (64-bit CPU models, KMS/DRM, OpenGL ES 3.X)
- **Frontend:** Custom libretro-based frontend, written in **C** (not open source)
- **Cores:** Uses libretro cores (both GL and Non-GL), pre-selected and pre-configured per Pi model
- **Hardware requirement:** Any Raspberry Pi with 1 GB+ RAM (performance limited by CPU/GPU, not RAM)
- **Auto-detects** Pi model — same SD card works across supported models

### Display / Video

- **DynaRes 2.0 engine** for CRT support (native timings, on-the-fly resolution changes)
- Supports LCD and CRT screens, single or dual screen configurations
- CRT profiles: consumer TVs, PC 31kHz, Arcade 15/25/31kHz
- NRR (Native Refresh Rate) mode on LCD screens
- AmbiScan: dynamic colored borders feature
- Ultra-low-latency: 0-1 frame input lag without runahead

### Configuration

- Main config file: **`/media/sd/config/replay.cfg`**
- Key settings:
  - `video_connector`: 0=HDMI, 1=DPI (GPIO)
  - `video_mode`: 0-9 (320x240@NRR to 3840x2160@60)
  - `video_monitor_multi_mode`: dual screen options (cloned, horizontal, vertical, smart rotation)
  - `video_lcd_type`: generic_60 or gaming_vrr
  - `video_crt_type`: generic_15, arcade_15, arcade_15_25, arcade_15_25_31, arcade_31
  - `video_crt_csync_mode`: 0=AND, 1=XOR, 2=separated H/V

### File Structure & Storage

- ROMs go in `/roms` folder, system folders prefixed by company name
- Special folders: `_autostart`, `_extra`, `_favorites`, `_recent`
- M3U files for multi-disc game management (hides individual disc files)

### Storage / Network Options

- **Local:** FAT partition named "replay" on MicroSD, accessible from PC
- **NFS v4 share:** configured in replay.cfg (server IP + share path, needs r/w)
- **SFTP:** for network file transfer (requires Pi's IP address)

### Utility Cores

- **PiBench:** CPU performance measurement
- **Screen Test:** CRT geometry and color range checking
- **Alpha Player:** custom media player for video/audio files

### Emulated System Categories

- Arcade
- Consoles
- Computers
- Handhelds

(Full list at https://www.replayos.com/systems/)

---

## Project Goal

**Web application** running directly on the RePlayOS Raspberry Pi. Accessible from **any device** (phone, tablet, desktop) via browser on the local network.

**Project name:** Replay

---

## Current Status

See `docs/features.md` for detailed per-page tracking of implemented, planned, and future features.

### Implemented
- **ROM browsing & management** — browse by system, search, rename, delete, favorite toggle, infinite scroll with pagination
- **Arcade display names** — embedded PHF database with 28,593 entries (Flycast, FBNeo, MAME 2003+, MAME current)
- **Non-arcade game database** — embedded PHF maps for ~34K ROM entries across 20+ systems (No-Intro DATs, TheGamesDB, libretro-database)
- **ROM filename parsing** — regex parser for No-Intro and GoodTools naming conventions (title, region, revision, flags)
- **Favorites management** — view, add, remove, with hero card, grouped/flat views, optimistic UI
- **Game detail page** — metadata grid, arcade-specific info, favorite toggle, rename, delete
- **Game metadata** — SQLite cache with LaunchBox XML import (auto-download + parse), libretro-thumbnails box art import with cancel/stop support, per-system coverage stats, real-time SSE progress
- **Settings pages** — skin/theme sync, Wi-Fi, NFS, hostname, metadata management, system logs viewer
- **Home page** — last played, recently played, library stats, systems overview
- **Installation** — `install.sh` supports SSH and SD card deployment methods

### Not Yet Implemented
- **Screenshots browser** — browsing and managing RePlayOS screenshots (see `docs/reference/screenshots-analysis.md`)
- **Game launching** — launching games from the web UI (see `docs/reference/game-launching.md`)
- **Remote control** — triggering actions on RePlayOS from the web UI
- **Backup & sync** — backup ROM library, save states, config
- **RetroAchievements integration** — show earned achievements per game
- **Game manuals viewer** — read game manuals from the web UI
- **CI/CD pipeline** — automated cross-compilation and GitHub Releases (see `docs/reference/binary-distribution.md`, `docs/reference/deployment.md`)

---

## Design Decisions

### Language: **Rust**
- Single binary deployment — no runtime dependencies on the Pi
- Cross-compiled for ARM (aarch64)
- Strong ecosystem for web (axum), async I/O (tokio), and WASM (Leptos)

### Interface: **Web app (SSR) + CLI**
- Primary UI: Leptos SSR (Server-Side Rendering) with hydration
  - Server pre-renders HTML for fast initial page loads
  - Client WASM hydrates for interactivity after load
  - Data fetching via Leptos server functions (no HTTP round-trip on server, automatic HTTP on client)
  - Client-side routing via `leptos_router` — proper browser history (back/forward), bookmarkable URLs
- REST API preserved alongside SSR for external/programmatic access
- Responsive design — works on mobile, tablet, and desktop browsers
- CLI mode for scripting and power users (same binary)
- Access via `http://replaypi.local` or Pi's IP address

### Routes
- `/` — Home (last played, recents, library stats, systems overview)
- `/games` — Systems grid (all systems with game counts)
- `/games/:system` — ROM list for a system (search, favorite toggle, rename, delete)
- `/games/:system/:filename` — Game detail (metadata, actions, arcade info)
- `/favorites` — Favorites (flat and grouped views)
- `/favorites/:system` — System-specific favorites
- `/more` — Settings and system info
- `/more/skin` — Skin/theme selection and sync
- `/more/wifi` — Wi-Fi configuration
- `/more/nfs` — NFS share settings
- `/more/hostname` — Hostname configuration
- `/more/metadata` — Metadata import, coverage, and cache management
- `/more/logs` — System logs viewer (journalctl)

### Internationalization (i18n)
- Built-in i18n support with English as default language
- Lightweight manual approach: translation keys in `i18n.rs`, locale context provided at App root
- All UI strings are translation-ready via `t(locale, "key")` calls
- To add a new language: add a variant to the `Locale` enum and add match arms in the `t()` function
- No external i18n crate dependency — can migrate to `leptos_i18n` later if needed

### Deployment: **Single binary on the Pi**
- Runs as a systemd service on RePlayOS
- Serves both the REST API and the web UI (SSR + WASM hydration)
- Easy install: single ARM binary + site assets + setup script
- Auto-discovery via mDNS/Avahi (e.g., `replaypi.local`)

---

## Architecture

```
┌─────────────────────┐         ┌──────────────────────────────────────┐
│   ANY DEVICE        │         │   RASPBERRY PI (RePlayOS)            │
│   (phone/tablet/PC) │  HTTP   │                                      │
│                     │         │   ┌──────────────────────────────┐   │
│   ┌─────────────┐   │────────►│   │  replay-control-app (single binary) │   │
│   │  Browser    │   │         │   │                              │   │
│   │  (Leptos    │   │◄────────│   │  ┌────────┐  ┌───────────┐  │   │
│   │   WASM)     │   │         │   │  │ Web UI │  │ REST API  │  │   │
│   └─────────────┘   │         │   │  │ (axum) │  │ (axum)    │  │   │
│                     │         │   │  └────────┘  └─────┬─────┘  │   │
└─────────────────────┘         │   │                    │        │   │
                                │   │  ┌─────────────────▼─────┐  │   │
                                │   │  │ Core Services         │  │   │
                                │   │  │ - ROM file manager    │  │   │
                                │   │  │ - Config R/W          │  │   │
                                │   │  │ - System info         │  │   │
                                │   │  │ - Backup engine       │  │   │
                                │   │  │ - Metadata manager    │  │   │
                                │   │  └───────────────────────┘  │   │
                                │   └──────────────────────────────┘   │
                                │                                      │
                                │   RePlayOS filesystem:               │
                                │   /roms, /media/sd/config, etc.      │
                                └──────────────────────────────────────┘
```

### Two Crates (SSR Architecture)

Since everything runs on the Pi, the app is a single merged crate with SSR:

1. **`replay-control-core`** (library crate)
   - ROM file operations (list, upload, delete, rename, move, dedup)
   - RePlayOS config parser (replay.cfg)
   - System info (storage, Pi model, network)
   - Metadata management (pluggable providers)
   - Backup engine

2. **`replay-control-app`** (binary + library crate, dual-feature)
   - **`ssr` feature:** Axum web server with SSR rendering + REST API + server functions
   - **`hydrate` feature:** WASM client for browser hydration
   - Components, pages, types, and i18n are shared between both features
   - `replay-control-core` is only compiled with `ssr` (it uses `std::fs`, not WASM-compatible)
   - Server functions (`#[server]`) bridge data fetching: direct calls on server, HTTP on client
   - CLI mode via clap (`replay-control-app cli <command>`)
   - systemd service integration

### Cargo Workspace Structure

```
replay/
├── Cargo.toml              (workspace: replay-control-core, replay-control-app)
├── build.sh                (builds WASM + server)
├── replay-control-core/            (library — business logic, native only)
├── replay-control-app/             (merged server + frontend)
│   ├── Cargo.toml          (features: ssr, hydrate)
│   ├── src/
│   │   ├── main.rs         (server entry, #[cfg(feature = "ssr")])
│   │   ├── lib.rs          (App component + hydrate entry)
│   │   ├── i18n.rs         (internationalization)
│   │   ├── server_fns.rs   (Leptos server functions)
│   │   ├── types.rs        (mirror types for client)
│   │   ├── api/            (REST API handlers, ssr-only)
│   │   ├── components/     (shared UI components)
│   │   └── pages/          (shared page components)
│   └── style/
│       └── style.css
├── dev.sh                  (auto-rebuild dev server)
└── README.md
```

### Browser Support

| Browser | Desktop | Mobile | PWA Install |
|---------|---------|--------|-------------|
| Firefox | Yes     | Yes    | Android only |
| Chrome  | Yes     | Yes    | Yes          |
| Safari  | Yes     | Yes    | Yes (Add to Home Screen) |

The app is a Progressive Web App (PWA) — installable on mobile and desktop for an app-like experience (standalone window, no browser chrome).

### Build Process (without cargo-leptos)

```bash
./build.sh   # Builds WASM (hydrate) + wasm-bindgen + server (ssr)
```

Output:
- `target/release/replay-control-app` — server binary
- `target/site/pkg/` — WASM + JS glue
- `target/site/style.css` — stylesheet

---

## Decided

- **Frontend:** Leptos SSR (Server-Side Rendering + WASM hydration) — full-stack Rust, no JavaScript dependency
- **Internationalization:** Built-in i18n with English default; all UI strings are translation-ready
- **Metadata:** LaunchBox XML as primary text metadata source (descriptions, ratings, publisher). libretro-thumbnails for box art images. Both are free, bulk-downloadable, and work offline. ScreenScraper as a potential future API-based source for richer media.
- **Authentication:** TBD (pairing token, password, etc.)

## Data Strategy

- **Filesystem is the source of truth** for what ROMs exist
- ROM lists, favorites, and recents are scanned from the filesystem on each request, with in-memory TTL caching
- **Embedded metadata** — arcade DB (~28K entries) and non-arcade game DB (~34K entries) are compiled into the binary as PHF maps, providing display names, year, genre, developer, and players without any external data
- **SQLite metadata cache** — `metadata.db` at `<storage>/.replay-control/metadata.db` stores imported text metadata (LaunchBox XML) and image references (libretro-thumbnails). Uses `nolock` VFS fallback for NFS mounts.
- **Future: full SQLite cache** — replace filesystem scanning with an indexed database, populated by background scan, updated via inotify

## How RePlayOS Manages Game Lists

- **Non-arcade systems:** no database — games are read directly from the filesystem, displayed by filename
- **Arcade (MAME/FBNeo):** internal DB embedded in the `replay` binary, auto-generated from FBNeo/MAME DAT files + adb.arcadeitalia.net. Maps ROM zip names to display names, orientation, players, buttons, controller type. Not user-editable.
- **Our app** scans the filesystem independently — no dependency on RePlayOS's internal DB
- For arcade metadata, we use the same public sources (MAME DATs, FBNeo DATs, Flycast CSV, catver.ini)

## Open Questions

- Backup format: tarball, incremental, or custom?
