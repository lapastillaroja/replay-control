# Deployment on RePlayOS

Design for deploying Replay Control on a Raspberry Pi running RePlayOS. Covers building, releasing, installing from a user's computer (Windows, macOS, or Linux), running as a system service, and updating.


## 1. Overview

Replay Control runs on the Raspberry Pi, not on the user's computer. The user's computer is only used to trigger the installation. There are two installation methods:

1. **Remote install via SSH** (primary) -- a script runs on the user's computer, connects to the Pi over the network via SSH, and deploys the app. The user never manually opens a terminal on the Pi. Works on Windows, macOS, and Linux.
2. **Direct SD card write** (secondary) -- for Unix systems only. The user mounts the Pi's SD card on their computer and the script writes files directly to it. Useful for first-time setup before the Pi has ever booted, or when SSH is unavailable.

The app consists of two artifacts produced by `build.sh`:

- **`replay-control-app`** -- the server binary (Axum + Leptos SSR), built for Linux aarch64
- **`site/`** -- static assets for client-side hydration (WASM bundle, icons, CSS)

At runtime the binary serves the web UI on a configurable port (default 8080) and needs read/write access to the RePlayOS storage location (`/media/sd`, `/media/usb`, or `/media/nfs`).

RePlayOS runs as root with publicly known credentials. The Replay Control service runs as root accordingly.


## 2. Build

### Native build

```
./build.sh
```

Produces a binary for the host architecture. On an x86_64 dev machine this gives a Linux x86_64 binary; on the Pi itself, an aarch64 binary.

### Cross-compilation (aarch64)

RePlayOS runs on Raspberry Pi (aarch64). Cross-compilation from an x86_64 host requires:

- The `aarch64-unknown-linux-gnu` Rust target (for the server binary)
- The `wasm32-unknown-unknown` target (for the hydration bundle -- architecture-independent)
- An aarch64 linker and sysroot (`gcc-aarch64-linux-gnu` or equivalent)

```
TARGET=aarch64-unknown-linux-gnu ./build.sh
```

Only the server binary build step changes. The WASM build and asset copy are identical. The resulting binary lands at `target/aarch64-unknown-linux-gnu/release/replay-control-app`.


## 3. GitHub Releases

Each release publishes pre-built artifacts to GitHub Releases.

### Release artifacts

| Artifact | Contents |
|---|---|
| `replay-control-app-aarch64-linux.tar.gz` | Server binary for Linux aarch64 (Raspberry Pi / RePlayOS) |
| `replay-control-app-x86_64-linux.tar.gz` | Server binary for Linux x86_64 (development machines) |
| `replay-site.tar.gz` | Static site assets (`pkg/`, icons, CSS) -- architecture-independent |

### Naming convention

```
replay-control-app-{ARCH}-{OS}.tar.gz
```

Where `ARCH` is `aarch64` or `x86_64`, and `OS` is `linux` or `darwin`.

The site assets archive is always `replay-site.tar.gz` since it contains architecture-independent WASM and static files.

### Tar structure

Each binary archive contains a single file at the root:

```
replay-control-app-aarch64-linux.tar.gz
  └── replay-control-app
```

The site archive preserves the directory structure:

```
replay-site.tar.gz
  └── site/
      ├── pkg/
      │   ├── replay-control-app.js
      │   ├── replay-control-app_bg.wasm
      │   └── ...
      ├── icons/
      ├── style.css
      └── ...
```


## 4. Installer

### User experience

The target user has a Raspberry Pi running RePlayOS and a personal computer (often Windows) with no technical knowledge of SSH or the command line. The installer must feel like running a single command and seeing "done".

**On macOS / Linux:**

```bash
curl -fsSL https://raw.githubusercontent.com/user/replay/main/install.sh | bash
```

**On Windows (PowerShell):**

```powershell
irm https://raw.githubusercontent.com/user/replay/main/install.ps1 | iex
```

Both scripts do the same thing: download the aarch64 binary and site assets from GitHub Releases, connect to the Pi over SSH, and deploy them. The user is prompted for the Pi's IP address if it cannot be auto-discovered.


### Architecture

The installer is two separate scripts (Bash and PowerShell) that share no code. They are kept independent because:

- PowerShell and Bash have fundamentally different idioms; a shared-logic approach (e.g., generating one from the other) would be brittle.
- Each script is small enough (~200 lines) that duplication is acceptable.
- The deployment logic on the Pi side is trivial: copy files, write a systemd unit, reload, enable, start. There is no complex shared logic to keep in sync.

Both scripts implement the same behavior:

