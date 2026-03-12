# Investigation: App Startup When NFS Storage Is Unavailable

> **Status**: Not yet addressed. The app still crashes with `exit(1)` when NFS storage is unavailable at startup.

Date: 2026-03-12

## Summary

When `storage_mode=nfs` is set in `replay.cfg` but the NFS mount at `/media/nfs` is not
available, the app **crashes at startup** with `std::process::exit(1)`. It does not start
the HTTP server, does not show an error page, and cannot self-recover. The user must wait
for NFS to become available and then restart the process (or a systemd unit must handle
restart).

---

## 1. What happens when `storage_mode=nfs` but the NFS mount is unavailable?

**The app exits immediately.**

The startup path in `main.rs` (lines 53-60) is:

```rust
let app_state = match api::AppState::new(cli.storage_path, cli.config_path) {
    Ok(state) => state,
    Err(e) => {
        tracing::error!("Failed to initialize: {e}");
        tracing::info!("Hint: use --storage-path to point to a RePlayOS storage location");
        std::process::exit(1);
    }
};
```

`AppState::new()` calls `StorageLocation::detect(&config)` (line 79 of `api/mod.rs`),
which for NFS mode does a simple `path.exists()` check on `/media/nfs`:

```rust
"nfs" => {
    let path = PathBuf::from("/media/nfs");
    if !path.exists() {
        return Err(Error::StorageNotFound);
    }
    (path, StorageKind::Nfs)
}
```

`StorageNotFound` propagates up through `AppState::new()` as a `Box<dyn Error>`, hits the
`Err(e)` branch in `main.rs`, and the process exits with code 1.

**Note:** The `--storage-path` CLI flag bypasses `detect()` entirely (line 51-69 of
`api/mod.rs`), so it is not affected. Local dev with `--storage-path` always works
regardless of NFS availability.

## 2. Does `StorageLocation::detect()` fail or return something useful?

**It fails with `Error::StorageNotFound`.** There is no fallback, no degraded mode, and no
retry. The function returns `Err(Error::StorageNotFound)` immediately if `/media/nfs` does
not exist.

The check is `!path.exists()`, which on Linux will:
- Return `false` if the directory does not exist at all.
- Return `false` (or hang/timeout) if the NFS mount point exists but the NFS server is
  unreachable and the mount is hard. With a soft mount, `stat()` may return `ETIMEDOUT`
  which also results in `exists()` returning `false`.
- Return `true` if the mount point exists and the NFS server is reachable.

## 3. Does the server start and show an error page, or does it crash?

**It crashes.** The server never reaches the `axum::serve()` call. The `Err` branch in
`main.rs` calls `std::process::exit(1)` before:
- The storage watcher is spawned
- The cache verification task is spawned
- The auto-import task is spawned
- The HTTP listener is bound
- Any routes are registered

There is no degraded/error-page mode. The app is all-or-nothing at startup.

## 4. When NFS becomes available later, does the config watcher pick it up?

**Not if the app crashed at startup.** Since the process exited, there is nothing running to
detect recovery.

**If the app was already running** (e.g., started while NFS was available, then NFS
dropped, then came back), the background watcher **would** pick it up -- but only at the
next 60-second poll tick. Here is the flow:

1. `spawn_storage_watcher()` starts a 60s poll loop (`background.rs`, lines 208-220).
2. Each tick calls `state.refresh_storage()`.
3. `refresh_storage()` re-reads `replay.cfg` from disk and calls
   `StorageLocation::detect(&config)`.
4. If `detect()` now succeeds (NFS is back), the storage is updated and the cache is
   invalidated.
5. If `detect()` fails, the error is logged as a warning:
   `tracing::warn!("Background storage re-detection failed: {e}")` -- but the
   **existing storage reference is not cleared**. The app continues operating with the old
   (now stale) `StorageLocation` pointing at `/media/nfs`.

The `notify`-based file watcher (inotify) watches `replay.cfg` for changes, not the NFS
mount point itself. It would not detect NFS recovery unless someone also edits `replay.cfg`
at the same time.

**Key nuance:** If NFS drops and comes back without any config change, `refresh_storage()`
will call `detect()` on the next poll. If NFS is back, `detect()` succeeds and returns the
same path, so `changed` evaluates to `false` (same root + same kind). The app silently
continues working -- filesystem operations that were failing will start succeeding again
naturally. No explicit recovery action is needed beyond the mount being available again.

## 5. What about SQLite operations when NFS drops mid-operation?

There are two scenarios depending on the SQLite open mode:

### Nolock mode (primary path for NFS)

The app tries `nolock` mode first (`metadata_db.rs`, lines 122-125). With `nolock=1`:
- Journal mode is `DELETE` (not WAL), since WAL requires shared memory which NFS can't do.
- There are no file locks, so no lock-related hangs.
- If NFS drops during a **read**, the `rusqlite` call will return an I/O error. The error
  propagates as `ServerFnError` to the client, which sees an error in the `ErrorBoundary`.
