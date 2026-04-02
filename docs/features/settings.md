# Settings

How system configuration and user preferences work.

## Overview

Settings are accessible from the **More** page, organized into Preferences, Game Data, and System sections.

## Preferences

### Region Preference

Primary and secondary preferred ROM region. Options: USA, Europe, Japan, World (default: World). Affects:

- Sort order within system ROM lists (preferred region first)
- Search scoring (preferred region gets a bonus)
- Recommendation deduplication (picks the preferred-region variant when multiple exist)

### Language Preference

Primary and secondary language (e.g., English, Spanish, Japanese). Defaults based on your region preference. Used for sorting game manual search results by language relevance.

### Font Size

Normal or large text. Applied across the entire app.

### Skin / Theme

Browse and apply skins from the [RePlayOS](https://www.replayos.com/) skin collection. Two modes:

- **Sync mode** (default) -- the app matches the skin active on the TV interface. Changes on either side are reflected immediately across all connected browsers.
- **Manual override** -- pick a specific skin for the web UI, independent of the TV skin.

### GitHub API Key

Optional GitHub personal access token for downloading thumbnails. Without a key, GitHub API requests are rate-limited to 60/hour; with one, the limit is 5,000/hour.

## System Configuration

These settings modify the Pi itself. On non-RePlayOS systems (local development), they are safely skipped.

### Hostname

View and change the Pi's hostname and mDNS address (e.g., `replay.local`). Changes take effect immediately for mDNS.

### Change Password

Change the Pi's root SSH password from the web UI. Requires entering the current password for verification.

### Wi-Fi

View and edit Wi-Fi settings: SSID, password, country code, security mode (WPA2/WPA3/transitional), and hidden network flag. The password is write-only -- it is never sent back to the browser. Changes take effect on reboot.

### NFS Share

View and edit NFS mount parameters: server address, export path, NFS version (3 or 4). Changes take effect on reboot.

### System Info

Storage type and path, disk usage, and network addresses.

### System Logs

View RePlayOS system logs with a source filter (All, Companion App, RePlayOS) and refresh button.

### Restart / Reboot

Restart the RePlayOS TV frontend, or reboot the entire Pi. Available from the Wi-Fi, NFS, and More pages.

## Version Display

The app version and git hash are shown in the More page footer.
