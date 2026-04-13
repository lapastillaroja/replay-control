---
title: "Architecture"
description: "Replay Control architecture and implementation details."
weight: 100
toc: true
layout: single
---



Replay Control is a Leptos 0.7 SSR web application for managing retro game libraries on RePlayOS. The codebase is split into three crates inside a Cargo workspace.

## Crates

### replay-control-core (library)

Pure Rust library with no web framework dependency. Contains:

- **Game databases**: embedded arcade_db, game_db (genres, ratings, players) compiled via `phf` at build time
- **ROM parsing**: filename tag extraction, tier classification, region detection, base title extraction, series key computation
- **Metadata storage**: SQLite schema and queries for `metadata.db` (game library, metadata cache, thumbnail index) and `user_data.db` (user customizations)
- **Image matching**: multi-tier fuzzy matching (exact, case-insensitive, base_title, version-stripped) for resolving ROM filenames to thumbnail paths
- **Configuration**: `replay.cfg` parser, settings reader/writer
- **Systems catalog**: static list of supported systems with display names, folder names, core assignments
- **Thumbnails**: libretro-thumbnails repo mapping, on-demand download logic, manifest parsing

Feature-gated: the `metadata` feature enables SQLite (`rusqlite`) and XML parsing (`quick-xml`).

### replay-control-app (web application)

Leptos 0.7 SSR + WASM hydration app built on Axum. Contains:

- **Server functions**: ~70 registered server functions for all UI data needs
- **API layer** (`src/api/`): AppState, connection pools, background pipeline, activity system, game library cache, enrichment, import/thumbnail pipelines
- **Pages** (`src/pages/`): home, system browser, game detail, favorites, settings, metadata management, search
- **Components** (`src/components/`): reusable UI components (hero cards, game rows, skeleton loaders, modals)
- **Internationalization**: runtime i18n with locale-keyed translation strings

### replay-control-libretro (TV display core)

Standalone cdylib (not in the workspace) that implements the libretro API. Runs as a RetroArch core on the TV, fetching game detail data from the companion app's HTTP API via `minreq`. Renders box art using the `png` crate. Lightweight by design -- no web framework, no SQLite.

## Sub-documents

- [Technical Foundation](technical-foundation.md) -- stack, embedded databases, ROM identification, cross-compilation
- [Startup Pipeline](startup-pipeline.md) -- background initialization phases
- [Database Schema](database-schema.md) -- SQLite tables, indexes, and migrations
- [Connection Pooling](connection-pooling.md) -- deadpool-sqlite setup and exFAT safety
- [Activity System](activity-system.md) -- mutual exclusion and progress broadcasting
- [Enrichment](enrichment.md) -- box art, genre, rating population pipeline
- [ROM Classification](rom-classification.md) -- filename parsing and tier assignment
- [Server Functions](server-functions.md) -- Leptos SSR, resource patterns, caching
- [Design Decisions](design-decisions.md) -- performance decisions, memory budget, rejected alternatives

## Key File Paths

| Concern | Path |
|---------|------|
| App entry point | `replay-control-app/src/main.rs` |
| AppState + pools | `replay-control-app/src/api/mod.rs` |
| Background pipeline | `replay-control-app/src/api/background.rs` |
| Activity system | `replay-control-app/src/api/activity.rs` |
| Enrichment | `replay-control-app/src/api/cache/enrichment.rs` |
| Image resolution | `replay-control-app/src/api/cache/images.rs` |
| DB schema | `replay-control-core/src/metadata/metadata_db/mod.rs` |
| User data DB | `replay-control-core/src/metadata/user_data_db.rs` |
| ROM tag parsing | `replay-control-core/src/game/rom_tags.rs` |
| Image matching | `replay-control-core/src/metadata/image_matching.rs` |
