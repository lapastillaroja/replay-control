# Server Lifecycle

How the Replay Control server starts, runs, and handles background work.

---

## Startup

```
Process start (systemd)
     │
     ▼
┌─────────────────────────────────────────────┐
│  AppState::new()                            │
│                                             │
│  1. Detect storage (SD / USB / NFS)         │
│  2. Open metadata.db                        │
│     └─ create schema if fresh               │
│  3. Open user_data.db                       │
│     └─ create schema if fresh               │
│  4. Build connection pools                  │
│     ├─ detect filesystem via /proc/mounts   │
│     ├─ metadata: 3 read + 1 write (WAL)     │
│     │           1 read + 1 write (DELETE)   │
│     └─ userdata: same                       │
│  5. Warm pools (1 read + 1 write each)      │
│     └─ fail fast if DB inaccessible         │
│  6. Create GameLibrary cache                │
│  7. Create ImportPipeline + ThumbnailPipeline│
│     └─ share a single `scanning` AtomicBool  │
└─────────────────────────────────────────────┘
     │
     ▼
┌─────────────────────────────────────────────┐
│  BackgroundManager::start()                 │
│  └─ spawns pipeline + watchers (see below)  │
└─────────────────────────────────────────────┘
     │
     ▼
┌─────────────────────────────────────────────┐
│  Register ~80 server functions              │
│  Build Axum router                          │
│  axum::serve() — start accepting requests   │
└─────────────────────────────────────────────┘
```

The server accepts requests as soon as `axum::serve()` runs. The background
pipeline may still be running — the `scanning` flag and `MetadataBusyBanner`
component tell the UI that scanning is in progress.

---

## Background Pipeline

Runs once on startup in a `spawn_blocking` thread. Phases execute sequentially —
each waits for the previous to finish.

```
spawn_blocking(run_pipeline)
     │
     ▼
┌─ Phase 1: Auto-Import ─────────────────────┐
│  If launchbox-metadata.xml exists on disk:  │
│  • Start import (no enrichment)             │
│  • Wait until import finishes               │
│  • Checkpoint WAL                           │
│  If not: skip                               │
└─────────────────────────────────────────────┘
     │
     ▼
┌─ Phase 2: Cache Populate / Verify ──────────┐
│  busy = true, scanning = true               │
│                                             │
│  If L2 (SQLite) is empty (fresh DB):        │
│  • Scan ALL systems from filesystem         │
│  • Save to L2 + warm L1                     │
│  • Enrich (box art, metadata, ratings)      │
│                                             │
│  If L2 has data:                            │
│  • Check mtime for each system              │
│  • Rescan + re-enrich only stale systems    │
│                                             │
│  Checkpoint WAL                             │
│  busy = false, scanning = false             │
└─────────────────────────────────────────────┘
     │
     ▼
┌─ Phase 3: Thumbnail Index Rebuild ──────────┐
│  If images exist on disk but index is empty: │
│  • Rebuild thumbnail index from disk files  │
│  • Checkpoint WAL                           │
│  If not: skip                               │
└─────────────────────────────────────────────┘
     │
     ▼
┌─ Watchers ──────────────────────────────────┐
│  spawn_storage_watcher()                    │
│  └─ monitors replay.cfg for storage changes │
│     (60s poll fallback)                     │
│                                             │
│  spawn_rom_watcher() [local storage only]   │
│  └─ inotify on roms/ directory (3s debounce)│
│     detects: file add, rename, delete       │
│     triggers: invalidate + rescan + enrich  │
└─────────────────────────────────────────────┘
```

---

## Connection Pools

Two `DbPool` instances (metadata + userdata), each with separate read and write pools.

### Journal Mode Selection (Filesystem-Aware)

The journal mode is chosen based on the filesystem where the database resides,
detected at startup via `/proc/mounts`:

| Filesystem | Journal Mode | Reason |
|------------|-------------|--------|
| ext4, btrfs, xfs, f2fs, tmpfs | **WAL** | Full POSIX advisory locking and shared memory support |
| exFAT, FAT32 (vfat) | **DELETE** | WAL's `-shm` shared memory file doesn't work reliably; causes `SQLITE_IOERR_SHORT_READ` |
| NFS | **DELETE** (nolock VFS) | No POSIX file locking or shared memory support |

This means USB drives formatted as exFAT (common for cross-platform use) get
DELETE journal mode, just like NFS. Only USB/SD/NVMe drives formatted with a
POSIX-capable filesystem (ext4, etc.) get WAL mode with concurrent readers.

### WAL Mode (POSIX-Capable Local Filesystems)

| Setting | Read Pool (3 conns) | Write Pool (1 conn) |
|---------|-------------------|-------------------|
| journal_mode | WAL | WAL |
| synchronous | NORMAL | NORMAL |
| cache_size | 8 MB | 8 MB |
| busy_timeout | 5 seconds | 5 seconds |
| journal_size_limit | 64 MB | 64 MB |
| foreign_keys | ON | ON |
| query_only | ON | — |
| wal_autocheckpoint | default (1000) | **0 (disabled)** |

