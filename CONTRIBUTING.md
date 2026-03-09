# Contributing

## Prerequisites

- **Rust** (stable) via [rustup](https://rustup.rs/)
- **wasm-bindgen-cli**: `cargo install wasm-bindgen-cli`
- **Rust targets**:
  ```
  rustup target add wasm32-unknown-unknown
  rustup target add aarch64-unknown-linux-gnu
  ```

### Cross-compilation (for deploying to Pi)

The server binary must be compiled for aarch64 to run on RePlayOS (Raspberry Pi).

**Fedora:**
```
sudo dnf install gcc-aarch64-linux-gnu
```

**Ubuntu / Debian:**
```
sudo apt install gcc-aarch64-linux-gnu
```

**macOS:**
```
brew install aarch64-unknown-linux-gnu
```

The Cargo linker config is already set up in `.cargo/config.toml`.


## Building

### Native (for local development)

```
./build.sh
```

Produces an x86_64 binary at `target/release/replay-app` and site assets at `target/site/`.

Run locally:
```
./target/release/replay-app --storage-path /run/media/$USER/replay-roms --site-root target/site
```

### Cross-compile for Pi (aarch64)

```
./build.sh --target aarch64
```

Produces an aarch64 binary at `target/aarch64-unknown-linux-gnu/release/replay-app`.


## Deploying to a Pi

### Via SSH (recommended)

With the Pi on the network:

```
bash install.sh --local --ip <pi-address>
```

The install script packages the local build, transfers it to the Pi over SSH, sets up the systemd service, and starts it. The Pi's RePlayOS credentials are used automatically.

If the Pi is discoverable via mDNS (`replaypi.local`), you can omit `--ip`:

```
bash install.sh --local
```

### Via SD card (first-time setup)

Mount the Pi's SD card rootfs partition on your computer, then:

```
bash install.sh --local --sdcard /path/to/rootfs
```

The app will start automatically on the next boot.

### Dry run

Preview what the installer would do without making changes:

```
bash install.sh --local --dry-run --ip <pi-address>
```


## Project structure

```
replay/
├── replay-core/          # Business logic (native only, not WASM)
├── replay-app/           # Leptos SSR app (server + hydration)
│   ├── src/
│   │   ├── main.rs       # Axum server entry point
│   │   ├── lib.rs        # App component + hydrate entry
│   │   ├── server_fns.rs # Leptos server functions
│   │   ├── types.rs      # Client-side mirror types
│   │   ├── i18n.rs       # Internationalization
│   │   ├── api/          # REST API (SSR only)
│   │   ├── components/   # Shared UI components
│   │   └── pages/        # Page components
│   └── style/
│       └── style.css
├── build.sh              # Build script (WASM + server)
├── install.sh            # Installer (SSH or SD card)
├── docs/                 # Design documents
└── data/                 # Arcade DB source files
```

### Two-crate architecture

- **`replay-core`**: ROM management, favorites, recents, storage, arcade DB. Native only (`std::fs`).
- **`replay-app`**: Leptos SSR + hydration. Has two Cargo features:
  - `ssr` — server binary (Axum, depends on `replay-core`)
  - `hydrate` — WASM client for browser hydration

Both features share the same components, pages, and types. Server functions (`#[server]`) are direct calls on the server and HTTP requests on the client.


## Development tips

- Always rebuild both WASM and server after changing shared types (`types.rs`, `server_fns.rs`). Stale WASM causes hydration failures.
- If `cargo clean -p replay-app` doesn't pick up changes, delete `target/wasm32-unknown-unknown/release/deps/replay_app*` manually.
- Server functions from `replay-core` need explicit registration in `main.rs` — the linker strips them otherwise.
- The app auto-detects storage at `/media/sd`, `/media/usb`, `/media/nfs`, or falls back to `--storage-path`.
