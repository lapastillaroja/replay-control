# Features

Replay Control is a web-based companion app for [RePlayOS](https://www.replayos.com/) — a browser UI for managing your game library, launching games, and configuring your retro gaming Pi from any device.

**Offline-first by design.** Metadata for ~34K console ROMs and ~15K arcade games works out of the box. Online sources (LaunchBox, libretro-thumbnails, Wikidata) optionally add descriptions, ratings, series data, and box art.

---

## Getting Started

- [Getting Started](getting-started.md) — Prerequisites, quick install, first launch, adding ROMs
- [Installation](install.md) — All install methods, update, uninstall, environment configuration

## Browse & Play

- [Game Library](game-library.md) — Browse systems, manage ROMs with favorites, rename, delete, and automatic library updates
- [Search](search.md) — Fast cross-system search with filters, developer pages, and random game
- [Game Detail](game-detail.md) — Box art, screenshots, game info, launch on TV, videos, manuals, series navigation, and variant management
- [Game Series](game-series.md) — Wikidata-powered series data with sequel/prequel navigation across systems
- [Recommendations](recommendations.md) — Genre-diverse picks, top rated, multiplayer, curated spotlights, and discover pills

## Data & Media

- [Metadata](metadata.md) — LaunchBox import for descriptions and ratings, embedded Wikidata series data
- [Thumbnails](thumbnails.md) — Box art, screenshots, and title screens from libretro-thumbnails

## Settings & System

- [Settings](settings.md) — Region, language, text size, skin/theme sync, first-run setup checklist
- [WiFi, NFS & Pi Setup](configuration.md) — WiFi, NFS shares, hostname, SSH password, system info, logs, restart/reboot
- [Storage](storage.md) — Auto-detects SD/USB/NVMe/NFS storage, automatic library refresh, corruption recovery
- [Auto-Updates](updates.md) — Stable and beta channels, one-click install with rollback support

## Technical

- [Benchmarks](benchmarks.md) — Performance measurements on Raspberry Pi 5
- [Libretro Core](libretro-core.md) — Proof of concept for TV display via RePlayOS frontend

---

For implementation details and design decisions, see the [Architecture](../architecture/index.md) section.

For third-party attribution, see [NOTICES.md](../../NOTICES.md).
