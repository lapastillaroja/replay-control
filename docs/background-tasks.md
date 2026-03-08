# Background Task System Design

## Problem

Several operations in the Replay companion app are too slow for a request/response cycle: scanning the full ROM library, building a metadata database, bulk ROM operations. These need to run in the background with progress reporting, and the user should be able to see what's running and cancel if needed.

## Use Cases

**Phase 1 (immediate):**
- **ROM library scan** -- full rescan of all systems, triggered manually or after USB mount changes. Currently done synchronously per-request with a 30-second cache; a background scan would pre-warm the cache on startup or on demand.
- **Metadata DB build** -- parse MAME/FBNeo XML files, build the SQLite lookup table (see `arcade-db-design.md`). This is a one-time operation per DB version, but takes seconds on a Pi.

**Phase 2 (future):**
- **Bulk ROM operations** -- delete duplicates, reorganize files, batch rename.
- **ROM scraping** -- download box art or metadata from external sources.
- **Backup/restore** -- copy saves or ROM folders to/from USB.

## Architecture

### Core Idea

A `TaskManager` holds a `DashMap<TaskId, TaskHandle>` and lives inside `AppState` as an `Arc<TaskManager>`. Tasks are spawned with `tokio::spawn` and communicate progress through `Arc<AtomicU32>` (percentage) and status through `Arc<RwLock<TaskStatus>>`. Cancellation uses `tokio_util::CancellationToken`.

No external job queue or database. Everything is in-memory. Task history is lost on restart, which is fine -- these are transient operations.

### Component Diagram

```
AppState
  |
  +-- Arc<TaskManager>
        |
        +-- tasks: DashMap<TaskId, TaskHandle>
        |
        +-- spawn_task(name, closure) -> TaskId
        +-- cancel_task(id) -> Result
        +-- list_tasks() -> Vec<TaskInfo>
        +-- get_task(id) -> Option<TaskInfo>
```

### TaskManager

```rust
use dashmap::DashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use tokio_util::sync::CancellationToken;

pub type TaskId = u64;

pub struct TaskManager {
    tasks: DashMap<TaskId, TaskHandle>,
    next_id: AtomicU64,
}

struct TaskHandle {
    info: Arc<RwLock<TaskMeta>>,
    progress: Arc<AtomicU32>,        // 0-100
    cancel_token: CancellationToken,
    join_handle: tokio::task::JoinHandle<()>,
}

struct TaskMeta {
    id: TaskId,
    name: String,
    status: TaskStatus,
    error: Option<String>,
    created_at: Instant,
    finished_at: Option<Instant>,
}

#[derive(Clone, Serialize, Deserialize)]
pub enum TaskStatus {
    Running,
    Completed,
    Failed,
    Cancelled,
}
```

### Spawning a Task

The caller provides a closure that receives a `TaskContext` -- a handle for reporting progress and checking cancellation:

```rust
pub struct TaskContext {
    progress: Arc<AtomicU32>,
    meta: Arc<RwLock<TaskMeta>>,
    cancel_token: CancellationToken,
}

impl TaskContext {
    pub fn set_progress(&self, pct: u32) {
        self.progress.store(pct.min(100), Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel_token.is_cancelled()
    }

    /// For use in async contexts: returns when cancellation is requested.
    pub fn cancelled(&self) -> WaitForCancellationFuture<'_> {
        self.cancel_token.cancelled()
    }
}

impl TaskManager {
    pub fn spawn<F, Fut>(&self, name: impl Into<String>, f: F) -> TaskId
    where
        F: FnOnce(TaskContext) -> Fut + Send + 'static,
        Fut: Future<Output = Result<(), String>> + Send,
    {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let cancel_token = CancellationToken::new();
        let progress = Arc::new(AtomicU32::new(0));
        let meta = Arc::new(RwLock::new(TaskMeta {
            id,
            name: name.into(),
            status: TaskStatus::Running,
            error: None,
            created_at: Instant::now(),
            finished_at: None,
        }));

        let ctx = TaskContext {
            progress: progress.clone(),
            meta: meta.clone(),
            cancel_token: cancel_token.clone(),
        };

        let join_handle = tokio::spawn(async move {
            let result = f(ctx).await;
            let mut m = meta.write().unwrap();
            m.finished_at = Some(Instant::now());
            match result {
                Ok(()) => m.status = TaskStatus::Completed,
                Err(e) => {
                    m.status = TaskStatus::Failed;
                    m.error = Some(e);
                }
            }
        });

        self.tasks.insert(id, TaskHandle {
            info: meta,
            progress,
            cancel_token,
            join_handle,
        });

        id
    }
}
```

