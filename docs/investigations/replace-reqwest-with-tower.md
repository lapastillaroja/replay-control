# Investigation: Replace reqwest with lighter alternative

Date: 2026-03-12
Status: Implemented (Option D -- curl)

## Motivation

The project uses `reqwest` as an HTTP client. The initial hypothesis was that
reqwest contributes to WASM bundle size. This investigation determines the
actual impact and evaluates alternatives.

## Key Finding: reqwest is NOT in the WASM bundle

**reqwest is SSR-only.** It is gated behind `optional = true` and only
activated by the `ssr` feature in `replay-control-app/Cargo.toml` (line 78).
The `hydrate` feature (used for the WASM build) does not include it.

Leptos `server_fn` uses **gloo-net** (which wraps browser `fetch`) for
client-side HTTP calls in WASM, not reqwest. This is configured automatically
via the `browser` feature of `server_fn`, which pulls in `gloo-net`, `web-sys`,
and `wasm-bindgen`. See `server_fn-0.7.8/src/request/browser.rs`.

**Removing reqwest would have zero impact on WASM bundle size.**

Current WASM bundle: 2,316,030 bytes raw, 699,366 bytes gzipped.

## Where reqwest is used

Exactly **1 place** in the entire codebase:

- `replay-control-app/src/server_fns/videos.rs` line 136
- Function: `search_game_videos` (a `#[server]` function, SSR-only)
- Purpose: HTTP GET to Piped/Invidious APIs for YouTube video search
- All target URLs are HTTPS

```rust
let client = reqwest::Client::builder()
    .timeout(std::time::Duration::from_secs(8))
    .build()
    .map_err(|e| ServerFnError::new(format!("HTTP client error: {e}")))?;
```

Used for simple GET + JSON deserialization against 6 API endpoints (3 Piped,
3 Invidious).

## SSR compile-time impact

reqwest pulls in the entire TLS stack exclusively. No other dependency in the
SSR build uses TLS. Removing reqwest would eliminate these crates (all
exclusively reachable through reqwest):

| Crate | Notes |
|-------|-------|
| reqwest | The HTTP client itself |
| hyper-rustls | Hyper HTTPS connector |
| rustls | Pure-Rust TLS implementation |
| rustls-webpki | WebPKI certificate validation |
| rustls-pki-types | PKI type definitions |
| ring | Cryptographic primitives (**C + assembly**, slowest to compile) |
| tokio-rustls | Async TLS streams |
| webpki-roots | Mozilla CA certificate bundle |
| untrusted | Input parsing for ring |
| subtle | Constant-time operations |
| zeroize | Secure memory zeroing |

That's **~11 crates removed**, including `ring` which has a C/assembly
compilation step and is one of the heavier crates to build.

Shared crates that would remain (also used by axum/leptos): hyper, hyper-util,
http, http-body, http-body-util, tower, tower-service, bytes, etc.

Total SSR crates currently: ~293 unique. Removing reqwest drops it to ~282.

## Alternatives evaluated

### Option A: hyper-util client (already in dep tree) + hyper-rustls

**NOT viable for eliminating TLS.** The target APIs (Piped, Invidious) all
require HTTPS. Using hyper directly would still need `hyper-rustls` or
`hyper-tls`, bringing back the same TLS stack. This would be strictly worse:
same dependencies, more verbose code.

### Option B: hyper-util + hyper-tls (native-tls / OpenSSL)

Replace rustls with the system's OpenSSL via `native-tls`. This eliminates
ring/rustls compilation but introduces a dependency on the system's
`libssl-dev`.

**Pros:**
- Eliminates ring C compilation (~11 crates gone, replaced by openssl-sys)
- Dynamically links TLS, so binary is smaller
- System CA certificates used automatically

**Cons:**
- Requires `libssl-dev` on the build machine and `libssl` at runtime on the Pi
- Cross-compilation for aarch64 becomes harder (need aarch64 OpenSSL headers)
- More verbose than reqwest (manual body reading, JSON parsing)
- Build script already needs aarch64-sysroot for SQLite; adding OpenSSL headers
  increases cross-compile complexity

**Verdict:** Not worth it. Trades one complexity for another.

### Option C: ureq (blocking HTTP client)

