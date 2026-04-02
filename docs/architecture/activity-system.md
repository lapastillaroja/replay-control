# Activity System

Defined in `replay-control-app/src/api/activity.rs`. Provides mutual exclusion for long-running operations and real-time progress broadcasting to the UI.

## Design

At most one activity runs at a time. The state is stored in `AppState::activity` (`Arc<RwLock<Activity>>`) and broadcast to SSE clients via `AppState::activity_tx` (a `tokio::sync::broadcast` channel).

## Activity Enum

```rust
enum Activity {
    Idle,
    Startup { phase: StartupPhase, system: String },
    Import { progress: ImportProgress },
    ThumbnailUpdate { progress: ThumbnailProgress, cancel: Arc<AtomicBool> },
    Rebuild { progress: RebuildProgress },
    Maintenance { kind: MaintenanceKind },
}
```

### Variants

- **Idle**: No operation running. All UI buttons enabled.
- **Startup**: Background pipeline phases 2+3 (cache verification + thumbnail index rebuild). Phase 1 auto-import uses `Import` instead. Contains `StartupPhase::Scanning` or `StartupPhase::RebuildingIndex` and the current system name.
- **Import**: LaunchBox metadata parse (local XML or download + parse). Carries `ImportProgress` with state machine (Downloading -> BuildingIndex -> Parsing -> Complete/Failed), processed/matched/inserted counts, and download bytes.
- **ThumbnailUpdate**: Index refresh + image download from libretro-thumbnails. Two phases: `Indexing` (GitHub API) then `Downloading` (raw.githubusercontent.com). Only variant with a `cancel` token (`Arc<AtomicBool>`) for cooperative cancellation.
- **Rebuild**: Game library rebuild (invalidate + rescan + enrich). Phases: `Scanning` -> `Enriching` -> `Complete`/`Failed`. Tracks per-system progress.
- **Maintenance**: Short DB/filesystem operations (clear metadata, clear images, cleanup orphans). No detailed progress -- just a `MaintenanceKind` discriminant.

## ActivityGuard (RAII Pattern)

```rust
pub struct ActivityGuard {
    state: Arc<RwLock<Activity>>,
    activity_tx: broadcast::Sender<Activity>,
}
```

The guard is obtained via `AppState::try_start_activity(initial)`:

1. Acquires the activity write lock
2. If not `Idle`, returns `Err("Another operation is already running")`
3. Sets the initial activity and broadcasts it
4. Returns the `ActivityGuard`

The guard provides `update()` to modify the activity in-place and broadcast changes. On `Drop`, it resets to `Idle` and broadcasts -- this is panic-safe, so even if an operation panics, the system returns to Idle.

## Mutual Exclusion

`try_start_activity` is the single entry point for claiming the activity slot. Only one caller succeeds; all others get an error message. This prevents conflicting operations (e.g., import during rebuild).

## SSE Broadcast

The `activity_tx` broadcast channel pushes every state change to all connected SSE clients. The SSE endpoint (`/sse/activity` in `main.rs`):

1. Sends an initial snapshot of the current activity state on connect
2. Waits on `activity_tx.subscribe()` for updates (no polling loop)
3. On `Lagged` (missed events), re-sends current state to catch up
4. Keep-alive every 15 seconds

The Activity enum is serialized as tagged JSON (`#[serde(tag = "type")]`), so clients can switch on the `type` field to render appropriate progress UI.

## Terminal States

Activities like Import, ThumbnailUpdate, and Rebuild have terminal states (Complete, Failed, Cancelled). The `is_terminal()` method checks for these, and `terminal_message()` produces a human-readable summary. Terminal states are broadcast so the UI can show completion messages before the guard drops and resets to Idle.

## Cancellation

Only `ThumbnailUpdate` supports cooperative cancellation via `AppState::request_cancel()`, which sets the `cancel` `AtomicBool`. The download loop checks this flag between systems and stops early if set, transitioning to `ThumbnailPhase::Cancelled`.

## UI Integration

The client-side JavaScript listens on the SSE stream and:
- Shows a progress bar banner for Import, ThumbnailUpdate, and Rebuild
- Disables action buttons when not Idle
- Displays the current system name during Startup
- Shows terminal messages (success/failure) briefly before returning to Idle
