# WiFi, NFS & Pi Setup

Manage your Raspberry Pi's network connectivity, storage, hostname, and access. These settings are accessible from the **Settings** page.

## Overview

Configuration settings modify the Pi itself and are essential for network setup, NFS connectivity, and system access. On non-RePlayOS systems (local development), these options are safely skipped.

## WiFi Setup

Configure WiFi connectivity directly from the web UI — no SSH or terminal required.

**What it does:** Sets up your Raspberry Pi to connect to your WiFi network.

**How to use it:**
1. Navigate to **Settings** > **WiFi**
2. Enter your WiFi SSID (network name) and password
3. Select your country code (for regulatory compliance)
4. Choose security mode: WPA2, WPA3, or transitional (auto-detect)
5. Check "Hidden network" if your network doesn't broadcast its name
6. Changes take effect on reboot

**Note:** The password is write-only — it is never sent back to the browser for security.

## NFS Share

Configure network-attached storage for hosting your ROM collection on a desktop, NAS, or other network device.

**What it does:** Mounts an NFS share to use as your game library storage instead of local USB/SD card.

**How to use it:**
1. Navigate to **Settings** > **NFS Share**
2. Enter the NFS server address (hostname or IP, e.g., `nas.local` or `192.168.1.100`)
3. Specify the export path (e.g., `/games` or `/media/roms`)
4. Select NFS version: 3 or 4 (version 4 recommended for modern networks)
5. Changes take effect on reboot

**Tip:** Verify your NFS server is running and the export path exists before rebooting. After reboot, check **Settings** > **System Info** to confirm the storage path shows your NFS mount.

## Hostname

View and change your Raspberry Pi's hostname — the network name others use to access your Pi.

**What it does:** Sets the mDNS address (e.g., `replay.local`) used to connect to the Pi from phones, tablets, and computers.

**How to use it:**
1. Navigate to **Settings** > **Hostname**
2. Enter a new hostname
3. Changes take effect immediately for mDNS

**Example:** Change hostname from `replay` to `replay-living-room` so you can access it at `replay-living-room.local` on your network.

## Change Password

Update the root SSH password for secure Pi access.

**What it does:** Changes the Pi's system password, which protects SSH login and local access.

**How to use it:**
1. Navigate to **Settings** > **Change Password**
2. Enter your current password (required for verification)
3. Enter your new password twice to confirm
4. Changes take effect immediately

**Important:** Choose a strong password. The default `replayos` is publicly known — change it if your Pi is accessible over the internet.

## Related Settings

- **System Info** — view storage path, disk usage, and network addresses (under Settings)
- **System Logs** — troubleshoot connectivity and configuration issues (under Settings)
- **Restart / Reboot** — restart the RePlayOS TV frontend or reboot the entire Pi (under Settings)
