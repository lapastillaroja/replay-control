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
    RefreshExternalMetadata { progress: RefreshMetadataProgress },
    ThumbnailUpdate { progress: ThumbnailProgress, cancel: Arc<AtomicBool> },
    Rebuild { progress: RebuildProgress },
    Identity { progress: IdentityProgress },
    Maintenance { kind: MaintenanceKind },
    Update { progress: UpdateProgress },
}
```

### Variants

- **Idle**: No operation running. All UI buttons enabled.
- **Startup**: First-run source fetch, cache verification, and thumbnail index rebuild. LaunchBox XML parsing uses `RefreshExternalMetadata` instead. Contains `StartupPhase::FetchingMetadata`, `StartupPhase::Scanning`, or `StartupPhase::RebuildingIndex` and the current system name.
- **Import**: LaunchBox metadata parse (local XML or download + parse). Carries `ImportProgress` with state machine (Downloading -> BuildingIndex -> Parsing -> Complete/Failed), processed/matched/inserted counts, and download bytes.
- **ThumbnailUpdate**: Index refresh + image download from libretro-thumbnails. Two phases: `Indexing` (GitHub API) then `Downloading` (raw.githubusercontent.com). Only variant with a `cancel` token (`Arc<AtomicBool>`) for cooperative cancellation.
- **Rebuild**: Game library rebuild or rescan (per-system strict reconcile + inline enrichment). Phases: `Scanning` -> `Enriching` -> `Complete`/`Failed`. The `RebuildProgress.is_rescan` flag distinguishes rescan ("Rescanning ...") from rebuild ("Rebuilding ...") in the banner copy.
- **Identity**: Background ROM identity matching after startup/rescan/rebuild. Shows row-based progress as "Matching ROMs" and owns the activity slot so a second rebuild/rescan cannot cancel long hash reads. Storage changes still cancel it through the storage-generation guard.
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

Only `ThumbnailUpdate` supports user-initiated cooperative cancellation via `AppState::request_cancel()`, which sets the `cancel` `AtomicBool`. The download loop checks this flag between systems and stops early if set, transitioning to `ThumbnailPhase::Cancelled`.

Identity matching is not user-cancellable from the UI. It is cancelled only by storage-generation changes, such as a storage swap or configured storage becoming unavailable. Ordinary rebuild/rescan requests are blocked while identity is active.

## UI Integration

The client-side JavaScript listens on the SSE stream and:
- Shows a progress bar banner for Import, ThumbnailUpdate, and Rebuild
- Shows a "Matching ROMs" progress banner for Identity
- Disables action buttons when not Idle
- Displays the current system name during Startup
- Shows terminal messages (success/failure) briefly before returning to Idle
