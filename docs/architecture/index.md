# Architecture

Replay Control is a Leptos 0.7 SSR web application for managing retro game libraries on RePlayOS. The codebase is split into three crates inside a Cargo workspace.

---

## Documents

- [Technical Foundation](technical-foundation.md) — Crates, stack, embedded databases, ROM identification, cross-compilation, key file paths
- [Design Decisions](design-decisions.md) — Performance decisions, memory budget, rejected alternatives
- [Startup Pipeline](startup-pipeline.md) — Background initialization phases
- [Library Build Pipeline](library-build-pipeline.md) — Scan/rescan/rebuild design, deferred identity, temporary-table reconcile
- [Database Schema](database-schema.md) — SQLite tables, indexes, and migrations
- [Server Functions](server-functions.md) — Leptos SSR, resource patterns, caching
- [Connection Pooling](connection-pooling.md) — deadpool-sqlite setup and exFAT safety
- [Enrichment](enrichment.md) — Box art, genre, rating population pipeline
- [Arcade Boards](arcade-boards.md) — Board attribution from MAME/FBNeo sourcefiles, the fused spelling table, the board-merge priority
- [ROM Classification](rom-classification.md) — Filename parsing and tier assignment
- [Activity System](activity-system.md) — Mutual exclusion and progress broadcasting
