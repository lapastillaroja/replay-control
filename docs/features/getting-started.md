# Getting Started

A quick guide to get Replay Control running on your Pi.

{{< screenshot "home.png" "Replay Control home page" >}}

## Prerequisites

- A Raspberry Pi running [RePlayOS](https://www.replayos.com/)
- ROM files for the systems you want to play
- A computer, phone, or tablet on the same network as the Pi

## Install

Open PowerShell, Terminal, or any shell. Connect to the Pi first, then paste the installer:

```bash
ssh root@replay.local
# default password: replayos
curl -fsSL https://raw.githubusercontent.com/lapastillaroja/replay-control/main/install.sh | bash
```

If SSH asks whether you trust the host, type `yes` and press Enter. When it asks for a password, type `replayos`; the password will not appear while typing. The installer downloads the latest stable release, installs the service, and starts Replay Control.

If `replay.local` doesn't resolve, find the Pi's IP in your router's connected-devices list and use `ssh root@<ip>` instead. For other install options (specific version, SD-card install before first boot, running the installer from another computer without SSHing first), see the [Installation guide](install.md).

## First Launch

Open a browser and go to:

```
https://replay.local:8443
```

If `replay.local` does not resolve, use your Pi's IP address instead (e.g., `https://your-pi-ip:8443`).

Replay Control uses a local self-signed HTTPS certificate. Your browser will show a security warning the first time you open it; approve the exception for your Pi to continue.

> **Tip:** To find your Pi's IP address, check your router's connected devices list, or run `hostname -I` on the Pi.

On first launch, you will see a "Scanning game library..." banner while the app indexes your ROMs. This runs in the background -- the UI is usable immediately, and your library fills in as systems are scanned.

A **setup checklist** will also appear on the home page with two optional steps:

- **Download metadata sources** — game descriptions, ratings, and a box art index to enrich your library
- **Enable RePlayOS integration** — connects Replay Control to RePlayOS Net Control so you can launch games from the browser and see what's playing on the TV

{{< screenshot "setup-mobile.png" "First-run setup checklist" >}}

Both are optional — skip and do them later from the [Settings](settings.md) page anytime.

## Add ROMs

Place ROM files in the `roms/` directory on your storage device (SD card, USB drive, or NFS share), organized by system folder. RePlayOS expects a folder per system:

```
roms/
  snes/
    Super Mario World (USA).sfc
    ...
  sega_smd/
    Sonic The Hedgehog (World).md
    ...
  arcade_fbneo/
    sf2.zip
    ...
```

On local storage (SD, USB, NVMe), new ROMs are detected automatically. On NFS, use the "Rescan Game Library" button in the metadata page to reconcile the library after external changes.

## Browse Your Library

The home page shows your recently played games, library stats, and personalized recommendations. Tap any system card to see its games, or use the search tab to find games across all systems.

You can install the app as a PWA (home screen app) on your phone for quick access.

## Enrich Your Library

The app includes built-in data for game names, genres, and player counts. For richer metadata (descriptions, ratings, box art), go to **Settings > Game Data**:

- **Download Metadata** -- imports game descriptions, ratings, and genres from LaunchBox
- **Download Images** -- fetches box art and screenshots from libretro-thumbnails

Both are optional and require an internet connection on the Pi.

## Next Steps

- [Game Library](game-library.md) -- browsing, favorites, multi-disc handling
- [Search](search.md) -- cross-system search and developer pages
- [Settings](settings.md) -- region, skins, font size
- [WiFi, NFS & Pi Setup](configuration.md) -- Wi-Fi, hostname, NFS shares, password
- [Feature overview](index.md) -- full list of features
