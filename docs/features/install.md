# Installation

How to install, update, and uninstall Replay Control on a Raspberry Pi running RePlayOS.

## Quick Install

From any computer on the same network as the Pi:

```bash
curl -fsSL https://raw.githubusercontent.com/lapastillaroja/replay-control/main/install.sh | bash -s -- --ip replay.local
```

The installer downloads the latest release from GitHub and deploys to the Pi over SSH. If `replay.local` doesn't work, replace it with your Pi's IP address (e.g., `your-pi-ip`).

## Install Methods

### SSH Install (from another computer)

The default method. Downloads the latest release and deploys to the Pi over SSH.

```bash
# Using replay.local (default mDNS hostname)
curl -fsSL https://raw.githubusercontent.com/lapastillaroja/replay-control/main/install.sh | bash -s -- --ip replay.local

# Using a specific IP address
curl -fsSL https://raw.githubusercontent.com/lapastillaroja/replay-control/main/install.sh | bash -s -- --ip your-pi-ip
```

> **Tip:** To find your Pi's IP address, check your router's connected devices list, or run `hostname -I` on the Pi.

If you already downloaded `install.sh`:

```bash
bash install.sh --ip replay.local
bash install.sh --ip your-pi-ip
```

### SSH Install (already on the Pi)

If you are logged into the Pi via SSH, the same script works:

```bash
curl -fsSL https://raw.githubusercontent.com/lapastillaroja/replay-control/main/install.sh | bash
```

### SD Card Install (from a computer)

Write directly to a mounted RePlayOS SD card before first boot. The app will start automatically when the Pi boots.

```bash
# Auto-detect the SD card
bash install.sh --sdcard

# Or specify the rootfs mount point
bash install.sh --sdcard /run/media/user/rootfs
```

The installer looks for mounted partitions with the RePlayOS signature (data partition with `roms/`, `bios/`, `config/replay.cfg`; boot partition with `issue.txt`). It needs the **rootfs** partition (ext4), not the data partition.

On Linux, you may need to mount the rootfs partition manually:

```bash
sudo mount /dev/sdX2 /mnt/replayos-rootfs
bash install.sh --sdcard /mnt/replayos-rootfs
```

## Options

### Custom Password

The default SSH password for RePlayOS is `replayos`. If you have changed it:

```bash
PI_PASS=mypassword bash install.sh --ip replay.local
```

Or with curl:

```bash
PI_PASS=mypassword curl -fsSL https://raw.githubusercontent.com/lapastillaroja/replay-control/main/install.sh | bash
```

### Specific Version

Install a particular release instead of the latest:

```bash
REPLAY_CONTROL_VERSION=v0.2.0 bash install.sh
```

### Pi Address via Environment Variable

Instead of `--ip`, you can set the address via environment variable:

```bash
REPLAY_PI_ADDR=your-pi-ip bash install.sh
```

### Dry Run

Preview what the installer would do without making any changes:

```bash
bash install.sh --dry-run
```

### Local Build Install

Deploy a locally built binary instead of downloading a release:

```bash
# Use artifacts from the current directory
bash install.sh --local

# Use artifacts from a specific directory
bash install.sh --local /path/to/replay-control
```

This expects `target/release/replay-control-app` (or `target/aarch64-unknown-linux-gnu/release/replay-control-app`) and `target/site/` to exist. Run `./build.sh` or `./build.sh aarch64` first.

## What Gets Installed

The installer places these files on the Pi:

| File | Purpose |
|------|---------|
| `/usr/local/bin/replay-control-app` | Application binary |
| `/usr/local/share/replay/site/` | Static web assets (CSS, WASM, icons) |
| `/etc/systemd/system/replay-control.service` | Systemd service unit |
| `/etc/default/replay-control` | Environment configuration |
| `/etc/avahi/services/replay-control.service` | mDNS service advertisement |

The service starts automatically on boot and listens on port 8080.

## Update

To update to the latest version, run the installer again. It overwrites the binary and site assets, restarts the service, and preserves your environment configuration.

```bash
curl -fsSL https://raw.githubusercontent.com/lapastillaroja/replay-control/main/install.sh | bash
```

To update to a specific version:

```bash
REPLAY_CONTROL_VERSION=v0.3.0 bash install.sh
```

## Uninstall

Remove the app from the Pi:

```bash
bash install.sh --uninstall
```

This stops and disables the service, removes the binary, site assets, systemd unit, and Avahi service file. The environment file (`/etc/default/replay-control`) is preserved in case you want to reinstall later.

Uninstall is only supported via SSH, not in SD card mode.

## Environment Configuration

After installation, you can customize behavior by editing `/etc/default/replay-control` on the Pi:

| Variable | Default | Description |
|----------|---------|-------------|
| `REPLAY_PORT` | `8080` | Web UI port |
| `REPLAY_SITE_ROOT` | `/usr/local/share/replay/site` | Static assets path |
| `REPLAY_STORAGE_PATH` | (auto-detected) | Override ROM storage path |
| `REPLAY_CONFIG_PATH` | (auto-detected) | Override RePlayOS config path |
| `RUST_LOG` | `replay_control_app=info,replay_control_core=info` | Log level |

## Windows Users

Run the install commands in [WSL](https://learn.microsoft.com/en-us/windows/wsl/) (Windows Subsystem for Linux).

## Troubleshooting

**Pi not found:** Ensure the Pi is powered on and connected to the same network. Try specifying the IP directly with `--ip`.

**SSH authentication failed:** Check the password. The default is `replayos`. Use `PI_PASS=yourpassword` if you have changed it.

**SD card rootfs not mounted:** On Linux, the ext4 rootfs partition may not auto-mount. Use `lsblk -o NAME,LABEL,FSTYPE` to find the right partition and mount it manually.
