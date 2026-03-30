# Replay Control

A companion web app for **RePlayOS** to manage ROMs, favorites, and settings from any device on the local network.

## About RePlayOS

RePlayOS is a Linux distribution featuring a custom libretro frontend for retro gaming on Raspberry Pi, with LCD and CRT support.

**Official site:** https://www.replayos.com/

## Features

See [`docs/features/`](docs/features/index.md) for detailed documentation per feature. Highlights:

- **ROM browsing & management** — browse by system, search, rename, delete, favorites
- **Game metadata** — embedded databases for ~34K console ROMs and ~15K arcade games, plus LaunchBox XML import and libretro-thumbnails box art
- **Game detail** — box art, screenshots, videos, manuals, series navigation, variant sections
- **Global search** — cross-system fuzzy search with filters (genre, driver status, multiplayer, rating)
- **Game launching** — launch games on the TV from the web UI
- **Settings** — skin/theme sync, Wi-Fi, NFS, hostname, system logs
- **PWA** — installable, app shell caching, offline fallback
- **Libretro core** — displays recently played / favorites on the TV via gamepad, with box art and metadata

## Tech Stack

- **Rust** — single binary, cross-compiled for ARM (aarch64)
- **Leptos 0.7** — SSR with WASM hydration
- **Axum** — HTTP server, REST API, SSE
- **SQLite** — metadata cache via deadpool-sqlite connection pool
- **No cargo-leptos** — custom `build.sh` (WASM + wasm-bindgen + server)

## Project Structure

```
replay-control/
├── replay-control-core/    — shared library (game DBs, ROM parsing, metadata, settings)
├── replay-control-app/     — web app (Leptos SSR + WASM hydration, Axum server)
├── replay-control-libretro/ — libretro core for TV display (.so)
├── scripts/                — data download scripts (No-Intro, TGDB, Wikidata)
├── tools/                  — analysis scripts, benchmarks, icon generation
├── docs/                   — feature documentation
├── research/               — investigations, plans, design docs
├── build.sh                — release build (WASM + server)
├── dev.sh                  — development (auto-reload, Pi deployment)
└── install.sh              — Pi installation (SSH or SD card)
```

## Build & Run

```bash
# Local development (auto-rebuild + reload)
./dev.sh --storage-path /path/to/roms

# Deploy to Pi
./dev.sh --pi [IP]

# Release build
./build.sh              # x86_64
./build.sh aarch64      # Pi cross-compile
```

## Routes

| Route | Page |
|---|---|
| `/` | Home (last played, recents, stats, systems, recommendations) |
| `/games/:system` | ROM list with search, filters, infinite scroll |
| `/games/:system/:filename` | Game detail (metadata, actions, media) |
| `/favorites` | Favorites (flat and grouped views) |
| `/search` | Global cross-system search |
| `/developer/:name` | Developer game list |
| `/more` | Settings, metadata, system info |

## Third-Party Resources

### Embedded Data (build time)
- **No-Intro DATs** — ROM identification, via [libretro-database](https://github.com/libretro/libretro-database) (MIT)
- **TheGamesDB** — game metadata, via [TheGamesDB](https://thegamesdb.net/) (GPLv3 codebase)
- **MAME / FBNeo** — arcade databases, via [libretro-database](https://github.com/libretro/libretro-database) (MIT/MAME License)
- **Wikidata** — game series relationships (CC0)

### Runtime Data (user-initiated downloads)
- **LaunchBox XML** — game descriptions, ratings, publishers ([launchbox-app.com](https://gamesdb.launchbox-app.com/)) — not redistributed, downloaded by user
- **libretro-thumbnails** — box art, screenshots, title screens ([GitHub](https://github.com/libretro-thumbnails)) — not redistributed, downloaded by user

### UI Assets
- **System controller icons** — [KyleBing/retro-game-console-icons](https://github.com/KyleBing/retro-game-console-icons) (GPLv3)
- **Phosphor Icons** — top bar icons ([phosphoricons.com](https://phosphoricons.com/)) (MIT)

## Documentation

- [`docs/features/`](docs/features/) — detailed feature documentation (game library, search, metadata, thumbnails, series, recommendations, storage, libretro core)
- [`docs/features/index.md`](docs/features/index.md) — full feature list with per-page breakdown

## AI Transparency

This project was developed with significant AI assistance (primarily Claude by Anthropic). The author reviews, understands, tests, and maintains all code. See [AI_POLICY.md](AI_POLICY.md) for contribution guidelines.

## Not Tested Systems

The following systems have no ROMs on the test device and have not been verified:

- Arcade (MAME 2003+)
- Atari 2600, 5200, 7800, Jaguar, Lynx
- Commodore 64, Amiga CD
- MSX
- PC Engine / TurboGrafx-16, PC Engine CD
- Nintendo DS, Game Boy, Game Boy Color
- 3DO, Philips CD-i
- ZX Spectrum
- Neo Geo, Neo Geo CD, Neo Geo Pocket
