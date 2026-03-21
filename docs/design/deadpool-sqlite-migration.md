# deadpool-sqlite Database Refactoring

## Status: Phases 1-3 Complete (Incremental)

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

## Deviations from Plan

1. **No async pool yet**: `DbPool` currently wraps `Arc<Mutex<Option<Connection>>>`
   synchronously. The `deadpool-sqlite` dependency is added but the actual async pool
   with `interact()` is not wired up yet. This is intentional -- the stateless API
   migration was the critical prerequisite, and switching the pool internals to
   deadpool is now a small, isolated change.

2. **Compatibility shims remain**: `metadata_db()` and `user_data_db()` methods on
   AppState return `MutexGuard<Option<Connection>>` for backward compatibility. The
   import pipeline's per-batch locking pattern (`state.metadata_db.lock()`) still
   uses direct Mutex access. These can be cleaned up when the deadpool async pool
   is wired up.

3. **Single pool (not read/write split)**: The plan called for separate read and write
   pools. Currently there's one `DbPool` per database. Read/write separation can be
   added when switching to deadpool internals (different pool sizes for readers vs writer).

## Next Steps (Phase 4+)

1. **Wire up deadpool-sqlite async pool**: Replace `Arc<Mutex<Option<Connection>>>`
   inside `DbPool` with `deadpool_sqlite::Pool`. Use a custom `Manager` that calls
   `db_common::open_connection()` for proper WAL/nolock setup.

2. **Make `read()`/`write()` async**: Change signatures to return futures. All server
   function callers are already async, so `.await` can be added naturally.

3. **Remove compatibility shims**: Delete `metadata_db()`, `user_data_db()` methods
   and the `metadata_db` field from AppState.

4. **Read/write pool split**: Create separate read pool (max_size=3 for local, 1 for NFS)
   and write pool (max_size=1). Route `read()` to read pool, `write()` to write pool.

5. **Deploy and test on Pi**: `./dev.sh --pi`
