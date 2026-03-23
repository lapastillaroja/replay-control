# deadpool-sqlite Database Refactoring

## Status: Complete

## Summary

Refactored the SQLite database layer from owned-connection structs to a stateless
pattern with a `DbPool` abstraction, preparing for full deadpool-sqlite async pooling.

## What Changed

### Phase 1a (pre-existing): MetadataDb made stateless
- `MetadataDb` is now a unit struct with associated functions taking `conn: &Connection`
- `open()` returns `(Connection, PathBuf)` instead of a struct instance
- All 496 core tests pass

### Phase 1b: UserDataDb made stateless
- Same pattern as MetadataDb
- `UserDataDb` is a unit struct; all methods take `conn: &Connection` or `conn: &mut Connection`
- `open()` returns `(Connection, PathBuf)`

### Phase 2: DbPool + AppState restructured
- Added `deadpool-sqlite` 0.9.0 dependency (compatible with rusqlite 0.32)
- Created `DbPool` struct in `api/mod.rs` with:
  - `read()` - runs a closure with `&Connection`
  - `write()` - runs a closure with `&mut Connection`
  - `raw_lock()` - direct Mutex access for import pipeline
  - `close()` / `reopen()` - for storage changes
- `AppState` now has `metadata_pool: DbPool` and `user_data_pool: DbPool`
- Compatibility shims: `metadata_db()` and `user_data_db()` methods return
  `Option<MutexGuard<Option<Connection>>>` for callers not yet fully migrated
- `metadata_db: Arc<Mutex<Option<Connection>>>` field kept for import pipeline
  direct Mutex access

### Phase 3: All callers migrated
- `GameLibrary.with_db_read()` / `with_db_mut()` now pass `&Connection` / `&mut Connection`
- All server_fns migrated from `db.method(args)` to `MetadataDb::method(conn, args)`
- All user_data_db calls migrated to `UserDataDb::method(conn, args)`
- Background pipeline and import pipeline updated
- Both `ssr` and `hydrate` targets compile
- All 496 core tests + 100 app tests pass

## Original Deviations (All Resolved)

The following deviations from the incremental plan existed after Phase 3 and have since
been resolved in the final implementation:

1. **~~No async pool yet~~**: `DbPool` now uses `deadpool` with a custom `SqliteManager`
   backed by `SyncWrapper`. Connections are obtained via `block_in_place` + `block_on`.

2. **~~Compatibility shims remain~~**: Removed. All callers use `DbPool.read()` /
   `DbPool.write()` directly.

3. **~~Single pool~~**: Read/write pool split implemented with separate pool sizes.

## Completed Steps (Phase 4+)

All phases are now complete. The final implementation uses:

1. **deadpool-sqlite async pool** with a custom `SqliteManager` that calls
   `db_common::open_connection()` for proper WAL/nolock setup.

2. **Synchronous `block_in_place` + `block_on`** for getting connections from the pool,
   which works from both tokio multi-thread worker threads and `spawn_blocking` threads.

3. **Compatibility shims removed** -- `metadata_db()` and `user_data_db()` methods
   removed from AppState. Import pipeline uses `DbPool.write()`.

4. **Read/write pool split** implemented: separate read pool (3 connections for local,
   1 for NFS) and write pool (1 connection). `DbPool.read()` routes to the read pool,
   `DbPool.write()` routes to the write pool.

5. **Deployed and tested on Pi** via `./dev.sh --pi`.