1. Determine the release URL (latest or pinned version).
2. Download `replay-control-app-aarch64-linux.tar.gz` and `replay-site.tar.gz` to a temp directory.
3. Discover or prompt for the Pi's IP address.
4. Transfer files to the Pi via SCP.
5. Run the installation commands on the Pi via SSH (extract, install to `/usr/local/`, set up systemd, start the service).
6. Verify the service is running and print the access URL.


### Pi discovery

The scripts try to locate the Pi automatically before prompting the user:

1. **mDNS** -- try `replaypi.local`. On macOS this works out of the box. On Linux it works if `avahi` is installed. On Windows it works if Bonjour or the Link-Local Multicast Name Resolution service is available (common on Windows 10+).
2. **User prompt** -- if mDNS resolution fails, ask the user for the Pi's IP address. Print a helpful message: "Could not find your RePlay Pi automatically. Enter its IP address (you can find this in your router's admin page)."
3. **Connectivity check** -- after resolving the address, verify SSH connectivity (port 22 open) before proceeding. Fail fast with a clear message if the Pi is unreachable.

Network scanning (e.g., ARP, nmap-style) is intentionally omitted. It is slow, unreliable across OSes, and requires elevated privileges on some systems. Asking the user for an IP is simpler and more reliable.


### SSH authentication

RePlayOS uses well-known public credentials. The installer automates SSH login without user interaction.

**Credentials** (stored as constants in the scripts):

- User: `root`
- Password: `replayos`

**All platforms** use the built-in OpenSSH client (`ssh` / `scp`). Windows 10 (1809+) and Windows 11 ship with these. macOS and Linux have them preinstalled.

- Pass `-o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null` to avoid host key prompts. This is acceptable because RePlayOS is a local appliance with publicly known credentials -- there is no meaningful MITM threat model.

**Password automation:**

For non-interactive password entry, both scripts use `SSH_ASKPASS`:

1. Write a tiny helper script to a temp file that outputs the password.
2. Set `SSH_ASKPASS` to point to it and `SSH_ASKPASS_REQUIRE=force` (OpenSSH 8.4+) to bypass the terminal check.
3. Run `ssh` / `scp` normally -- OpenSSH calls the askpass helper instead of prompting.

**Bash:**

```bash
ASKPASS="$(mktemp)"
printf '#!/bin/sh\necho "%s"\n' "$PASSWORD" > "$ASKPASS"
chmod +x "$ASKPASS"
export SSH_ASKPASS="$ASKPASS"
export SSH_ASKPASS_REQUIRE="force"

scp -o StrictHostKeyChecking=no ... root@$PI_ADDR:/tmp/
ssh -o StrictHostKeyChecking=no root@$PI_ADDR "commands..."

rm -f "$ASKPASS"
```

**PowerShell:**

```powershell
$askpass = "$env:TEMP\replay-askpass.cmd"
Set-Content $askpass "@echo off`necho $password"
$env:SSH_ASKPASS = $askpass
$env:SSH_ASKPASS_REQUIRE = "force"

scp.exe -o StrictHostKeyChecking=no ... root@${piAddress}:/tmp/
ssh.exe -o StrictHostKeyChecking=no root@$piAddress "commands..."

Remove-Item $askpass
```

**Fallback:** If `SSH_ASKPASS_REQUIRE` is not supported (older OpenSSH), the script prints "When prompted for the password, type: `<password>`" and lets the user enter it manually. Since the password is publicly known and short, this is a minor inconvenience, not a blocker. No third-party tools are needed.


### Remote installation sequence

Once files are transferred to `/tmp/` on the Pi, both scripts run the same sequence over SSH:

```bash
# Extract binary
tar -xzf /tmp/replay-control-app-aarch64-linux.tar.gz -C /tmp/
install -m755 /tmp/replay-control-app /usr/local/bin/replay-control-app

# Extract site assets
rm -rf /usr/local/share/replay/site
mkdir -p /usr/local/share/replay
tar -xzf /tmp/replay-site.tar.gz -C /usr/local/share/replay/

# Write systemd service file
cat > /etc/systemd/system/replay-companion.service << 'UNIT'
[Unit]
Description=Replay Control
After=network.target
After=media-sd.mount media-usb.mount

[Service]
Type=simple
EnvironmentFile=-/etc/default/replay-companion
ExecStart=/usr/local/bin/replay-control-app \
    --port ${REPLAY_PORT} \
    --site-root ${REPLAY_SITE_ROOT}
Restart=on-failure
RestartSec=5
StandardOutput=journal
StandardError=journal
SyslogIdentifier=replay-companion

[Install]
WantedBy=multi-user.target
UNIT

# Write default environment file (preserve existing)
if [ ! -f /etc/default/replay-companion ]; then
    cat > /etc/default/replay-companion << 'ENV'