Write connections disable auto-checkpoint so heavy batch operations (import,
thumbnail rebuild) don't trigger checkpoints mid-write. Explicit
`PRAGMA wal_checkpoint(PASSIVE)` runs after each bulk operation completes.

### DELETE Mode (exFAT, FAT32, NFS)

| Setting | Read Pool (1 conn) | Write Pool (1 conn) |
|---------|-------------------|-------------------|
| journal_mode | DELETE (nolock VFS for NFS) | DELETE (nolock VFS for NFS) |
| synchronous | NORMAL | NORMAL |
| cache_size | 8 MB | 8 MB |
| busy_timeout | 5 seconds | 5 seconds |
| foreign_keys | ON | ON |
| query_only | ON | — |

DELETE mode is used for filesystems that lack reliable POSIX locking or shared
memory support. Only 1 read connection (no concurrent readers with DELETE
journal). No WAL-specific settings (journal_size_limit, wal_autocheckpoint).

### Connection Lifecycle

- **Creation**: `SqliteManager::create()` opens a connection via
  `db_common::open_connection()`, then applies per-role PRAGMAs.
- **Reuse**: Deadpool returns connections to the pool after each `read()`/`write()`.
- **Recycle**: `SqliteManager::recycle()` checks for mutex poisoning and
  runs `PRAGMA optimize` at most once per hour.
- **Warmup**: `DbPool::new()` eagerly creates 1 read + 1 write connection
  at startup. Fails fast if the DB is inaccessible.

---

## Concurrency Model

```
                    ┌──────────────┐
   HTTP requests ──▶│  Read Pool   │──▶ concurrent reads (WAL only)
                    │  (3 or 1)    │
                    └──────────────┘

   Server fns    ──▶│  Write Pool  │──▶ serialized writes
   (mutations)      │  (1 conn)    │
                    └──────────────┘

   Background    ──▶│  Write Pool  │──▶ batch operations
   pipeline         │  (shared)    │    (import, thumbnail rebuild)
                    └──────────────┘
```

The write pool has 1 connection — SQLite serializes writes regardless of
how many connections exist. On WAL-mode filesystems (ext4, etc.), the read
pool has 3 connections for concurrent page loads. On DELETE-mode filesystems
(exFAT, NFS), the read pool has 1 connection (no concurrent readers).

The `scanning` AtomicBool prevents concurrent import + thumbnail operations.
User-triggered operations check `is_scanning()` before starting. ROM lookups
(`get_roms`) check the `scanning` flag (not the legacy `busy` flag) so they
work correctly during thumbnail updates and imports.

---

## User-Triggered Operations

These run in the background via `tokio::spawn`, with SSE progress streaming.

| Operation | Trigger | Progress | Cancellable |
|-----------|---------|----------|-------------|
| LaunchBox import | Metadata page button | SSE: phase + entry count | No |
| LaunchBox download + import | Metadata page button | SSE: download → import | No |
| Thumbnail update | Metadata page button | SSE: index → download per system | Yes |
| Metadata regenerate | Metadata page button | SSE: clear → re-import | No |
| Game library rebuild | Metadata page button | Synchronous (fast) | No |

All mutually exclusive via the shared `scanning` flag. The UI checks
`is_scanning()` before starting and shows "Another operation is
already running" if busy.

---

## Error Handling

| Scenario | Behavior |
|----------|----------|
| DB inaccessible at startup | Fail fast — process exits, systemd restarts |
| DB error during request | Server function returns error to client; logged server-side via `tracing::warn!` with full details |
| USB unplugged | `refresh_storage()` detects change, closes pools, reopens at new location |
| NFS unreachable | Queries timeout after busy_timeout (5s); NFS TTL (30 min) prevents constant retries |
| Import fails mid-batch | Error logged, progress shows "Failed", scanning flag cleared |

User-facing error messages are clean ("Could not load metadata stats. Please try again.").
Server logs contain full diagnostic details including SQLite extended error codes.

---

## Key Source Files

| File | Role |
|------|------|
| `main.rs` | Process entry point, server setup |
| `api/mod.rs` | AppState, DbPool, SqliteManager |
| `api/background.rs` | BackgroundManager, startup pipeline, watchers |
| `api/cache/mod.rs` | GameLibrary, CacheEntry, Freshness |
| `api/cache/enrichment.rs` | Post-scan metadata enrichment |
| `api/cache/images.rs` | Thumbnail/box art index |
| `api/cache/favorites.rs` | Favorites cache |
| `api/import.rs` | ImportPipeline, ThumbnailPipeline |
| `core/metadata/db_common.rs` | Connection open, PRAGMA config, filesystem detection, journal mode selection |
