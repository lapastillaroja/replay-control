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

## Feature Priorities

### Phase 1 — Core (MVP)
- **ROM management & organization** — browse, add, remove, rename, organize ROMs on the Pi
  - View ROM library by system/category
  - Upload ROMs from any device via browser
  - Rename, move, delete ROMs
  - Detect duplicates
  - M3U multi-disc management
- **Favorites management**
  - View, add, remove favorites
  - Organize favorites by system — user can enable "group by system" which creates subfolders inside `_favorites/` (e.g., `_favorites/sega_smd/`, `_favorites/nintendo_n64/`)
  - When enabled, existing `.fav` files in the root `_favorites/` folder are automatically moved to the matching system subfolder (based on the `<system>@` prefix in the filename)
  - `.fav` filename convention stays the same inside subfolders
  - Reversible: user can disable grouping and flatten back to root
- **Game navigation** — browse and search installed games across all systems
  - Filter by system, name, format
  - Quick search

### Phase 2 — Enhancements
- **Remote control** — trigger actions on RePlayOS from the web UI
- **Backup & sync** — backup ROM library, save states, config
- **Game metadata** — box art, descriptions, ratings (pluggable sources)
- **RetroAchievements integration** — connect user's RetroAchievements account, show earned achievements per game while browsing the library

### Nice to Have
- **Wi-Fi & NFS configuration** — configure Wi-Fi networks and NFS share settings from the web UI (instead of manually editing replay.cfg)
- **Game recommendations** — suggest games based on user's library, favorites, play history, or genre/system preferences
- **Non-installed game search** — discover games not yet in the library (future)
- **Game videos** — on the game detail page, search for related videos (trailers, longplays, 1CC runs) from YouTube or other sources

### Installation / Setup

**RePlayOS first-boot process:**
- On first boot, RePlayOS creates and expands a new exFAT partition on the SD card
- This partition holds ROMs, BIOS, saves, and config (`/media/sd/config/replay.cfg`)
- Before first boot, only a FAT boot partition exists
- First boot is silent (black screen) and can take time — user must not power off

**Our setup approach:** Post-first-boot, SD-based installer
1. User burns RePlayOS image to SD card
2. User boots Pi — RePlayOS does first-boot setup (partitions, folders)
3. User removes SD card, plugs into computer
4. User runs our installer tool (simple CLI or script) which writes the binary + systemd service to the SD card's exFAT partition
5. User re-inserts SD, boots Pi — our app starts automatically

**Alternative (ideal, future):** Partner with RePlayOS to bundle the app in the image, or provide a hook in the FAT boot partition that runs post-first-boot setup automatically.

**Option B: SSH/SCP install (network)**
1. Pi is connected via ethernet or Wi-Fi is already configured
2. User transfers binary via SCP: `scp replay-app pi@<ip>:/path/`
3. User SSHs in and runs the setup script: `ssh pi@<ip> ./setup.sh`
4. App starts automatically

Both options (SD card and SSH/SCP) should be supported and documented.

### Phase 3 — Future
- **Game manuals viewer** — read game manuals directly from the web UI. Sources: internet downloads + official manual list from the RePlayOS Telegram group. Details TBD.
- Additional features TBD as the app matures

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
- `/favorites` — Favorites (flat and grouped views)
- `/more` — Settings and system info

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
│   ┌─────────────┐   │────────►│   │  replay-app (single binary) │   │
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

1. **`replay-core`** (library crate)
   - ROM file operations (list, upload, delete, rename, move, dedup)
   - RePlayOS config parser (replay.cfg)
   - System info (storage, Pi model, network)
   - Metadata management (pluggable providers)
   - Backup engine

2. **`replay-app`** (binary + library crate, dual-feature)
   - **`ssr` feature:** Axum web server with SSR rendering + REST API + server functions
   - **`hydrate` feature:** WASM client for browser hydration
   - Components, pages, types, and i18n are shared between both features
   - `replay-core` is only compiled with `ssr` (it uses `std::fs`, not WASM-compatible)
   - Server functions (`#[server]`) bridge data fetching: direct calls on server, HTTP on client
   - CLI mode via clap (`replay-app cli <command>`)
   - systemd service integration

### Cargo Workspace Structure

```
replay/
├── Cargo.toml              (workspace: replay-core, replay-app)
├── build.sh                (builds WASM + server)
├── replay-core/            (library — business logic, native only)
├── replay-app/             (merged server + frontend)
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
- `target/release/replay-app` — server binary
- `target/site/pkg/` — WASM + JS glue
- `target/site/style.css` — stylesheet

---

## Decided

- **Frontend:** Leptos SSR (Server-Side Rendering + WASM hydration) — full-stack Rust, no JavaScript dependency
- **Internationalization:** Built-in i18n with English default; all UI strings are translation-ready
- **Metadata:** Yes — show game info (box art, descriptions, ratings) for installed games and for discovery. RePlayOS already includes some metadata. Source TBD (ScreenScraper likely). Design the metadata layer to be pluggable.
- **Authentication:** Will have auth, specific mechanism TBD (pairing token, password, etc.). Design the API with auth middleware from the start but implement it later.

## Data Strategy

- **Filesystem is the source of truth** for what ROMs exist — no local database required for MVP
- All ROM lists, favorites, and recents are scanned from the filesystem on each request
- **Future: SQLite cache layer** — populated by a background scan on startup, updated incrementally via inotify when files change; needed when we add metadata or if performance becomes an issue on large libraries
- **Metadata fetched lazily** — when a user views a game detail page for the first time, fetch metadata from the configured provider and cache it in the DB
- No metadata integration or local DB until the core browsing experience is solid

## How RePlayOS Manages Game Lists

- **Non-arcade systems:** no database — games are read directly from the filesystem, displayed by filename
- **Arcade (MAME/FBNeo):** internal DB embedded in the `replay` binary, auto-generated from FBNeo/MAME DAT files + adb.arcadeitalia.net. Maps ROM zip names to display names, orientation, players, buttons, controller type. Not user-editable.
- **Our app** scans the filesystem independently — no dependency on RePlayOS's internal DB
- For arcade metadata, we can use the same public sources (MAME DATs, Arcade Italia)

## Open Questions

- Backup format: tarball, incremental, or custom?
- Metadata source: ScreenScraper, IGDB, No-Intro DATs, Arcade Italia, or combination?
