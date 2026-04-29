# Installation

How to install, update, and uninstall Replay Control on a Raspberry Pi running RePlayOS.

## Install

```bash
curl -fsSL https://raw.githubusercontent.com/lapastillaroja/replay-control/main/install.sh | bash
```

Run this on your Pi over SSH or from any computer on the same network — the installer detects which case it's in and does the right thing. It auto-discovers the Pi via mDNS, downloads the latest stable release (falling back to the latest beta if no stable exists), installs the systemd service, and starts it. The web UI will be at `http://replay.local:8080`.

### If auto-discovery fails

Pass the Pi's IP address explicitly:

```bash
curl -fsSL https://raw.githubusercontent.com/lapastillaroja/replay-control/main/install.sh | bash -s -- --ip 192.168.1.50
```

Find the IP in your router's connected-devices list, or run `hostname -I` on the Pi.

## Update / Uninstall

Re-run the install command to update; the binary and site assets are replaced and the service is restarted, while `/etc/default/replay-control` is preserved.

To remove the app:

```bash
curl -fsSL https://raw.githubusercontent.com/lapastillaroja/replay-control/main/install.sh | bash -s -- --uninstall
```

The service is stopped and disabled, and the binary, site assets, systemd unit, and Avahi service are removed. The environment file is preserved in case you reinstall.

To wipe everything Replay Control has put on the Pi — binary, service files, environment file, and the on-storage `.replay-control/` directory (DBs, settings, downloaded media, LaunchBox XML):

```bash
curl -fsSL https://raw.githubusercontent.com/lapastillaroja/replay-control/main/install.sh | bash -s -- --purge --yes
```

`--purge` prompts for confirmation when run interactively; pass `--yes` to skip the prompt (required when piping from `curl`). ROMs, saves, captures, and BIOS files are not touched.

## Options

Append flags after `bash -s --`:

| Flag | Effect |
|---|---|
| `--ip ADDRESS` | Skip mDNS discovery and use this address. |
| `--pi-pass PASSWORD` | SSH password for the Pi (default: `replayos`). |
| `--version v0.3.0` | Install a specific release. Use `beta` for the latest pre-release. |
| `--dry-run` | Print what would happen without making changes. |
| `--uninstall` | Remove the app from the Pi. Preserves `.replay-control/` data and the env file. |
| `--purge` | Like `--uninstall` but also wipes `.replay-control/` and `/etc/default/replay-control`. ROMs/saves/captures/BIOS untouched. |
| `--yes` | Skip the confirmation prompt (required for `--purge` when piping from `curl`). |

Or set environment variables before the curl call:

| Variable | Default | Effect |
|---|---|---|
| `PI_PASS` | `replayos` | Same as `--pi-pass`. |
| `REPLAY_PI_ADDR` | (mDNS) | Same as `--ip`. |
| `REPLAY_CONTROL_VERSION` | `latest` | Same as `--version`. |

Combining options:

```bash
curl -fsSL https://raw.githubusercontent.com/lapastillaroja/replay-control/main/install.sh | bash -s -- --pi-pass mypassword --version v0.3.0
```

## SD card install

Write to a mounted RePlayOS SD card before first boot — useful when the Pi isn't on the network yet. This mode needs the script on disk because the SD partitions need to be mountable from the same machine:

```bash
wget https://raw.githubusercontent.com/lapastillaroja/replay-control/main/install.sh
bash install.sh --sdcard
```

The installer needs the **rootfs** (ext4) partition mounted, not the data partition. On Linux that partition often doesn't auto-mount — `lsblk -o NAME,LABEL,FSTYPE` shows the labels; mount it manually if needed:

```bash
sudo mount /dev/sdX2 /mnt/replayos-rootfs
bash install.sh --sdcard /mnt/replayos-rootfs
```

The app will start automatically when the Pi boots. Uninstall isn't supported in SD-card mode — remove via SSH after first boot instead.

## What gets installed

| Path | Contents |
|---|---|
| `/usr/local/bin/replay-control-app` | Application binary. |
| `/usr/local/bin/catalog.sqlite` | Read-only embedded game catalog. The service won't start without it. |
| `/usr/local/share/replay/site/` | Static web assets (CSS, WASM, icons). |
| `/etc/systemd/system/replay-control.service` | Systemd service unit. |
| `/etc/default/replay-control` | Environment configuration. |
| `/etc/avahi/services/replay-control.service` | mDNS service advertisement. |

The service starts automatically on boot and listens on port 8080. Customise behaviour by editing `/etc/default/replay-control`:

| Variable | Default | Effect |
|---|---|---|
| `REPLAY_PORT` | `8080` | Web UI port. |
| `REPLAY_SITE_ROOT` | `/usr/local/share/replay/site` | Static-assets path. |
| `REPLAY_STORAGE_PATH` | (auto-detected) | Override ROM storage path. |
| `REPLAY_CONFIG_PATH` | (auto-detected) | Override the RePlayOS config path. |
| `RUST_LOG` | `replay_control_app=info,replay_control_core=info` | Log level. |

## Troubleshooting

**Pi not found.** Make sure the Pi is powered on and on the same network. Check your router for its IP and pass `--ip <addr>`.

**SSH authentication failed.** The default password is `replayos`. If you've changed it, pass `--pi-pass yourpassword`.

**SD card rootfs not mounted.** On Linux, the ext4 rootfs partition often doesn't auto-mount. Use `lsblk -o NAME,LABEL,FSTYPE` to find it and mount it manually.

**Windows.** Run the install commands inside [WSL](https://learn.microsoft.com/en-us/windows/wsl/).