### Cancellation

```rust
impl TaskManager {
    pub fn cancel(&self, id: TaskId) -> bool {
        if let Some(handle) = self.tasks.get(&id) {
            handle.cancel_token.cancel();
            true
        } else {
            false
        }
    }
}
```

The task closure is responsible for checking `ctx.is_cancelled()` at reasonable intervals (e.g., between processing each system directory, between each ROM file). This is cooperative cancellation -- the token signals intent, the task must respect it.

For async work, use `tokio::select!`:

```rust
tokio::select! {
    _ = ctx.cancelled() => {
        return Err("cancelled".into());
    }
    result = do_work() => {
        result?;
    }
}
```

### Cleanup

Completed/failed/cancelled tasks stay in the map for a while so the UI can show results. A simple approach: when `list_tasks()` is called, prune tasks that finished more than 5 minutes ago. No background reaper needed.

```rust
impl TaskManager {
    pub fn list_tasks(&self) -> Vec<TaskInfo> {
        let cutoff = Instant::now() - Duration::from_secs(300);
        // Remove stale finished tasks.
        self.tasks.retain(|_, h| {
            let meta = h.info.read().unwrap();
            meta.finished_at.map_or(true, |t| t > cutoff)
        });
        // Return remaining.
        self.tasks.iter().map(|entry| {
            let meta = entry.info.read().unwrap();
            TaskInfo {
                id: meta.id,
                name: meta.name.clone(),
                status: meta.status.clone(),
                progress: entry.progress.load(Ordering::Relaxed),
                error: meta.error.clone(),
            }
        }).collect()
    }
}
```

## Data Model

### TaskInfo (serialized to frontend)

```rust
#[derive(Clone, Serialize, Deserialize)]
pub struct TaskInfo {
    pub id: u64,
    pub name: String,
    pub status: TaskStatus,   // "running" | "completed" | "failed" | "cancelled"
    pub progress: u32,        // 0-100
    pub error: Option<String>,
}
```