ureq is a synchronous HTTP client with much lighter dependencies. It supports
rustls or native-tls.

**Pros:**
- Much simpler API than raw hyper
- Fewer dependencies overall (~30% smaller binary in reported benchmarks)
- No async runtime needed (it's blocking)

**Cons:**
- Still needs TLS (rustls or native-tls), so ring/rustls remain unless
  native-tls is used
- Blocking I/O inside an async server function requires `spawn_blocking` or
  `block_in_place`, adding a thread pool hop
- With `ureq` + rustls: same TLS deps, marginal savings from removing hyper
  client code
- With `ureq` + native-tls: same OpenSSL cross-compile issues as Option B

**Verdict:** Marginal benefit. The TLS stack is the real cost, and ureq doesn't
eliminate it.

### Option D: Shell out to curl

Replace the reqwest call with `tokio::process::Command::new("curl")`.

**Pros:**
- Eliminates ALL HTTP client + TLS dependencies from the Rust build
- All 11 crates removed, including ring
- curl is available on the Pi (used by install.sh)
- Simple to implement: `curl -sS --max-time 8 <url>` piped to serde_json
- No cross-compilation complexity

**Cons:**
- Runtime dependency on curl binary (must be present on the Pi)
- Slightly more overhead per request (process spawn)
- Error handling is string-based (parse stderr)
- Less idiomatic Rust

**Example implementation:**
```rust
async fn http_get_json(url: &str, timeout_secs: u64) -> Result<serde_json::Value, String> {
    let output = tokio::process::Command::new("curl")
        .args(["-sS", "--max-time", &timeout_secs.to_string(), url])
        .output()
        .await
        .map_err(|e| format!("curl spawn failed: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("curl failed: {stderr}"));
    }

    serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("JSON parse error: {e}"))
}
```

**Verdict:** Best option for eliminating compile-time cost. The video search is
infrequent (user-initiated), low-throughput, and already tolerant of latency
(8s timeout). Process spawn overhead is negligible for this use case.

### Option E: reqwest with native-tls feature instead of rustls-tls

Switch `reqwest` from `rustls-tls` to `native-tls` feature.

**Pros:**
- Eliminates ring/rustls compilation
- Keeps the reqwest API (no code changes except Cargo.toml)

**Cons:**
- Same OpenSSL cross-compilation issues as Options B/C
- Still compiles reqwest itself + openssl-sys

**Verdict:** Only viable if the cross-compile story improves.

## Recommendation

### Short term: Option D (shell out to curl)

- **Effort:** ~30 minutes
- **Impact:** Removes ~11 crates from SSR build, including ring (C compilation)
- **Risk:** Low. curl is universally available. The function is SSR-only and
  called infrequently.
- **WASM impact:** None (reqwest was never in WASM)

### What NOT to do

- Do not try to replace the HTTP client used by Leptos server functions for
  browser-side calls. That is already gloo-net (browser fetch), not reqwest.
- Do not switch to native-tls/OpenSSL. The cross-compilation burden is not
  worth the trade.
- Do not use raw hyper for HTTPS. It requires the same TLS stack and is far
  more verbose.

## Impact summary

| Metric | Current | After removing reqwest |
|--------|---------|----------------------|
| WASM bundle (gzipped) | 699 KB | 699 KB (unchanged) |
| SSR unique crates | ~293 | ~282 |
| ring C compilation | Yes | No |
| TLS crates in SSR | 11 | 0 |
| Cross-compile complexity | Moderate | Slightly reduced |
| SSR clean build time | ~2m38s | ~2m20s (estimated) |
| SSR incremental build | Unaffected | Unaffected |

The primary benefit is **reduced SSR clean build time** (especially the ring
C/asm compilation step) and **simpler dependency tree**, not WASM bundle size.

---

## Implementation Notes

**Option D (shell out to curl) has been implemented.** The `curl_get_json()` async
helper function in `replay-control-app/src/server_fns/videos.rs` spawns `curl -sS
--max-time {timeout}` via `tokio::process::Command` and parses the stdout as JSON.
It is used by `search_game_videos()` to query Piped and Invidious API instances.

reqwest has been fully removed from the dependency tree -- it no longer appears in
any `Cargo.toml` file in the workspace.
