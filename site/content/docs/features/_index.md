---
title: "Features"
description: "Replay Control feature documentation."
weight: 10
toc: true
layout: single
---


Replay Control is a web-based companion app for [RePlayOS](https://www.replayos.com/) — a browser UI for managing your game library, launching games, and configuring your retro gaming Pi from any device.

**Offline-first by design.** Metadata for ~34K console ROMs and ~15K arcade games works out of the box. Online sources (LaunchBox, libretro-thumbnails, Wikidata) optionally add descriptions, ratings, series data, and box art.

---

## Getting Started

- [Getting Started](getting-started.md) — Prerequisites, quick install, first launch, adding ROMs
- [Installation](install.md) — All install methods, update, uninstall, environment configuration

## Browse & Play

- [Game Library](game-library.md) — Browse systems, open game lists, manage ROM files, and understand automatic library refresh
- [Favorites](favorites.md) — Build a personal shortlist, organize it by system/genre/developer, and power favorite-based discovery
- [Recently Played](recents.md) — Resume games from the home page through Last Played and Recently Played sections
- [Now Playing](now-playing.md) — Live indicator showing the active game on the appliance, with elapsed time and quick actions
- [Search](search.md) — Fast cross-system search with filters, developer pages, and random game
- [Game Detail](game-detail.md) — Box art, screenshots, game info, launch on TV, videos, manuals, series navigation, and variant management
- [Arcade Boards](arcade-boards.md) — Browse, search, and get recommendations by arcade hardware board (CPS-2, Neo Geo MVS, …)
- [Game Series](game-series.md) — Wikidata-powered series data with sequel/prequel navigation across systems
- [Recommendations](recommendations.md) — Genre-diverse picks, top rated, multiplayer, curated spotlights, and discover pills

## Data & Media

- [Library Management and Metadata](library-management.md) — Rescan/rebuild the library, refresh metadata, inspect coverage, export CSV reports, and clean up media
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