- If NFS drops during a **write** (e.g., metadata import), the write fails with I/O error.
  Because `nolock` disables rollback journal locking, the database file could be left in a
  corrupt state if a partial page write occurred. However, the `Mutex` in the app layer
  ensures single-writer access, so there is no concurrent-write corruption risk -- only
  incomplete-write corruption.

### WAL mode (fallback path)

If `nolock` open fails for some reason and the app falls back to WAL mode:
- WAL requires `mmap` for shared memory (`-shm` file), which does not work on NFS.
- The `PRAGMA journal_mode = WAL` call itself may hang for ~5 seconds (the SQLite busy
  timeout) before failing on NFS.
- If NFS drops during a WAL transaction, the `-wal` and `-shm` files may be left in an
  inconsistent state. Recovery would require the NFS server to come back and SQLite to
  replay the WAL on next open.

### Lazy DB opening

The metadata DB is opened lazily on first access (`AppState::metadata_db()`, lines 108-126
of `api/mod.rs`). If the DB can't be opened, `None` is returned and callers gracefully
degrade (no metadata enrichment, empty box art, etc.). The DB handle is cached, so if NFS
drops after a successful open, the cached `Connection` will start returning I/O errors on
subsequent queries. There is no automatic reconnect -- the `Option<MetadataDb>` stays
`Some(broken_connection)` until the cache is invalidated or the process restarts.

## 6. Are there timeout issues (the 5s SQLite WAL timeout on NFS)?

**With the current code, the 5s WAL timeout is avoided.** The app tries `nolock` first,
which skips WAL entirely. The WAL fallback only runs if `nolock` fails, and at that point
you're already on a problematic filesystem, so the 5s delay is the least of your problems.

However, there are other potential timeout/hang scenarios on NFS:

- **`path.exists()` in `detect()`**: On a hard NFS mount, `stat()` can hang indefinitely
  if the server is unreachable. This would block the thread calling `detect()`. In the
  startup path, this blocks the main thread. In the background watcher path, this blocks
  the poll tick (but the next tick would just queue behind it).

- **`std::fs::create_dir_all()` in `MetadataDb::open()`**: If the NFS mount exists but is
  stale, this call can hang waiting for the NFS server.

- **`std::fs::read_dir()` in `scan_systems()`**: Same hang risk on a stale NFS mount.

- **`df` command in `disk_usage_for()`**: Shells out to `df`, which can hang on a stale
  NFS mount. However, the `get_info()` server function uses `unwrap_or` on the result, so
  a failure here just returns zeroed disk stats (but the hang itself would block the
  request thread).

## Recommendations

1. **Start in degraded mode instead of crashing**: The app could start the HTTP server even
   when storage is unavailable, showing an "NFS storage unavailable, waiting..." page.
   `StorageLocation` could be wrapped in `Option` (or the `RwLock` could hold an `Option`)
   to allow a "no storage" state.

2. **Retry on startup**: Before giving up, try a few retries with backoff when
   `storage_mode=nfs` -- NFS mounts may not be ready at boot time due to network timing.

3. **Handle stale NFS mounts**: `path.exists()` on a hard NFS mount can hang forever. A
   timeout wrapper (e.g., `tokio::time::timeout` around a `spawn_blocking` stat call) would
   prevent infinite hangs.

4. **Reconnect stale DB connections**: When a cached `MetadataDb` connection starts
   returning I/O errors, drop it and set the `Option` back to `None` so the next access
   attempts to reopen.

5. **systemd restart policy**: As a short-term mitigation, ensure the systemd unit for the
   app has `Restart=on-failure` with `RestartSec=10s` and `After=network-online.target
   remote-fs.target` so it retries after NFS becomes available.

---

## Files Referenced

- `replay-control-core/src/storage.rs` -- `StorageLocation::detect()`, NFS path check
- `replay-control-core/src/config.rs` -- `ReplayConfig`, `storage_mode()` accessor
- `replay-control-core/src/error.rs` -- `Error::StorageNotFound`
- `replay-control-core/src/metadata_db.rs` -- `MetadataDb::open()`, nolock/WAL strategy
- `replay-control-core/src/roms.rs` -- `scan_systems()`, filesystem access
- `replay-control-app/src/main.rs` -- startup error handling, `exit(1)` on failure
- `replay-control-app/src/api/mod.rs` -- `AppState::new()`, `refresh_storage()`
- `replay-control-app/src/api/background.rs` -- 60s poll watcher, inotify watcher
- `replay-control-app/src/api/cache.rs` -- mtime-based cache, NFS flake tolerance
