# Cross-Compilation for aarch64 (Raspberry Pi)

The app cross-compiles from x86_64 to aarch64 for deployment on the Pi. This requires a C cross-compiler and target sysroot because `rusqlite` bundles SQLite and compiles it from C source.

## Prerequisites

- Fedora (tested on 42)
- Rust with aarch64 target: `rustup target add aarch64-unknown-linux-gnu`

## Setup (Fedora)

### 1. Install the cross-compiler toolchain

```bash
sudo dnf install gcc-aarch64-linux-gnu binutils-aarch64-linux-gnu
```

### 2. Populate GCC's sysroot with aarch64 headers and libraries

This uses `dnf --installroot` to install aarch64 glibc into GCC's default sysroot path. GCC finds it automatically — no `--sysroot` flags needed.

```bash
sudo dnf \
  --installroot=/usr/aarch64-linux-gnu/sys-root \
  --releasever=$(rpm -E %fedora) \
  --forcearch=aarch64 \
  --use-host-config \
  -y install glibc-devel
```

### 3. Create the libgcc_s linker script

The `gcc-aarch64-linux-gnu` package doesn't include `libgcc_s.so` (the linker script that resolves `-lgcc_s`). Create it manually:

```bash
echo 'GROUP ( /usr/lib64/libgcc_s.so.1 )' \
  | sudo tee /usr/aarch64-linux-gnu/sys-root/usr/lib64/libgcc_s.so
```

The path `/usr/lib64/libgcc_s.so.1` is sysroot-relative — the linker resolves it inside the sysroot.

### 4. Verify

```bash
# Should show the sysroot contents
ls /usr/aarch64-linux-gnu/sys-root/usr/include/stdio.h
ls /usr/aarch64-linux-gnu/sys-root/usr/lib64/crti.o
ls /usr/aarch64-linux-gnu/sys-root/usr/lib64/libgcc_s.so

# Should build successfully
cargo check -p replay-control-app --target aarch64-unknown-linux-gnu --features ssr --no-default-features
```

## Cargo configuration

The only cargo configuration needed is the linker (in `.cargo/config.toml`):

```toml
[target.aarch64-unknown-linux-gnu]
linker = "aarch64-linux-gnu-gcc"
```

No `--sysroot` rustflags are needed — GCC finds its sysroot automatically at `/usr/aarch64-linux-gnu/sys-root`.

## Updating after Fedora upgrade

When upgrading to a new Fedora release, update the sysroot:

```bash
sudo dnf \
  --installroot=/usr/aarch64-linux-gnu/sys-root \
  --releasever=$(rpm -E %fedora) \
  --forcearch=aarch64 \
  --use-host-config \
  -y distro-sync
```

## Building

```bash
# Full build (WASM + aarch64 server)
./build.sh aarch64

# Dev build + deploy to Pi
./dev.sh --pi
```

## Troubleshooting

**`cannot find crti.o`** or **`cannot find -lgcc_s`**: The sysroot is missing or incomplete. Re-run step 2 and 3 above.

**`cannot find /lib64/libm.so.6 inside sysroot`**: The sysroot lacks runtime glibc. The `glibc-devel` package should include it; if not, also install `glibc`:

```bash
sudo dnf \
  --installroot=/usr/aarch64-linux-gnu/sys-root \
  --releasever=$(rpm -E %fedora) \
  --forcearch=aarch64 \
  --use-host-config \
  -y install glibc
```

**OOM during linking**: The aarch64 linker uses ~1.5GB RAM. Ensure no other heavy builds are running. The cargo config limits parallelism to 8 jobs (half of 16 cores).
