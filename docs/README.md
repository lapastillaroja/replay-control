# Documentation

Replay Control documentation, organized into two sections:

- **[Features](features/index.md)** — what the app does, from a user perspective
- **[Architecture](architecture/index.md)** — how it works under the hood

## Features

| Doc | Covers |
|-----|--------|
| [Getting Started](features/getting-started.md) | Prerequisites, quick install, first launch, adding ROMs |
| [Installation](features/install.md) | All install methods, update, uninstall, environment configuration |
| [Game Library](features/game-library.md) | System browsing, game actions, favorites, recents, region preference |
| [Game Series](features/game-series.md) | Series navigation, sequel/prequel links, cross-system matching |
| [Metadata](features/metadata.md) | Embedded databases, LaunchBox import, ROM classification |
| [Recommendations](features/recommendations.md) | Home page and favorites recommendations, spotlight rotation |
| [Search](features/search.md) | Global search, developer search, developer game list |
| [Storage](features/storage.md) | Storage detection, automatic updates, config boundary |
| [Thumbnails](features/thumbnails.md) | Box art, screenshots, title screens, image matching |
| [Settings](features/settings.md) | System configuration, user preferences |
| [Benchmarks](features/benchmarks.md) | Performance measurements on Raspberry Pi |
| [Libretro Core](features/libretro-core.md) | TV display proof of concept |

## Architecture

| Doc | Covers |
|-----|--------|
| [Overview](architecture/index.md) | Crate structure, key components |
| [Design Decisions](architecture/design-decisions.md) | Performance decisions, memory budget, rejected alternatives |
| [Technical Foundation](architecture/technical-foundation.md) | Stack, embedded databases, cross-compilation |
| [Database Schema](architecture/database-schema.md) | Tables, indexes, migrations |
| [Connection Pooling](architecture/connection-pooling.md) | Pool setup, WriteGate, journal modes |
| [Server Functions](architecture/server-functions.md) | SSR, streaming, caching |
| [Startup Pipeline](architecture/startup-pipeline.md) | Background initialization phases |
| [Enrichment](architecture/enrichment.md) | Box art, genre, rating population |
| [ROM Classification](architecture/rom-classification.md) | Filename parsing, tier assignment |
| [Activity System](architecture/activity-system.md) | Mutual exclusion, progress broadcasting |
