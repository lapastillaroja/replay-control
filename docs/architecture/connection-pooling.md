# Connection Pooling

Defined in `replay-control-app/src/api/mod.rs`.

## Pool Architecture

Each SQLite database gets a `DbPool` instance backed by `deadpool` with a custom `SqliteManager`. The app has two pools:

- `library_pool` -- for `library.db` (game library, thumbnails, imported metadata)
- `user_data_pool` -- for `user_data.db` (box art overrides, saved videos)

Each `DbPool` contains two internal deadpool pools:

- **Read pool**: `READ_POOL_SIZE = 1` connection
- **Write pool**: 1 connection (SQLite serializes writes)

Load tests on USB storage (DELETE journal mode) showed no performance improvement with more than 1 reader -- the single-user access pattern and fast queries (<50ms) don't benefit from concurrent readers. Keeping 1 saves ~2MB per unused connection.

## Custom SqliteManager

Instead of deadpool-sqlite's default `Connection::open()`, the custom manager uses `sqlite::open_connection()` which handles WAL/nolock/PRAGMA configuration based on filesystem capabilities.

### Connection creation

Per-role PRAGMAs applied on top of base PRAGMAs from `open_connection()`:

```
PRAGMA cache_size = 500;           -- All connections: reduce from 2000 pages (8MB) to 500 (2MB)
PRAGMA wal_autocheckpoint = 0;     -- Write + WAL only: manual checkpoint control
PRAGMA query_only = ON;            -- Read only: defense-in-depth, prevents accidental writes
```

### Connection recycling

Skips the default SELECT health check (3.5x faster, per Matrix SDK findings). If the connection is broken, the next `interact()` call fails and the pool discards it.

Runs `PRAGMA optimize` (with `analysis_limit = 400`) at most once per hour to keep query planner statistics fresh without per-return overhead.

## Journal Mode Detection

At pool creation, a warmup connection queries `PRAGMA journal_mode` to determine the actual mode:

- **WAL mode**: Used on filesystems that support it (ext4, btrfs). Enables concurrent readers, manual checkpointing after heavy writes.
- **DELETE mode**: Fallback for exFAT (USB drives). `open_connection()` detects this automatically.

The detected mode controls whether WAL-specific PRAGMAs are set.

## WriteGate: exFAT Corruption Prevention

`WriteGate` is an RAII guard that prevents concurrent reads during heavy write operations.

On exFAT filesystems (DELETE journal mode), concurrent reads during bulk writes can cause SQLite corruption. The write gate works by setting an `AtomicBool` flag on the pool:

```rust
pub(crate) struct WriteGate(Arc<AtomicBool>);
```

When activated, `DbPool::read()` checks the flag and returns `None` immediately, preventing any read connections from being acquired. The gate auto-clears on drop (panic-safe).

Used during:
- Full system populate (`populate_all_systems`)
- Rebuild game library

NOT used during enrichment writes (small per-system UPDATEs, not bulk INSERTs -- low corruption risk, and gating would block the reads that enrichment itself needs).

## Corruption Detection

Every `read()` and `write()` call checks `sqlite3_errcode()` after the user closure runs. If `SQLITE_CORRUPT` (error code 11) is detected, the pool sets a `corrupt` flag and closes all connections. Subsequent calls return `None` until the DB is rebuilt and the flag is cleared.

## Pool Lifecycle

- **Startup**: Pools open eagerly. One read + one write connection are warmed immediately. Failure to warm means the DB is inaccessible -- the server exits.
- **Storage change**: `close()` drops all connections. `reopen()` verifies the new DB path, rebuilds both pools, and clears the corrupt flag.
- **Closed state**: `DbPool::new_closed()` creates a pool where all reads/writes return `None`. Used at startup when storage is unavailable.

## Manual Checkpointing

`DbPool::checkpoint()` runs `PRAGMA wal_checkpoint(PASSIVE)` on the write connection. PASSIVE mode doesn't block readers. Called after heavy write operations (import, thumbnail rebuild, full populate) to fold the WAL back into the main database file and prevent unbounded WAL growth.
