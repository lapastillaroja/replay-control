# Integration Testing Analysis

Analysis of integration testing options for the replay-control-app project (Leptos 0.7 SSR + Axum + WASM hydration).

## Current State

- **265 unit tests** in `replay-control-core`, zero integration tests
- Core tests use manual temp directories with `StorageLocation::from_path()` (no `tempfile` crate)
- No `tests/` directories in either crate
- App crate has a dual-target build: WASM (`hydrate` feature) and native (`ssr` feature)
- Server functions use `expect_context::<AppState>()` for state access
- REST API routes use Axum `State(state): State<AppState>` extractors
- `build.rs` compiles CSS partials into `OUT_DIR` and `include_str!` embeds them in the binary

## 1. HTTP-Level Server Tests (Axum `tower::ServiceExt`)

**Approach**: Build the Axum `Router` with a test `AppState` pointing at a temp directory, then use `tower::ServiceExt::oneshot()` to send synthetic HTTP requests without binding a TCP port.

**Pros**:
- Tests the full HTTP path: routing, extractors, serialization, status codes
- No network dependency; runs in-process
- Well-documented pattern in the Axum ecosystem
- Can test REST API routes (`/api/*`) and server function endpoints (`/sfn/*`) identically
- Fast: no server startup, no port allocation

**Cons**:
- Requires building with `--features ssr` (won't compile under `hydrate`)
- The `build.rs` CSS compilation means tests need a valid `style/` directory (already present)
- SSR fallback handler (`leptos_axum::render_app_to_stream_with_context`) requires `any_spawner::Executor::init_tokio()` before use
- Server function endpoints at `/sfn/*` need explicit registration before testing

**Test fixture setup**:
```rust
#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt; // for oneshot()
    use http_body_util::BodyExt; // for collect()

    /// Build a test AppState with a temp directory containing a minimal ROM layout.
    fn test_app_state(tmp: &std::path::Path) -> crate::api::AppState {
        // Create minimal directory structure
        let roms = tmp.join("roms/nintendo_nes");
        std::fs::create_dir_all(&roms).unwrap();
        std::fs::write(roms.join("TestGame.nes"), b"fake rom data").unwrap();
        std::fs::create_dir_all(tmp.join("roms/_favorites")).unwrap();
        std::fs::create_dir_all(tmp.join("roms/_recent")).unwrap();

        crate::api::AppState::new(
            Some(tmp.to_string_lossy().into_owned()),
            None,
        ).unwrap()
    }

    /// Build the API-only router (no SSR fallback, no static files).
    fn test_router(state: crate::api::AppState) -> axum::Router {
        axum::Router::new()
            .nest("/api", {
                axum::Router::new()
                    .merge(crate::api::system_info::routes())
                    .merge(crate::api::roms::routes())
                    .merge(crate::api::favorites::routes())
                    .merge(crate::api::recents::routes())
            })
            .with_state(state)
    }

    #[tokio::test]
    async fn api_systems_returns_ok() {
        let tmp = tempdir();
        let state = test_app_state(&tmp);
        let app = test_router(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/systems")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let systems: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
        // At least one system should have games
        assert!(systems.iter().any(|s| s["game_count"].as_u64().unwrap() > 0));
    }

    #[tokio::test]
    async fn api_favorites_empty_initially() {
        let tmp = tempdir();
        let state = test_app_state(&tmp);
        let app = test_router(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/favorites")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let favs: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
        assert!(favs.is_empty());
    }
}
```

**Effort estimate**: 1-2 days for initial setup + first 10 tests.

---

## 2. Server Function Testing (Direct Invocation)

**Approach**: Call the generated server function structs directly via `server_fn::ServerFn::run_body()`, or call the inner logic by providing Leptos context manually.

**How `#[server]` functions access state**: Every server function does `expect_context::<AppState>()`. In production, the SSR handler and the `/sfn/*` handler both call `provide_context(state.clone())` before dispatching. In tests, we would need to set up a Leptos reactive runtime with the context provided.

**Option A: Test via HTTP (recommended)** -- Use the tower::ServiceExt approach from section 1, hitting `/sfn/<FnName>` with POST requests. This is the most realistic test.

**Option B: Test the inner logic directly** -- Extract the business logic from server functions into separate `pub(crate)` functions that take `AppState` as a parameter, then test those. This avoids the Leptos context system entirely.

**Option C: Provide Leptos context manually** -- Use `leptos::reactive::owner::Owner` to create a reactive scope with `provide_context`:
```rust
// Conceptual -- Leptos 0.7 reactive API
let owner = Owner::new();
owner.with(|| {
    provide_context(test_state);
    // Now server fn logic that calls expect_context will work
    let result = tokio::runtime::Runtime::new().unwrap().block_on(get_info());
    assert!(result.is_ok());
});
```
This approach is fragile and depends on Leptos internals. Not recommended.

**Recommendation**: Use Option A (HTTP-level tests via tower) for integration tests. If specific server function logic needs unit-level testing, use Option B (extract logic into testable functions).

**Pros** (Option A):
- Tests serialization/deserialization round-trip
- Tests that server function registration works
- No dependency on Leptos reactive internals

**Cons** (Option A):
- Server functions are POST with URL-encoded or CBOR bodies; need to construct these correctly
- Requires `server_fn::axum::register_explicit::<T>()` calls in test setup

**Effort estimate**: 1 day on top of the HTTP test infrastructure from section 1.

---

## 3. SSE Endpoint Testing

**Approach**: Use tower::ServiceExt to hit `/sse/image-progress` and `/sse/metadata-progress`, then read the streaming response.

**Key challenge**: SSE responses are infinite streams (they keep alive until the import finishes). The test needs to:
1. Set up `AppState` with an active import progress
2. Hit the SSE endpoint
3. Read a few events
4. Verify the JSON payloads
5. Drop the connection (or let the idle counter close it)

```rust
#[tokio::test]
async fn sse_image_progress_emits_null_when_idle() {
    let tmp = tempdir();
    let state = test_app_state(&tmp);
    // image_import_progress is None by default = no import running
    let app = axum::Router::new()
        .route("/sse/image-progress", /* the SSE handler */)
        .with_state(state);

    let resp = app
        .oneshot(Request::builder().uri("/sse/image-progress").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers().get("content-type").unwrap(),
        "text/event-stream"
    );

    // Read the body stream -- first event should be "data: null\n\n"
    let body = resp.into_body();
    // Use hyper::body or tokio to read the first chunk
    // The stream auto-closes after 5 idle ticks (1 second)
}
```

**Pros**:
- Tests the actual SSE wire format
- Validates that progress serialization works end-to-end

**Cons**:
- SSE handlers are currently defined inline in `main.rs` (anonymous closures), not extracted into testable functions
- Would need to either extract the SSE handler into a named function or duplicate the route setup in tests
- Timing-dependent: the 200ms interval and idle counter make tests slower

**Recommendation**: Extract SSE handlers into named functions in the `api` module so they can be mounted in test routers. This is a small refactor with high test value.

**Effort estimate**: 0.5 day refactor + 0.5 day for tests.

---

## 4. Test Fixtures (Minimal ROM Storage Layout)

**Current pattern in core tests**: Manual `std::env::temp_dir()` with an atomic counter to avoid collisions. No `tempfile` crate.

**Recommended structure for integration test fixtures**:

```
<tempdir>/
  roms/
    _favorites/           # empty initially
    _recent/              # empty initially
    nintendo_nes/
      TestGame.nes        # 13 bytes, fake data
      AnotherGame.nes     # 13 bytes, fake data
    sega_smd/
      Sonic.md            # 13 bytes, fake data
  .replay-control/
    metadata.db           # created lazily by MetadataDb::open()
    media/                # empty initially
  config/
    replay.cfg            # minimal: "storage_mode=sd\n"
```

**Helper function**:
```rust
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

static TEST_COUNTER: AtomicU32 = AtomicU32::new(0);

/// Create a temp directory with a minimal ROM storage layout.
/// Returns the temp directory path (caller should clean up if desired).
fn create_test_storage() -> PathBuf {
    let id = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp = std::env::temp_dir()
        .join(format!("replay-integ-{}-{id}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);

    // Create directory structure
    for dir in &[
        "roms/_favorites",
        "roms/_recent",
        "roms/nintendo_nes",
        "roms/sega_smd",
        ".replay-control/media",
        "config",
    ] {
        std::fs::create_dir_all(tmp.join(dir)).unwrap();
    }

    // Create fake ROM files
    std::fs::write(tmp.join("roms/nintendo_nes/TestGame.nes"), b"fake").unwrap();
    std::fs::write(tmp.join("roms/nintendo_nes/AnotherGame (USA).nes"), b"fake").unwrap();
    std::fs::write(tmp.join("roms/sega_smd/Sonic The Hedgehog (USA).md"), b"fake").unwrap();

    // Minimal config
    std::fs::write(tmp.join("config/replay.cfg"), "storage_mode=sd\n").unwrap();

    tmp
}
```

**On `tempfile` crate**: Adding `tempfile` as a dev-dependency would provide `TempDir` with automatic cleanup on drop. Worth adding but not strictly necessary given the existing pattern. The manual approach works fine and is already proven in the codebase.

**Effort estimate**: 0.5 day.

---

## 5. Metadata DB Testing at App Level

**Current coverage in core**: `MetadataDb` methods (`open`, `upsert`, `bulk_upsert`, `lookup`, `stats`, `clear`, etc.) are already tested via core-level unit tests using temp directories.

**What app-level tests would add**:
- Test that `AppState::metadata_db()` lazy initialization works correctly
- Test that server functions like `GetMetadataStats`, `GetSystemCoverage`, `ImportLaunchboxMetadata` return correct HTTP responses
- Test the enrichment pipeline: `enrich_from_metadata_cache()` populating `GameInfo` with description, rating, and image URLs

**Recommendation**: App-level metadata tests should focus on the HTTP layer (do the server functions return the right status codes and shapes?), not re-test DB operations. Pre-populate the DB using `MetadataDb::open()` + `upsert()` in test setup, then hit the server function endpoints.

```rust
#[tokio::test]
async fn metadata_stats_returns_zeros_for_empty_db() {
    let tmp = create_test_storage();
    let state = test_app_state(&tmp);
    // Trigger DB creation by accessing it
    let _ = state.metadata_db();
    let app = test_router_with_sfn(state);

    // POST to the server function endpoint
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/sfn/GetMetadataStats")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
}
```

**Effort estimate**: 0.5 day (leveraging existing HTTP test infrastructure).

---

## 6. Browser / E2E Testing

### Option A: Playwright / Cypress
- **Pros**: Tests real browser behavior including hydration, JavaScript interactivity, navigation, keyboard shortcuts (`/` for search)
- **Cons**: Requires running the full server, a built WASM bundle, and a browser. Slow, flaky, heavy CI dependencies
- **Cost**: 3-5 days setup + ongoing maintenance

### Option B: `wasm-bindgen-test`
- **Pros**: Tests WASM code in a headless browser; good for testing client-side logic (signal reactivity, DOM manipulation)
- **Cons**: Limited to client-side code. Cannot test SSR. Leptos 0.7 component testing in WASM is still immature
- **Cost**: 2-3 days for meaningful tests

### Option C: `leptos::testing` (if available)
- Leptos 0.7 does not ship a robust testing utility for component rendering. The `leptos_dom::testing` module from 0.6 was removed. Component-level testing in Leptos is effectively done via SSR snapshot tests (see section 7)
- **Cost**: N/A (not viable)

**Recommendation**: **Skip browser/E2E testing for now.** The cost-benefit ratio is poor for this project:
- The app is used by a single developer on a known device (Raspberry Pi)
- The UI is relatively simple (lists, detail pages, settings forms)
- Most bugs will be in the server-side logic (data processing, file operations), which is covered by HTTP-level tests
- Revisit if the app grows a larger user base or the UI becomes more complex

**Effort estimate**: 0 (deferred).

---

## 7. SSR Snapshot Testing

**Approach**: Render a page via SSR and assert on the HTML output. This catches regressions in component rendering, data binding, and conditional display logic.

**Implementation**: Use the existing Axum test infrastructure to GET a page URL (which hits the SSR fallback handler) and capture the HTML body.

```rust
#[tokio::test]
async fn home_page_renders_system_count() {
    let tmp = create_test_storage();
    let state = test_app_state(&tmp);
    let app = test_router_with_ssr(state);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = String::from_utf8(
        resp.into_body().collect().await.unwrap().to_bytes().to_vec()
    ).unwrap();
    // Verify key elements are present
    assert!(body.contains("Replay Control"));
    assert!(body.contains("nintendo_nes") || body.contains("NES"));
}
```

**Pros**:
- Fast (no browser needed)
- Catches rendering bugs, missing data, broken component logic
- Can use `insta` crate for snapshot management

**Cons**:
- SSR requires the full Leptos reactive runtime (`any_spawner::Executor::init_tokio()`)
- HTML output includes hydration markers that change between versions
- Brittle if checking exact HTML; better to assert on key content strings
- The SSR handler uses `include_str!(concat!(env!("OUT_DIR"), "/style.css"))` which means the app crate's `build.rs` must have run

**Recommendation**: Start with a few "smoke test" SSR assertions (page returns 200, contains expected text). Avoid full HTML snapshot comparisons due to hydration marker churn.

**Effort estimate**: 1 day (including `any_spawner` initialization setup).

---

## 8. CI Considerations

### Build Matrix

Integration tests must compile with `--features ssr`. The test command would be:

```bash
cargo test -p replay-control-app --features ssr --no-default-features
```

This is separate from unit tests in the core crate:
```bash
cargo test -p replay-control-core --features metadata
```

### Dependencies to Add

In `replay-control-app/Cargo.toml`:
```toml
[dev-dependencies]
tower = { version = "0.5", features = ["util"] }  # for ServiceExt::oneshot
http-body-util = "0.1"                              # for BodyExt::collect
```

`axum`, `tokio`, `serde_json`, `http` are already dependencies (behind `ssr` feature). For dev-dependencies they would be available unconditionally, but since integration tests only make sense with `ssr`, the existing feature-gated deps suffice. The `tower` dep already exists but may need the `util` feature for `ServiceExt`.

### Build Times

- Full `ssr` build from clean: ~45-60 seconds (includes building `rusqlite` with bundled SQLite, `quick-xml`, and the `phf` code generation in `build.rs`)
- Incremental test runs: ~5-10 seconds
- The WASM build is **not needed** for integration tests (only `ssr` feature)

### Test Data

- No external test data files needed; tests create their own temp directories
- The embedded databases (`arcade_db`, `game_db`, `systems`) are compiled into the binary via `build.rs` and `phf` -- they are available in tests automatically
- For metadata import tests that need a LaunchBox XML file: create a tiny synthetic XML in the test fixture (5-10 entries), not the full 460MB file

### Proposed CI Pipeline

```yaml
test:
  steps:
    - cargo test -p replay-control-core --features metadata
    - cargo test -p replay-control-app --features ssr --no-default-features
    # Optional: clippy on both feature sets
    - cargo clippy -p replay-control-app --features ssr --no-default-features
    - cargo clippy -p replay-control-app --features hydrate --no-default-features --target wasm32-unknown-unknown
```

---

## Recommended Test Architecture

### File Structure

```
replay-control-app/
  tests/
    common/
      mod.rs          # Test helpers: create_test_storage(), test_app_state(), test_router()
    api_systems.rs    # REST API: /api/systems, /api/info
    api_roms.rs       # REST API: /api/systems/:system/roms, /api/roms (delete/rename)
    api_favorites.rs  # REST API: /api/favorites (CRUD, group, flatten)
    api_upload.rs     # REST API: /api/upload/:system (multipart)
    sfn_basic.rs      # Server functions: get_info, get_systems, get_recents
    sfn_metadata.rs   # Server functions: metadata stats, coverage
    sse_progress.rs   # SSE endpoints (after handler extraction refactor)
    ssr_smoke.rs      # SSR page rendering smoke tests
```

All files in `tests/` are integration tests (separate compilation unit). They must use `--features ssr`.

### Shared Test Module

`tests/common/mod.rs` provides:
- `create_test_storage() -> PathBuf` -- temp dir with ROM layout
- `test_app_state(tmp: &Path) -> AppState` -- AppState pointing at temp dir
- `test_api_router(state: AppState) -> Router` -- REST API routes only
- `test_full_router(state: AppState) -> Router` -- with server function handler (requires registration)
- `assert_json_ok(resp: Response) -> serde_json::Value` -- helper to assert 200 + parse JSON

---

## Priority Order

| Priority | Category | Value | Effort | Rationale |
|----------|----------|-------|--------|-----------|
| 1 | REST API tests (`/api/*`) | High | Low | Most straightforward; tests real HTTP paths that external clients use. No Leptos dependency. |
| 2 | Server function tests (`/sfn/*`) | High | Medium | Tests the main UI data pipeline. Requires server function registration in test setup. |
| 3 | Test fixture helpers | High | Low | Foundation for all other tests. Build once, use everywhere. |
| 4 | SSE endpoint tests | Medium | Medium | Requires extracting inline handlers. Validates real-time progress reporting. |
| 5 | SSR smoke tests | Medium | Medium | Catches rendering regressions. Requires Leptos runtime initialization. |
| 6 | Metadata DB integration tests | Low | Low | Core already tests the DB well. App-level adds marginal value. |
| 7 | Browser/E2E tests | Low | High | Poor cost-benefit for a single-user app. Defer indefinitely. |

### Recommended Implementation Order

1. **Create test infrastructure** (Priority 3): `tests/common/mod.rs` with helpers
2. **REST API tests** (Priority 1): Start with `/api/systems`, `/api/info`, `/api/favorites`
3. **Server function tests** (Priority 2): Start with `GetInfo`, `GetSystems`, `GetRomsPage`
4. **SSE tests** (Priority 4): Extract handlers first, then test
5. **SSR smoke tests** (Priority 5): Home page, game list page

### Required Cargo.toml Changes

```toml
[dev-dependencies]
http-body-util = "0.1"
```

The `tower` dependency with `util` feature is needed for `ServiceExt::oneshot()`. Check if the existing `tower = "0.5"` dep (behind `ssr` feature) already includes `util`. If not, add it explicitly to dev-dependencies:

```toml
[dev-dependencies]
tower = { version = "0.5", features = ["util"] }
http-body-util = "0.1"
```

### Required Code Changes (Small Refactors)

1. **Extract SSE handlers** from `main.rs` into `api/sse.rs` as named functions, so they can be mounted in test routers
2. **Consider making `test_router` construction available** by extracting the Router assembly in `main.rs` into a reusable function (e.g., `pub fn build_router(state: AppState, leptos_options: LeptosOptions) -> Router`)

Neither refactor changes any behavior; they only improve testability.
