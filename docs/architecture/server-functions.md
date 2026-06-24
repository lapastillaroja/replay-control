# Server Functions and SSR

Defined across `replay-control-app/src/main.rs`, `src/pages/`, and `src/server_fns/`.

## Leptos 0.7 SSR with WASM Hydration

The app uses server-side rendering with client-side WASM hydration. Six build profiles handle this:

| Environment | SSR Server | WASM Client |
|-------------|-----------|-------------|
| Fast dev (`dev.sh`) | `dev-fast` (opt 0) | `wasm-dev-fast` (opt 0) |
| Debug / compact dev | `dev` (opt 1) | `wasm-dev` (opt "s") |
| Prod | `release` (opt 3) | `wasm-release` (opt "z") |

`dev.sh` uses the fast profiles because rebuild latency matters more than artifact size during iteration. `wasm-dev` remains available when a smaller development WASM payload is worth the extra compile time.

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

## Now-Playing Hydration

Now-playing must be visible on the first page load when a game is already running. The server render reads `AppState.now_playing()` in `Shell` and emits a small bootstrap script before `HydrationScripts`. `App` initializes a `RwSignal<NowPlayingState>` from the same server value during SSR, and from the bootstrap value during browser hydration.

After hydration, `SseNowPlayingListener` subscribes to `/sse/now-playing`. The stream sends the current state first, then later changes. Consumers use `use_now_playing()`, which exposes a `Signal<NowPlayingState>` derived from the app-root signal.

Do not wrap the route body in an empty `Suspense` only to satisfy now-playing reads. It can change SSR marker placement and cause hydration mismatches. If a now-playing branch needs async detail data on first paint, use a blocking resource scoped to that branch and keep the conditional shape stable with `<Show>`.

## SSE Endpoints

Two SSE streams provide real-time updates without polling:

### /sse/activity
Broadcasts `Activity` enum as tagged JSON. Clients receive import progress, thumbnail download counts, rebuild status. See [Activity System](activity-system.md).

### /sse/config
Broadcasts skin changes, storage changes, available-update notifications, and database-corruption flag transitions. Sends an initial state snapshot on connect (which seeds the client's corruption banner among other things), then event-driven updates. Keep-alive every 30 seconds. The corruption events come from the pool-level callback in `DbPool::set_corruption_callback`; see [Connection Pooling](connection-pooling.md#corruption-detection).

## Authorization Guard

Device mode wraps app pages, REST handlers, SSE streams, media routes, and server functions in one authorization middleware. Standalone mode stays open by default because it runs off-device as a local ROM manager; stale device cookies are ignored there.

Sessions are stateless HMAC-signed cookies. The server stores only the signing key, rate-limit state, and app settings, not per-session rows. Rotating the signing key invalidates every session. Session claims include the current role, base role, expiration, optional admin-elevation expiration, and fingerprints tied to the RePlayOS Net Control code stored by Replay Control or device password state, so changing those stored credentials invalidates old cookies without an auth database. A TV-side Net Control code reset is detected separately when API probes/actions return unauthorized; that detection does not delete app sessions by itself.

Normal-user sessions last 30 days. Admin access defaults to 1 hour and can be configured to 1 hour, 3 hours, or 12 hours in `settings.cfg`. A direct device-password login expires back to anonymous, while a normal user who temporarily unlocks admin expires back to normal user access and can also downgrade from **Settings > Access & Security**. Changing the admin unlock duration refreshes the current admin cookie from the time of the change.

Server functions are explicitly classified by function name before they reach their per-function handler:

- **Public** functions support auth bootstrap and status checks.
- **User** functions support normal app usage such as browsing, favorites, launching, player controls, and read/write user actions.
- **Admin** functions change device state, credentials, network/storage identity, metadata, logs, updates, or certificates.

Unknown server functions fail closed as admin-only. A regression test scans `src/server_fns/*.rs` so adding a new server-function file requires updating the authorization inventory.

Signed-out sessions can only reach sign-in, setup, static assets, and health/version bootstrap endpoints. App browsing, media routes, REST endpoints, SSE streams, and non-bootstrap server functions require at least normal-user access. Authenticated unsafe requests also pass a CSRF check using Fetch Metadata and Origin/Referer validation.

## Storage Guard Middleware

When storage is unavailable, a middleware redirects ALL requests to `/waiting` (a static HTML page with auto-refresh). Only these paths bypass the guard:
- `/waiting` itself
- `/static/*` (CSS, JS, WASM)
- `/api/version`
- Auth bootstrap paths needed before storage is ready: `/login`, `/first-setup`, and anonymous server functions such as auth status, sign-in, first setup completion, and logout
