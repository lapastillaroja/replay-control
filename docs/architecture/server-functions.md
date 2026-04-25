# Server Functions and SSR

Defined across `replay-control-app/src/main.rs`, `src/pages/`, and `src/server_fns/`.

## Leptos 0.7 SSR with WASM Hydration

The app uses server-side rendering with client-side WASM hydration. Four build profiles handle this:

| Environment | SSR Server | WASM Client |
|-------------|-----------|-------------|
| Dev | `dev` (opt 1) | `wasm-dev` (opt "s") |
| Prod | `release` (opt 3) | `wasm-release` (opt "z") |

`wasm-dev` exists because unoptimized WASM can be 20-40 MB. `opt "s"` keeps dev WASM at a few MB.

## any_spawner::Executor::init_tokio()

Called at the top of `run()` in `main.rs`:

```rust
let _ = any_spawner::Executor::init_tokio();
```

This is **required** for the Leptos reactive system to run async tasks during SSR. Without it, `Resource` async tasks silently don't execute. Leptos's `generate_route_list` and `leptos_routes_with_handler` initialize the executor automatically, but since this app uses `render_app_to_stream_with_context` directly (custom Axum routing), the executor must be initialized explicitly.

## register_explicit

All ~70 server functions are registered explicitly in `main.rs`:

```rust
server_fn::axum::register_explicit::<replay_control_app::server_fns::GetInfo>();
server_fn::axum::register_explicit::<replay_control_app::server_fns::GetSystems>();
// ... ~68 more
```

This is necessary because the server functions are defined in the library crate (`replay-control-app`), not the binary crate (`main.rs`). Rust's `inventory` crate auto-registration relies on linker magic that gets stripped when the functions are in a library -- the linker sees no direct references from `main` to the inventory items and discards them.

## First-Run Setup

Two server functions support the setup checklist:

- **`get_setup_status(force)`** — returns which setup steps are pending (metadata downloaded, thumbnail index updated). When `force` is true, re-checks even if previously dismissed.
- **`dismiss_setup()`** — sets the `setup_dismissed` preference so the banner no longer appears on the home page.

## Resource Patterns

### Resource::new_blocking (critical path)

Used for data that must be available in the initial HTML (no loading state). Blocks SSR until the data is ready:

```rust
let info = Resource::new_blocking(|| (), |_| server_fns::get_info());
let systems = Resource::new_blocking(|| (), |_| server_fns::get_systems());
```

The home page uses `new_blocking` for system info and library data so the page has meaningful content on first paint.

### Resource::new (streaming)

Used for data that can load after the initial HTML ships. The page renders with a skeleton/loading state, then streams the data via a `<Suspense>` boundary:

```rust
let recents = Resource::new(|| (), |_| server_fns::get_recents());
let recommendations = Resource::new(|| (), |_| server_fns::get_recommendations(6));
```

### Skeleton Loaders

Non-blocking resources use `<Suspense>` with skeleton fallbacks. The pattern:

```rust
<ErrorBoundary fallback=|errors| view! { <ErrorDisplay errors /> }>
    <Suspense fallback=move || view! { <HeroCardSkeleton /> }>
        {move || Suspend::new(async move {
            let data = resource.await;
            // render with data
        })}
    </Suspense>
</ErrorBoundary>
```

`Suspend::new(async { resource.await })` eliminates `.get().map()` boilerplate. `ErrorBoundary` catches server function errors.

## Response Cache

Defined in `replay-control-app/src/api/response_cache.rs`.

A `ResponseCache` on `AppState` caches fully assembled server function responses with a 10-second TTL:

- `get_recommendations` response
- `get_favorites_recommendations` response

This means back-navigation and rapid reloads within 10 seconds skip all DB queries and box art resolution entirely.

The cache is invalidated:
- On any mutation (favorite add/remove, ROM delete, etc.)
- After enrichment completes (new box art URLs)
- After on-demand thumbnail downloads complete
- Via `invalidate_all()` when storage changes

## SSE Endpoints

Two SSE streams provide real-time updates without polling:

### /sse/activity
Broadcasts `Activity` enum as tagged JSON. Clients receive import progress, thumbnail download counts, rebuild status. See [Activity System](activity-system.md).

### /sse/config
Broadcasts config changes (skin changes, storage changes). Sends initial state snapshot on connect, then event-driven updates. Keep-alive every 30 seconds.

## Storage Guard Middleware

When storage is unavailable, a middleware redirects ALL requests to `/waiting` (a static HTML page with auto-refresh). Only these paths bypass the guard:
- `/waiting` itself
- `/static/*` (CSS, JS, WASM)
- `/api/version`
