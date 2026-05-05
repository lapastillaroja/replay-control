# Connection Pooling

`DbPool`, the private `WriteGate`, and the custom `SqliteManager` live in `replay-control-core-server/src/db_pool.rs` as a generic, app-agnostic utility. `replay-control-app/src/api/mod.rs` constructs the app-specific pool instances and stores them on `AppState`.

## Pool Architecture

Each mutable SQLite database gets a `DbPool` instance backed by `deadpool` with a custom `SqliteManager`. The app has three `DbPool` instances plus the separate read-only catalog pool:

- `library_pool` — for `library.db` (game library, thumbnails, imported metadata). Stored centrally on the host SD at `/var/lib/replay-control/storages/<storage-id>/library.db`. Always WAL on ext4.
- `external_metadata_pool` — for host-global `external_metadata.db` (LaunchBox rows, libretro thumbnail manifests, source stamps). Stored under `/var/lib/replay-control/external_metadata.db` on the host SD.
- `user_data_pool` — for `user_data.db` (box art overrides, saved videos). Stays per-storage at `<storage>/.replay-control/user_data.db`. Mode follows the storage filesystem (WAL on ext4, DELETE on exFAT/NFS).

Each `DbPool` contains two internal deadpool pools, sized per-pool at construction:

- **Library read pool**: `LIBRARY_READ_POOL_SIZE = 3`. WAL on ext4 lets concurrent readers actually parallelise; 3 covers SSR fan-out (recommendations + recents + favorites + system info) overlapping with one long enrichment / thumbnail-planning pass.
- **External metadata read pool**: `EXTERNAL_METADATA_READ_POOL_SIZE = 2`. Keeps short UI/server-function reads moving while one longer background enrichment or thumbnail-manifest read is active.
- **User data read pool**: `USER_DATA_READ_POOL_SIZE = 1`. exFAT/NFS DELETE-mode pools serialise readers vs. writers via the gate; extra readers don't help.
- **Write pool**: 1 connection (SQLite serialises writes).

## Custom SqliteManager

Instead of deadpool-sqlite's default `Connection::open()`, the custom manager uses `sqlite::open_connection()` which handles WAL/nolock/PRAGMA configuration based on filesystem capabilities.

### Connection creation

Per-role PRAGMAs applied on top of base PRAGMAs from `open_connection()`:

```
PRAGMA cache_size = 1000;          -- Read connections: ~4 MB
PRAGMA cache_size = 500;           -- Write connection: ~2 MB
PRAGMA query_only = ON;            -- Read only: defense-in-depth, prevents accidental writes
```

### Connection recycling

Skips the default SELECT health check (3.5x faster, per Matrix SDK findings). If the connection is broken, the next `interact()` call fails and the pool discards it.

Runs `PRAGMA optimize` (with `analysis_limit = 400`) on write connections at most once per hour to keep query planner statistics fresh without per-return overhead. Read connections are `query_only` and do not run optimize.

## Journal Mode Detection

At pool creation, a warmup connection queries `PRAGMA journal_mode` to determine the actual mode. The mode is stored on the pool as an `AtomicU8` and is fixed for the lifetime of the pool — `reopen()` re-detects after a path change. The mode swap precedes the pool-slot swap on `reopen` so a concurrent `try_write` arriving mid-reopen never observes (old mode, new pool).

## WAL Recovery

`sqlite::recover_after_unclean_shutdown(path)` runs **once per `DbPool` instance**, before any deadpool connection exists, inside `DbPool::new` and `DbPool::reopen`. Per-connection opens never touch sidecar files — historical bug: running recovery from inside a per-connection opener unlinks `-wal`/`-shm` while sibling connections hold them, returning empty reads to live callers (see `investigations/2026-05-01-library-wal-unlink-under-live-connections.md`).

## API: Result, not Option