REPLAY_PORT=8080
REPLAY_SITE_ROOT=/usr/local/share/replay/site
RUST_LOG=replay_control_app=info,replay_control_core=info
ENV
fi

# Write Avahi service for mDNS discovery
if [ -d /etc/avahi/services ]; then
    cat > /etc/avahi/services/replay-companion.service << 'AVAHI'
<?xml version="1.0" standalone='no'?>
<!DOCTYPE service-group SYSTEM "avahi-service.dtd">
<service-group>
  <name>Replay Control</name>
  <service>
    <type>_http._tcp</type>
    <port>8080</port>
  </service>
</service-group>
AVAHI
fi

# Reload and start
systemctl daemon-reload
systemctl enable replay-companion
systemctl restart replay-companion

# Cleanup
rm -f /tmp/replay-control-app-aarch64-linux.tar.gz /tmp/replay-site.tar.gz /tmp/replay-control-app
```

This entire sequence is sent as a single SSH command (heredoc or semicolon-separated) to minimize round trips.


### SD card direct write (Unix only)

For first-time setup before the Pi has booted, or when SSH is unavailable, the Bash script supports writing directly to a mounted SD card.

**Invocation:**

```bash
curl -fsSL https://raw.githubusercontent.com/user/replay/main/install.sh | bash -s -- --sdcard
```

Or with an explicit mount point:

```bash
curl -fsSL https://raw.githubusercontent.com/user/replay/main/install.sh | bash -s -- --sdcard /media/user/REPLAYOS
```

**SD card detection:**

If no mount point is given, the script searches for the RePlayOS root partition:

1. Look for mounted filesystems matching common labels: `REPLAYOS`, `replayos`, or partitions with a `/etc/replayos-release` marker file (or similar RePlayOS-specific file).
2. Search common mount points: `/run/media/$USER/*`, `/media/$USER/*`, `/mnt/*`.
3. If multiple candidates are found, list them and ask the user to pick one.
4. If none are found, print instructions for mounting the SD card and exit.

**Validation:** Before writing, the script checks for a RePlayOS marker (e.g., a specific file or directory structure) to avoid accidentally writing to the wrong disk.

**Write process:**

The script writes directly to the SD card's root filesystem:

```bash
SD_ROOT="/run/media/user/REPLAYOS"

install -m755 replay-control-app "$SD_ROOT/usr/local/bin/replay-control-app"
mkdir -p "$SD_ROOT/usr/local/share/replay"
cp -r site/ "$SD_ROOT/usr/local/share/replay/site"

# Install systemd service
cp replay-companion.service "$SD_ROOT/etc/systemd/system/"

# Install environment file (if not present)
if [ ! -f "$SD_ROOT/etc/default/replay-companion" ]; then
    cp replay-companion.env "$SD_ROOT/etc/default/replay-companion"
fi

# Enable the service for first boot
ln -sf /etc/systemd/system/replay-companion.service \
    "$SD_ROOT/etc/systemd/system/multi-user.target.wants/replay-companion.service"
```

The service starts automatically on the next boot.

**Why not on Windows:** Detecting and mounting ext4 partitions on Windows requires third-party drivers (ext2fsd, WSL mount). It is fragile and not worth supporting. Windows users should use the SSH method after the Pi has booted at least once.


### Script flags and environment variables

**Bash (`install.sh`):**

| Flag / Env var | Description |
|---|---|
| `--help` | Show usage |
| `--uninstall` | Remove the app from a connected Pi via SSH |
| `--sdcard [path]` | Use SD card direct write instead of SSH |
| `--ip <address>` | Skip Pi discovery, use this address |
| `REPLAY_VERSION` | Release tag to install (default: `latest`) |
| `REPLAY_PI_ADDR` | Pi address, same as `--ip` |

**PowerShell (`install.ps1`):**

| Parameter | Description |
|---|---|
| `-Help` | Show usage |
| `-Uninstall` | Remove the app from a connected Pi via SSH |
| `-PiAddress <addr>` | Skip Pi discovery, use this address |
| `-Version <tag>` | Release tag to install (default: `latest`) |


### Error handling

Both scripts must handle these failure modes gracefully:

| Failure | Behavior |
|---|---|
| No internet | "Error: cannot reach GitHub. Check your internet connection." |
| Release not found | "Error: release vX.Y.Z not found. Check https://github.com/user/replay/releases for available versions." |
| Pi not reachable | "Error: cannot connect to <address>:22. Is the Pi powered on and connected to your network?" |
| SSH auth failure | "Error: SSH authentication failed. RePlayOS credentials may have changed." |
| Disk full on Pi | "Error: not enough disk space on the Pi." (check before writing) |
| SSH not available (Win) | "Error: ssh.exe not found. Install OpenSSH from Windows Settings > Apps > Optional Features." |
| SD card not found | "Error: no RePlayOS SD card found. Mount the SD card and try again, or specify the path: install.sh --sdcard /path/to/mount" |

All scripts use colored output (green for success, red for errors, yellow for warnings) where the terminal supports it. Progress messages are printed at each step so the user knows what is happening.


## 5. systemd Service

### Service file: `replay-companion.service`

Installed to `/etc/systemd/system/replay-companion.service` by the installer.

```ini
[Unit]
Description=Replay Control
After=network.target
After=media-sd.mount media-usb.mount

[Service]
Type=simple
EnvironmentFile=-/etc/default/replay-companion
ExecStart=/usr/local/bin/replay-control-app \
    --port ${REPLAY_PORT} \
    --site-root ${REPLAY_SITE_ROOT}
Restart=on-failure
RestartSec=5
StandardOutput=journal
StandardError=journal
SyslogIdentifier=replay-companion

[Install]
WantedBy=multi-user.target
```

No `User=` or `Group=` directive -- the service runs as root, same as everything else on RePlayOS.

### Environment file: `/etc/default/replay-companion`

```bash
# Port for the web UI
REPLAY_PORT=8080

# Path to static site assets
REPLAY_SITE_ROOT=/usr/local/share/replay/site

# Uncomment to override auto-detected storage path
#REPLAY_STORAGE_PATH=/media/sd

# Uncomment to override auto-detected config path
#REPLAY_CONFIG_PATH=/media/sd/config/replay.cfg

# Log level (trace, debug, info, warn, error)
RUST_LOG=replay_control_app=info,replay_control_core=info
```

The environment file is only written on first install. Re-running the installer preserves user customizations.

### CLI args reference

| Arg | Default | Description |
|---|---|---|
| `--port` | `8080` | Port to listen on |
| `--storage-path` | auto-detect | Storage root path override |
| `--config-path` | auto-detect | Path to `replay.cfg` |
| `--site-root` | `target/site` | Path to static assets (`pkg/`, `icons/`, `style.css`) |

### Service management

```bash
systemctl status replay-companion    # Check status
systemctl restart replay-companion   # Restart after config change
journalctl -u replay-companion -f    # Follow logs
```


## 6. mDNS / Avahi

Once the app is deployed, users access it via `http://replaypi.local:8080` in their browser. This relies on mDNS, which RePlayOS supports via Avahi.

The installer drops an Avahi service file at `/etc/avahi/services/replay-companion.service` that advertises the HTTP service for network discovery:

```xml
<?xml version="1.0" standalone='no'?>
<!DOCTYPE service-group SYSTEM "avahi-service.dtd">
<service-group>
  <name>Replay Control</name>
  <service>
    <type>_http._tcp</type>
    <port>8080</port>
  </service>
</service-group>
```

mDNS resolution works out of the box on macOS and most Linux desktops. On Windows, `.local` resolution works when Bonjour is installed (comes with iTunes) or via the built-in LLMNR fallback. If mDNS does not work, the user accesses the Pi by IP address directly.


## 7. Updates

### Via the installer

Re-run the same one-liner. The installer downloads the latest release, deploys it over SSH, and restarts the service. Downtime is a few seconds. The environment file is preserved.

**Unix:**

```bash
curl -fsSL https://raw.githubusercontent.com/user/replay/main/install.sh | bash
```

**Windows:**

```powershell
irm https://raw.githubusercontent.com/user/replay/main/install.ps1 | iex
```

To install a specific version:

```bash
REPLAY_VERSION=v0.2.0 curl -fsSL https://raw.githubusercontent.com/user/replay/main/install.sh | bash
```

```powershell
$env:REPLAY_VERSION="v0.2.0"; irm https://raw.githubusercontent.com/user/replay/main/install.ps1 | iex
```

### Future: self-update from the web UI

Replay Control could check for new releases on startup or periodically and offer a one-click update from the web UI. Under the hood it would download the new artifacts and restart itself via systemd. This removes the need for the user to re-run the installer from their computer.


## 8. Security Considerations

This deployment model automates SSH with publicly known credentials. This is acceptable for RePlayOS's threat model:

- RePlayOS is a single-purpose local appliance, not a multi-user server.
- The credentials are already public -- anyone with network access to the Pi can log in.
- The installer connects over the local network only. No credentials are sent over the internet.
- `StrictHostKeyChecking=no` is used because there is no PKI infrastructure and the threat model does not include MITM attacks on a home LAN.

The PuTTY tools downloaded on Windows are fetched from the official PuTTY distribution site over HTTPS. The installer could verify checksums in a future version.
