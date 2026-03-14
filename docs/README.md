# Documentation Index

Replay Control companion app documentation, organized into two tiers within `docs/` (features and reference), plus a separate `research/` directory at the repository root for internal development work.

---

## Features (current-state developer docs)

Start here when joining the project. Each doc describes how a feature works today, with key source files and architecture decisions.

| Doc | Covers |
|-----|--------|
| [index.md](features/index.md) | Feature tracker and backlog (checklist format) |
| [game-library.md](features/game-library.md) | Three-tier cache (L1 in-memory, L2 SQLite, L3 filesystem), ROM scanning, display name resolution, enrichment, filesystem watching, cache invalidation |
| [metadata.md](features/metadata.md) | Embedded databases (arcade_db ~28K entries, game_db ~34K entries), LaunchBox XML import, genre fallback, unified GameInfo API, ROM tag parsing |
| [thumbnails.md](features/thumbnails.md) | Thumbnail index (manifest), 5-tier fuzzy matching, arcade multi-repo images, on-demand download with SSE progress, box art swap |
| [recommendations.md](features/recommendations.md) | Home page recommendation blocks (random, top-rated, multiplayer, favorites-based, related), dedup CTE pattern, ROM tag filters |
| [search.md](features/search.md) | Global search scoring algorithm, genre/driver-status/favorites filters, recent searches, random game |
| [rom-organization.md](features/rom-organization.md) | Favorites (.fav files), recents (.rec files), region preference, game launching (autostart mechanism), favorites organization |
| [storage.md](features/storage.md) | Storage detection (USB/NFS/SD), StorageKind, config/ROM watchers (inotify + polling), config boundary (replay.cfg vs settings.cfg), .replay-control/ directory |
| [game-launching.md](features/game-launching.md) | Game launching implementation guide |

---

## Reference (stable specs, data, benchmarks)

Long-lived documents that describe how things are built, specifications, and benchmark data.

| Doc | Topic |
|-----|-------|
| [source-code-analysis.md](reference/source-code-analysis.md) | Full codebase walkthrough |
| [performance-benchmarks.md](reference/performance-benchmarks.md) | Before/after measurements |
| [compile-time-analysis.md](reference/compile-time-analysis.md) | Rust compile time breakdown |
| [integration-testing-analysis.md](reference/integration-testing-analysis.md) | Integration testing approach |
| [background-tasks.md](reference/background-tasks.md) | Background task system design (formal design not adopted) |
| [binary-distribution.md](reference/binary-distribution.md) | Binary distribution via GitHub |
| [deployment.md](reference/deployment.md) | Deployment on RePlayOS (build, install, service) |
| [replay-control-folder.md](reference/replay-control-folder.md) | .replay-control/ directory structure |
| [game-metadata.md](reference/game-metadata.md) | Metadata source evaluation and storage design |
| [rom-identification.md](reference/rom-identification.md) | ROM filename parsing specification |
| [rom-matching.md](reference/rom-matching.md) | ROM matching pipeline and coverage |

---

## Other top-level docs

| Doc | Purpose |
|-----|---------|
| [known-issues.md](known-issues.md) | Known issues and TODOs |

---

## Research (internal development work)

Development analyses, feasibility studies, and implementation plans live in the [`research/`](../research/) directory at the repository root. See [`research/README.md`](../research/README.md) for a full index.