```rust
pub async fn try_read<F, R>(&self, f: F) -> Result<R, DbError>;
pub async fn try_write<F, R>(&self, f: F) -> Result<R, DbError>;
```

`DbError::{Closed, Corrupt, Busy, Timeout, Sql, Acquire, Interact, Other}` distinguishes "pool can't answer" from "query ran and returned nothing." Cascade gates (e.g. *is the library empty?* before a destructive populate) **must** use `try_read` and treat `Err(_)` as "skip — pool unavailable", never as "no rows" — silently defaulting `None`/`Err` to a destructive default is what produced the visible "library shows 0 games" regression.

`read()` / `write()` are kept as `try_*().ok()` adapters for sites where best-effort is genuinely correct (cache-clearing afterthoughts, log-only metrics queries).

## Write Gate (DELETE-mode only)

`WriteGate` is private (`pub(crate)`). The pool itself decides whether to activate it based on `journal_mode`:

- **WAL pool** (`library_pool` on ext4): the gate is never set. SQLite's MVCC means writers don't conflict with readers.
- **DELETE pool** (`user_data_pool` on exFAT/NFS): the gate auto-activates inside `try_write` for the duration of the closure. Concurrent `try_read` calls return `Err(DbError::Busy)`. Releases on drop (panic-safe).

Gate scope is **a single `try_write` call**. Long write sequences should call `try_write` per logical write rather than holding an outer gate, so SSR readers stay responsive between calls.

## Corruption Detection

After every `interact()` closure runs, `check_for_corruption` reads `sqlite3_errcode()`. `SQLITE_CORRUPT` (11) or `SQLITE_NOTADB` (26) flips the pool's `corrupt` flag and fires the corruption callback. Subsequent `try_*` calls short-circuit with `Err(DbError::Corrupt)` *before* acquiring a connection — pool slots stay populated until the host explicitly recovers.

`mark_corrupt` is sync (it's reached from the corruption probe inside `interact()`, a sync context). It does **not** drain the pool; `reset_to_empty` / `replace_with_file` do that work explicitly.

`DbPool` exposes a `set_corruption_callback()` hook that fires on the actual transitions of the corrupt flag — both false→true (`mark_corrupt`) and true→false (`reopen`). Idempotent calls do not re-fire. The host crate registers a callback that broadcasts `ConfigEvent::CorruptionChanged` over `/sse/config`, so the UI banner reflects pool state without polling.

## Pool Lifecycle

- **Startup**: pre-flight WAL recovery → warmup connection (detects journal mode) → pool slots populated. Failure to warm means the DB is inaccessible — server exits.
- **`close()`**: async. Calls deadpool's `pool.close()`, polls `status().size > 0` until in-flight `Object`s drain or `INTERACT_TIMEOUT * 2` elapses. Returns `bool` — destructive callers (`reset_to_empty`, `replace_with_file`) abort if drain timed out, so a stuck closure can't race a follow-up `delete_db_files`.
- **`reopen(db_path)`**: drains current connections, runs WAL recovery (skipped if same path and already recovered), rebuilds both pools. Atomic in the order `journal_mode` swap → pool-slot swap so a concurrent `try_write` mid-reopen never sees stale mode + new pool.
- **`reset_to_empty()`**: drain → `delete_db_files` → reopen empty. The supported "clear and rebuild" entry point. Direct `pool.close(); delete_db_files; pool.reopen()` is racy because old `Object`s can still hold inodes when the unlink runs.
- **`replace_with_file(src)`**: drain → unlink sidecars → copy `src` over → reopen. Used by user-data restore-from-backup.
- **Closed state**: `DbPool::new_closed()` creates a pool where all `try_*` return `Err(DbError::Closed)`. Used at startup when storage is unavailable.

## WAL Checkpointing

WAL pools use SQLite's default automatic checkpointing. The app does not disable `wal_autocheckpoint` and normal heavy-write paths do not force broad post-scan checkpoints through the generic `DbPool::write` timeout window.