This is intentionally minimal. No created_at timestamp is sent to the client (Instant is not serializable and we don't need it in the UI). The server uses Instant internally for cleanup.

## API

Server functions, consistent with the rest of the app. All prefixed with `/sfn`.

### list_tasks

```rust
#[server(prefix = "/sfn")]
pub async fn list_tasks() -> Result<Vec<TaskInfo>, ServerFnError> {
    let state = expect_context::<AppState>();
    Ok(state.task_manager.list_tasks())
}
```

### start_library_scan

```rust
#[server(prefix = "/sfn")]
pub async fn start_library_scan() -> Result<u64, ServerFnError> {
    let state = expect_context::<AppState>();
    let storage = state.storage.clone();
    let cache = state.cache.clone();

    let id = state.task_manager.spawn("Library scan", move |ctx| async move {
        cache.invalidate();
        // scan_systems is synchronous and blocking -- run on a blocking thread.
        let systems = tokio::task::spawn_blocking({
            let storage = storage.clone();
            move || replay_core::roms::scan_systems(&storage)
        }).await.map_err(|e| e.to_string())?;

        ctx.set_progress(50);
        if ctx.is_cancelled() {
            return Err("cancelled".into());
        }

        // Pre-warm per-system caches.
        let total = systems.len();
        for (i, sys) in systems.iter().enumerate() {
            if ctx.is_cancelled() {
                return Err("cancelled".into());
            }
            let storage = storage.clone();
            let folder = sys.folder_name.clone();
            let _ = tokio::task::spawn_blocking(move || {
                replay_core::roms::list_roms(&storage, &folder)
            }).await;
            ctx.set_progress(50 + ((i + 1) * 50 / total) as u32);
        }
        Ok(())
    });
    Ok(id)
}
```

### cancel_task

```rust
#[server(prefix = "/sfn")]
pub async fn cancel_task(id: u64) -> Result<bool, ServerFnError> {
    let state = expect_context::<AppState>();
    Ok(state.task_manager.cancel(id))
}
```

### Duplicate prevention

`TaskManager::spawn` should check if a task with the same name is already running and return the existing id instead of spawning a duplicate. This prevents the user from accidentally kicking off two library scans.

```rust
pub fn spawn_unique<F, Fut>(&self, name: impl Into<String>, f: F) -> TaskId
where ...
{
    let name = name.into();
    // Check for existing running task with the same name.
    for entry in self.tasks.iter() {
        let meta = entry.info.read().unwrap();
        if meta.name == name && matches!(meta.status, TaskStatus::Running) {
            return meta.id;
        }
    }
    self.spawn_inner(name, f)
}
```

## UI Integration

### Approach: polling with setInterval

No SSE or WebSockets. The frontend polls `list_tasks` every 2 seconds while tasks are active, then stops polling. This keeps the implementation simple and avoids holding connections open on a resource-constrained Pi.

### Notification indicator in the top bar

Add an activity indicator next to the existing favorites star in the top bar. It shows a count badge when tasks are running.

```rust
#[component]
fn TaskIndicator() -> impl IntoView {
    let poll_trigger = RwSignal::new(0u32);
    let tasks = Resource::new(
        move || poll_trigger.get(),
        |_| list_tasks(),
    );

    // Poll every 2s while there are running tasks.
    #[cfg(feature = "hydrate")]
    {
        use leptos::prelude::*;
        Effect::new(move |_| {
            // set_interval to bump poll_trigger
        });
    }

    let running_count = move || {
        tasks.get()
            .and_then(|r| r.ok())
            .map(|t| t.iter().filter(|t| matches!(t.status, TaskStatus::Running)).count())
            .unwrap_or(0)
    };

    view! {
        <Show when=move || running_count() > 0>
            <A href="/tasks" attr:class="icon-btn task-indicator">
                <span class="task-badge">{move || running_count().to_string()}</span>
            </A>
        </Show>
    }
}
```

### Tasks page

A new route at `/tasks` showing all active and recent tasks. Each row shows the task name, a progress bar, status, and a cancel button for running tasks.

```
/tasks
  +-- TaskRow("Library scan", progress=73, Running)   [Cancel]
  +-- TaskRow("Metadata DB build", Completed)
```

No need for a dedicated page initially. This could also live as a section within the existing "More" page. Start there, promote to its own page if the list grows.

### After-completion behavior

When a task like "Library scan" completes, the cache is already warm. The next navigation to the games page picks up the new data automatically through the existing cache. No explicit invalidation signal to the UI is needed -- the cache TTL handles it.

## New Dependencies

```toml
# replay-app Cargo.toml, under [dependencies] (ssr-only)
dashmap = { version = "6", optional = true }
tokio-util = { version = "0.7", features = ["rt"], optional = true }
```

`dashmap` gives a concurrent HashMap without holding a mutex across await points. `tokio-util` provides `CancellationToken`. Both are lightweight, well-maintained, and already widely used in the tokio ecosystem.

## Where Code Lives

```
replay-app/
  src/
    tasks/
      mod.rs          -- TaskManager, TaskContext, TaskInfo, TaskStatus
      library_scan.rs -- spawn_library_scan logic
    server_fns.rs     -- add list_tasks, start_library_scan, cancel_task
    pages/
      more.rs         -- add task section or link to /tasks
    components/
      nav.rs          -- add TaskIndicator to top bar
```

The task system lives in `replay-app` (not `replay-core`) because it depends on tokio and is server-only. The actual work functions (scanning, DB building) call into `replay-core`.

## Implementation Plan

### Phase 1: Task infrastructure + library scan

1. Add `dashmap` and `tokio-util` dependencies.
2. Create `tasks/mod.rs` with `TaskManager`, `TaskContext`, `TaskInfo`.
3. Add `Arc<TaskManager>` to `AppState`.
4. Implement `start_library_scan` server function.
5. Add `list_tasks` and `cancel_task` server functions, register all three.
6. Add a "Scan Library" button to the More page that calls `start_library_scan`.
7. Add a minimal task status section below the button showing progress.

### Phase 2: UI polish

1. Add `TaskIndicator` to the top bar.
2. Add `/tasks` route (or section in More page) with task list.
3. Add polling logic (setInterval on the client side).

### Phase 3: Metadata DB task

1. Once the arcade metadata DB work lands (see `arcade-db-design.md`), add a `start_metadata_build` task that parses XML and populates SQLite.
2. Same pattern: `spawn_unique`, progress reporting per system, cancellation between entries.

## Design Decisions

**Why not SSE/WebSocket for progress?** Adds complexity (connection management, reconnection logic) for marginal benefit. Polling every 2 seconds is fine for a local-network app with one user. Revisit if latency matters.

**Why DashMap instead of `RwLock<HashMap>`?** Avoids holding a read lock across the `.iter()` call when listing tasks. DashMap's sharded design means listing tasks doesn't block spawning or cancelling.

**Why not a trait for task types?** Over-engineering. A closure that takes `TaskContext` is flexible enough. If we end up with 10+ task types with shared behavior, reconsider.

**Why AtomicU32 for progress instead of a channel?** Progress is a single number that gets overwritten. An atomic is the simplest possible primitive -- no allocation, no channel overhead, lock-free reads from the polling endpoint.
