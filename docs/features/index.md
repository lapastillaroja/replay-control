# Features

Replay Control is a web-based companion app for [RePlayOS](https://www.replayos.com/), a retro gaming operating system for the Raspberry Pi. It runs as a local server on the Pi and provides a browser UI for managing your game library, launching games, and configuring your setup — all from your phone, tablet, or desktop. Think of it as the control center for your retro gaming Pi: browse your collection, launch games on the TV, enrich your library with metadata and artwork, and tweak system settings without ever needing SSH or a terminal.

**Offline-first by design.** Display names, genres, player counts, developer, and year data for ~34K console ROMs and ~15K playable arcade games are available out of the box — no internet required. Online sources ([LaunchBox](https://gamesdb.launchbox-app.com/), [libretro-thumbnails](https://github.com/libretro-thumbnails), [Wikidata](https://www.wikidata.org/)) can optionally enrich the library with descriptions, ratings, series data, and box art when connected.

> **A personal project.** Replay Control is built by one person who loves retro gaming and wanted a better companion app for RePlayOS. Feature requests and ideas are always welcome, but the roadmap follows personal preferences and what makes the app better for its core use case.

---

## Browse & Play

Find games, explore your collection, and launch on the TV.

### Home Page

Last played hero card, recently played games, library stats, systems overview, and personalized recommendations — genre-diverse picks, top rated, multiplayer, favorites-based suggestions, rotating curated spotlights, and discover pills. [Detail](recommendations.md)

### Game Library

Browse systems, manage ROMs with favorites, inline rename, and delete. Arcade display names for ~15K entries, automatic M3U multi-disc handling, and automatic library updates on local storage. Consistent game cards across all views. [Detail](game-library.md)

### Game Detail

Box art, screenshot gallery, game info, launch on TV, user screenshots, video search and pinning, box art swap, game series navigation, alternate versions, cross-system availability, related games, game manuals, and smart multi-file management. [Detail](game-detail.md), [Series detail](game-series.md)

### Global Search

Fast cross-system search across 23K+ games with fuzzy matching, filters (genre, driver status, favorites, rating, year range, multiplayer, co-op), developer search with dedicated developer pages, recent searches, and random game. [Detail](search.md)

## Personalize

Curate your collection and make it yours.

### Favorites

Featured card, recently added, per-system cards, flat/grouped views, organize by developer/genre/system/players/rating (up to 2 levels), and favorites-based recommendations. [Detail](game-library.md)

### Recommendations

Genre-diverse picks, top rated, multiplayer suggestions, favorites-based "Because You Love" and "More from Series" sections, rotating curated spotlights, and discover pills on the home page. [Detail](recommendations.md)

### Preferences

Region and language preference, font size, 11 built-in skins with sync mode and manual override, GitHub API key for higher rate limits. [Detail](settings.md)

## Manage

Enrich your library with metadata and artwork, and keep everything running smoothly.

### Metadata & Thumbnails

One-click LaunchBox import for descriptions and ratings, per-system or batch image downloads from libretro-thumbnails, embedded Wikidata series data, and cache management tools. [Detail](metadata.md), [Thumbnails detail](thumbnails.md)

### Storage

Auto-detects SD, USB, NVMe, or NFS storage. Automatic library refresh on storage changes, smart filesystem adaptation, corruption recovery, and portable app data in `.replay-control/`. [Detail](storage.md)

### Auto-Updates

Check for updates from the web UI with stable and beta channels, automatic background checks, one-click install with progress tracking and rollback support. [Detail](updates.md)

## Configure

Set up your Pi directly from the browser — no SSH required.

### WiFi, NFS & Pi Setup

Configure WiFi, NFS shares, hostname, and SSH password from the browser. View system info and logs, restart or reboot the Pi. [Detail](configuration.md)

## Technical

### User Experience

Streaming SSR with skeleton loaders, installable PWA with offline fallback, responsive mobile-first design, and i18n infrastructure.

### Libretro Core (Proof of Concept)

> A technical experiment for displaying game library data on the TV via the RePlayOS frontend. [Detail](libretro-core.md)

---

## Feature Documentation

| Document | Coverage |
|----------|----------|
| [Getting Started](getting-started.md) | Prerequisites, quick install, first launch, adding ROMs |
| [Installation](install.md) | All install methods, update, uninstall, environment configuration |
| [Game Library](game-library.md) | System browsing, game actions, favorites, recents, region preference, automatic updates |
| [Game Detail](game-detail.md) | Box art, game info, launch, videos, manuals, series, variants, actions |
| [Game Series](game-series.md) | Wikidata series data, sequel/prequel navigation, cross-system matching |
| [Metadata](metadata.md) | Embedded databases, LaunchBox import, ROM classification |
| [Recommendations](recommendations.md) | Home page and favorites recommendation engine, spotlight rotation, deduplication |
| [Search](search.md) | Global search, developer search, developer game list page |
| [Storage](storage.md) | Storage detection, automatic updates, config boundary |
| [Thumbnails](thumbnails.md) | Box art, screenshots, title screens, image matching, box art swap |
| [WiFi, NFS & Pi Setup](configuration.md) | WiFi setup, NFS shares, hostname, password, system info, logs |
| [Preferences](settings.md) | Region, language, text size, skin/theme, GitHub API key |
| [Auto-Updates](updates.md) | Update channels, automatic checks, one-click install |
| [Libretro Core](libretro-core.md) | Recently played viewer, TV display |
| [Benchmarks](benchmarks.md) | Performance measurements on Raspberry Pi 5 |

## Architecture Documentation

For implementation details, database schemas, caching strategies, and design decisions, see the [Architecture](../architecture/index.md) section:

| Document | Coverage |
|----------|----------|
| [Architecture Overview](../architecture/index.md) | Crate structure, key file paths |
| [Technical Foundation](../architecture/technical-foundation.md) | Stack, embedded databases, ROM identification, cross-compilation |
| [Design Decisions](../architecture/design-decisions.md) | Performance design decisions, memory budget, rejected alternatives |
| [Database Schema](../architecture/database-schema.md) | SQLite tables, indexes, migrations |
| [Connection Pooling](../architecture/connection-pooling.md) | Database pool setup and filesystem safety |
| [Server Functions](../architecture/server-functions.md) | SSR, streaming, resource patterns, response cache |
| [Startup Pipeline](../architecture/startup-pipeline.md) | Background initialization phases |
| [Enrichment](../architecture/enrichment.md) | Box art, genre, rating population pipeline |
| [ROM Classification](../architecture/rom-classification.md) | Filename parsing and tier assignment |
| [Activity System](../architecture/activity-system.md) | Mutual exclusion and progress broadcasting |

---

## Third-Party Attribution

For full attribution of third-party data sources and tools, see [NOTICES.md](../../NOTICES.md).
