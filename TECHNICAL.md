# Technical Reference

Low-level implementation details, gotchas, and debugging notes.

## Leptos 0.7 SSR (without cargo-leptos)

When using `render_app_to_stream_with_context` directly (without `generate_route_list` / `leptos_routes_with_handler`):

1. **Executor must be initialized manually** — call `any_spawner::Executor::init_tokio()` before any SSR rendering. The reactive system's `Resource` uses `Executor::spawn` internally; without initialization, async tasks silently fail and `Suspense` boundaries never resolve.

2. **Server functions need explicit registration** — when `#[server]` functions are defined in the library crate (`lib.rs`) and consumed from the binary crate (`main.rs`), the `inventory` crate's auto-registration is stripped by the linker. Call `server_fn::axum::register_explicit::<StructName>()` for each server function in `main()`. The struct name is the function name in PascalCase (e.g., `get_info` → `GetInfo`).

3. **Axum version must match leptos_axum** — `leptos_axum` 0.7 depends on `axum` 0.7 (`axum-core` 0.4). Using `axum` 0.8 causes `Body` type mismatches.

## Build Pipeline

`cargo-leptos` was not used because it requires `openssl-sys` headers during install. Instead, `build.sh` runs:

1. `cargo build --lib --target wasm32-unknown-unknown --features hydrate` — WASM client
2. `wasm-bindgen` — generates JS glue + optimized `.wasm` into `target/site/pkg/`
3. `cargo build --bin replay-control-app --features ssr` — server binary

The `wasm-bindgen-cli` version must match the `wasm-bindgen` crate version in `Cargo.lock`. The `--out-name` must use underscores (matching `LeptosOptions::output_name`).
